use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use lovelace::{Language, ProcessingOptions, TranscriptionOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::State;
use tauri_plugin_dialog::DialogExt;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    WhisperModel,
    LlamaModel,
    WhisperEngine,
    LlamaEngine,
}

impl Kind {
    const ALL: [Self; 4] = [
        Self::WhisperModel,
        Self::LlamaModel,
        Self::WhisperEngine,
        Self::LlamaEngine,
    ];

    fn default_path(self, root: &Path) -> PathBuf {
        root.join(match self {
            Self::WhisperModel => "models/ggml-small-q5_1.bin",
            Self::LlamaModel => "models/qwen2.5-3b-instruct-q4_k_m.gguf",
            Self::WhisperEngine if cfg!(windows) => "engines/whisper/Release/whisper-cli.exe",
            Self::WhisperEngine => "engines/whisper/Release/whisper-cli",
            Self::LlamaEngine if cfg!(windows) => "engines/llama/llama-server.exe",
            Self::LlamaEngine => "engines/llama/llama-server",
        })
    }
}

#[derive(Clone, Deserialize, Serialize)]
struct Asset {
    id: String,
    kind: Kind,
    label: String,
    platform: String,
    #[serde(default = "default_architecture")]
    architecture: String,
    #[serde(default)]
    backend: String,
    #[serde(default)]
    notes: String,
    url: String,
    sha256: String,
    size: u64,
    filename: String,
    executable: Option<String>,
    source: String,
    license: String,
    #[serde(default)]
    runtimes: Vec<DownloadFile>,
}

#[derive(Clone, Deserialize, Serialize)]
struct DownloadFile {
    url: String,
    sha256: String,
    size: u64,
    filename: String,
}

fn default_architecture() -> String {
    "x86_64".into()
}

impl Asset {
    fn download_size(&self) -> u64 {
        self.size + self.runtimes.iter().map(|file| file.size).sum::<u64>()
    }

    fn files(&self) -> Vec<DownloadFile> {
        let mut files = vec![DownloadFile {
            url: self.url.clone(),
            sha256: self.sha256.clone(),
            size: self.size,
            filename: self.filename.clone(),
        }];
        files.extend(self.runtimes.iter().cloned());
        files
    }
}

fn catalog_for(platform: &str, architecture: &str) -> Vec<Asset> {
    let assets: Vec<Asset> = serde_json::from_str(include_str!("model-catalog.json"))
        .expect("Invalid bundled model catalog");
    assets
        .into_iter()
        .filter(|asset| {
            asset.platform == "all"
                || (asset.platform == platform && asset.architecture == architecture)
        })
        .collect()
}

fn catalog() -> Vec<Asset> {
    catalog_for(std::env::consts::OS, std::env::consts::ARCH)
}

#[derive(Clone, Default, Serialize)]
struct Download {
    id: String,
    stage: String,
    received: u64,
    total: u64,
    error: Option<String>,
}

impl Download {
    fn busy(&self) -> bool {
        matches!(
            self.stage.as_str(),
            "downloading" | "installing" | "cancelling"
        )
    }
}

#[derive(Default)]
struct Inner {
    paths: HashMap<Kind, PathBuf>,
    download: Download,
}

pub(super) struct ModelStore {
    root: PathBuf,
    directory: PathBuf,
    config: PathBuf,
    inner: Mutex<Inner>,
    cancel: AtomicBool,
}

#[derive(Serialize)]
struct InstalledAsset {
    #[serde(flatten)]
    asset: Asset,
    path: Option<PathBuf>,
    download_size: u64,
}

#[derive(Serialize)]
struct ActivePath {
    kind: Kind,
    path: PathBuf,
    exists: bool,
}

#[derive(Serialize)]
pub(super) struct Snapshot {
    catalog: Vec<InstalledAsset>,
    active: Vec<ActivePath>,
    download: Download,
    directory: PathBuf,
    job_busy: bool,
}

fn find_executable(directory: &Path, name: &str) -> Result<PathBuf> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_file() && entry.file_name() == name {
            return Ok(entry.path());
        }
        if ty.is_dir() {
            if let Ok(path) = find_executable(&entry.path(), name) {
                return Ok(path);
            }
        }
    }
    bail!("The engine archive does not contain {name}")
}

fn asset_path(directory: &Path, asset: &Asset) -> Result<PathBuf> {
    let path = if let Some(name) = &asset.executable {
        find_executable(directory, name)?
    } else {
        directory.join(&asset.filename)
    };
    ensure!(super::is_regular_file(&path), "Model file is unavailable");
    Ok(path)
}

fn validate_path(kind: Kind, path: &Path) -> Result<PathBuf> {
    let path = path
        .canonicalize()
        .context("The selected file is unavailable")?;
    ensure!(path.is_file(), "Select a file, not a folder");
    match kind {
        Kind::WhisperModel | Kind::LlamaModel => {
            let mut magic = [0; 4];
            fs::File::open(&path)?
                .read_exact(&mut magic)
                .context("The model file is incomplete")?;
            let expected = if kind == Kind::WhisperModel {
                *b"lmgg"
            } else {
                *b"GGUF"
            };
            ensure!(
                magic == expected,
                "Select a compatible {} model",
                if kind == Kind::WhisperModel {
                    "Whisper GGML .bin"
                } else {
                    "GGUF"
                }
            );
        }
        _ => {
            #[cfg(windows)]
            ensure!(
                path.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("exe")),
                "Select an engine executable (.exe)"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                ensure!(
                    fs::metadata(&path)?.permissions().mode() & 0o111 != 0,
                    "The engine file is not executable"
                );
            }
        }
    }
    Ok(path)
}

impl ModelStore {
    pub(super) fn new(root: PathBuf, directory: PathBuf, config: PathBuf) -> Result<Self> {
        let paths = match fs::File::open(&config) {
            Ok(file) => serde_json::from_reader(file).context("Could not read model settings")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            root,
            directory,
            config,
            inner: Mutex::new(Inner {
                paths,
                ..Inner::default()
            }),
            cancel: AtomicBool::new(false),
        })
    }

    fn path(&self, inner: &Inner, kind: Kind) -> PathBuf {
        inner
            .paths
            .get(&kind)
            .cloned()
            .unwrap_or_else(|| kind.default_path(&self.root))
    }

    pub(super) fn processing_options(&self, language: Option<Language>) -> ProcessingOptions {
        let inner = self.inner.lock().unwrap();
        let mut options = ProcessingOptions::desktop(&self.root, language);
        options.transcription.set_paths(
            self.path(&inner, Kind::WhisperEngine),
            self.path(&inner, Kind::WhisperModel),
        );
        options.set_summary_paths(
            self.path(&inner, Kind::LlamaEngine),
            self.path(&inner, Kind::LlamaModel),
        );
        options
    }

    pub(super) fn transcription_options(&self, language: Option<Language>) -> TranscriptionOptions {
        self.processing_options(language).transcription
    }

    pub(super) fn downloading(&self) -> bool {
        self.inner.lock().unwrap().download.busy()
    }

    fn snapshot(&self, job_busy: bool) -> Snapshot {
        let inner = self.inner.lock().unwrap();
        Snapshot {
            catalog: catalog()
                .into_iter()
                .map(|asset| {
                    let path = asset_path(&self.directory.join(&asset.id), &asset)
                        .and_then(|path| Ok(path.canonicalize()?))
                        .ok();
                    let download_size = asset.download_size();
                    InstalledAsset {
                        asset,
                        path,
                        download_size,
                    }
                })
                .collect(),
            active: Kind::ALL
                .into_iter()
                .map(|kind| {
                    let path = self.path(&inner, kind);
                    let path = path.canonicalize().unwrap_or(path);
                    let exists = path.is_file();
                    ActivePath { kind, path, exists }
                })
                .collect(),
            download: inner.download.clone(),
            directory: self.directory.clone(),
            job_busy,
        }
    }

    fn save_paths(&self, paths: &HashMap<Kind, PathBuf>) -> Result<()> {
        let parent = self
            .config
            .parent()
            .context("Model settings folder is unavailable")?;
        fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(file.as_file_mut(), paths)?;
        file.as_file().sync_all()?;
        file.persist(&self.config)
            .context("Could not save model settings")?;
        Ok(())
    }

    fn select(&self, kind: Kind, path: Option<PathBuf>) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        ensure!(
            !inner.download.busy(),
            "Wait for the download to finish or cancel it first"
        );
        let mut paths = inner.paths.clone();
        if let Some(path) = path {
            paths.insert(kind, validate_path(kind, &path)?);
        } else {
            paths.remove(&kind);
        }
        self.save_paths(&paths)?;
        inner.paths = paths;
        inner.download = Download::default();
        Ok(())
    }

    fn remove(&self, id: &str) -> Result<()> {
        let asset = catalog()
            .into_iter()
            .find(|asset| asset.id == id)
            .context("Unknown download")?;
        let mut inner = self.inner.lock().unwrap();
        ensure!(
            !inner.download.busy(),
            "Wait for the download to finish or cancel it first"
        );
        // Only catalog IDs can name a managed directory. Never delete a user-selected path.
        let root = self.directory.canonicalize()?;
        let directory = root.join(&asset.id);
        ensure!(
            fs::symlink_metadata(&directory)?.is_dir() && directory.canonicalize()? == directory,
            "Invalid managed model folder"
        );
        let installed = asset_path(&directory, &asset)?;
        let mut paths = inner.paths.clone();
        if self.path(&inner, asset.kind).canonicalize().ok() == installed.canonicalize().ok() {
            paths.remove(&asset.kind);
            self.save_paths(&paths)?;
            inner.paths = paths;
        }
        fs::remove_dir_all(directory).context("Could not delete the download")?;
        if inner.download.id == id {
            inner.download = Download::default();
        }
        Ok(())
    }

    fn check_cancelled(&self) -> Result<()> {
        ensure!(!self.cancel.load(Ordering::Relaxed), "Download cancelled");
        Ok(())
    }

    fn install(&self, asset: &Asset) -> Result<()> {
        fs::create_dir_all(&self.directory)?;
        let staging = tempfile::Builder::new()
            .prefix(".download-")
            .tempdir_in(&self.directory)?;
        let agent = ureq::Agent::config_builder()
            .https_only(true)
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_per_read(Some(Duration::from_secs(15)))
            .build()
            .new_agent();
        let files = asset.files();
        let mut offset = 0;
        for (index, file) in files.iter().enumerate() {
            self.check_cancelled()?;
            let archive = staging.path().join(format!("download-{index}"));
            let mut response = agent
                .get(&file.url)
                .header("User-Agent", "Lovelace model manager")
                .call()
                .with_context(|| {
                    format!(
                        "Could not download {}. Check your internet connection",
                        file.filename
                    )
                })?;
            copy_verified(
                response.body_mut().as_reader(),
                fs::File::create(&archive)?,
                file.size,
                &file.sha256,
                |received| {
                    self.check_cancelled()?;
                    self.inner.lock().unwrap().download.received = offset + received;
                    Ok(())
                },
            )?;
            offset += file.size;
        }
        self.check_cancelled()?;
        self.inner.lock().unwrap().download.stage = "installing".into();
        let extracted = staging.path().join("files");
        fs::create_dir(&extracted)?;
        let archive = staging.path().join("download-0");
        if asset.executable.is_some() {
            extract_archive(
                &archive,
                &extracted,
                asset.filename.ends_with(".zip"),
                || self.check_cancelled(),
            )?;
        } else {
            fs::rename(&archive, extracted.join(&asset.filename))?;
        }
        let path = asset_path(&extracted, asset)?;
        // CUDA runtime packages have different top-level folders on Linux.
        // Collect their library files beside llama-server, whose rpath is $ORIGIN.
        for (index, file) in files.iter().enumerate().skip(1) {
            let runtime = staging.path().join(format!("runtime-{index}"));
            fs::create_dir(&runtime)?;
            extract_archive(
                &staging.path().join(format!("download-{index}")),
                &runtime,
                file.filename.ends_with(".zip"),
                || self.check_cancelled(),
            )?;
            merge_runtime_files(
                &runtime,
                path.parent().context("Engine folder is unavailable")?,
                || self.check_cancelled(),
            )?;
        }
        validate_path(asset.kind, &path)?;
        self.check_cancelled()?;
        let destination = self.directory.join(&asset.id);
        ensure!(
            !destination.exists(),
            "This download is already installed. Delete it before downloading again"
        );
        fs::rename(&extracted, &destination)?;
        // Resolve only after moving the staging folder; temporary paths are never persisted.
        let installed = asset_path(&destination, asset)?.canonicalize()?;
        let mut inner = self.inner.lock().unwrap();
        let mut paths = inner.paths.clone();
        paths.insert(asset.kind, installed);
        self.save_paths(&paths).context(
            "Download installed, but could not activate it. Select Use in Settings to retry",
        )?;
        inner.paths = paths;
        Ok(())
    }

    fn start(self: &Arc<Self>, id: &str) -> Result<()> {
        let asset = catalog()
            .into_iter()
            .find(|asset| asset.id == id)
            .context("Unknown download")?;
        let mut inner = self.inner.lock().unwrap();
        ensure!(!inner.download.busy(), "A download is already in progress");
        ensure!(
            !self.directory.join(&asset.id).exists(),
            "This download is already installed"
        );
        self.cancel.store(false, Ordering::Relaxed);
        inner.download = Download {
            id: asset.id.clone(),
            stage: "downloading".into(),
            total: asset.download_size(),
            ..Download::default()
        };
        let store = Arc::clone(self);
        if let Err(error) = std::thread::Builder::new()
            .name("model-download".into())
            .spawn(move || {
                let result = store.install(&asset);
                let mut inner = store.inner.lock().unwrap();
                match result {
                    Ok(()) => {
                        inner.download.stage = "done".into();
                        inner.download.error = None;
                    }
                    Err(error) => {
                        inner.download.stage = if store.cancel.load(Ordering::Relaxed) {
                            "cancelled"
                        } else {
                            "failed"
                        }
                        .into();
                        inner.download.error = Some(format!("{error:#}"));
                    }
                }
            })
        {
            inner.download.stage = "failed".into();
            inner.download.error = Some(error.to_string());
            return Err(error.into());
        }
        Ok(())
    }
}

fn copy_verified(
    mut reader: impl Read,
    mut output: fs::File,
    size: u64,
    checksum: &str,
    mut progress: impl FnMut(u64) -> Result<()>,
) -> Result<()> {
    let mut hasher = Sha256::new();
    let mut received = 0;
    let mut buffer = [0; 128 * 1024];
    loop {
        progress(received)?;
        let count = reader
            .read(&mut buffer)
            .context("Download interrupted. Retry to download again")?;
        if count == 0 {
            break;
        }
        received += count as u64;
        ensure!(received <= size, "Downloaded file is larger than expected");
        output
            .write_all(&buffer[..count])
            .context("Could not write download. Check available disk space")?;
        hasher.update(&buffer[..count]);
    }
    ensure!(
        received == size,
        "Incomplete download: expected {size} bytes, received {received}"
    );
    ensure!(
        format!("{:x}", hasher.finalize()) == checksum,
        "Download checksum did not match. Please retry"
    );
    output.sync_all()?;
    Ok(())
}

fn merge_runtime_files(
    directory: &Path,
    destination: &Path,
    check: impl Fn() -> Result<()> + Copy,
) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        check()?;
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            merge_runtime_files(&entry.path(), destination, check)?;
        } else {
            ensure!(ty.is_file(), "Unexpected link in runtime package");
            let target = destination.join(entry.file_name());
            ensure!(
                !target.try_exists()?,
                "Runtime package would overwrite {}",
                target.display()
            );
            fs::rename(entry.path(), target)?;
        }
    }
    Ok(())
}

fn extract_archive(
    archive: &Path,
    destination: &Path,
    zipped: bool,
    check: impl Fn() -> Result<()>,
) -> Result<()> {
    if zipped {
        let mut zip = zip::ZipArchive::new(fs::File::open(archive)?)?;
        for index in 0..zip.len() {
            check()?;
            let mut entry = zip.by_index(index)?;
            let path = entry.enclosed_name().context("Invalid archive path")?;
            ensure!(!entry.is_symlink(), "Unexpected link in engine ZIP");
            let path = destination.join(path);
            if entry.is_dir() {
                fs::create_dir_all(path)?;
            } else {
                fs::create_dir_all(path.parent().context("Invalid archive path")?)?;
                std::io::copy(&mut entry, &mut fs::File::create(path)?)?;
            }
        }
    } else {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(archive)?));
        for entry in tar.entries()? {
            check()?;
            ensure!(
                entry?.unpack_in(destination)?,
                "Invalid engine archive path"
            );
        }
    }
    Ok(())
}

#[tauri::command]
pub(super) fn get_models(state: State<'_, super::Desktop>) -> Snapshot {
    let job = state.job.lock().unwrap();
    state.models.snapshot(job.status.phase.busy())
}

#[tauri::command]
pub(super) fn download_model(id: String, state: State<'_, super::Desktop>) -> Result<(), String> {
    let job = state.job.lock().unwrap();
    if job.status.phase.busy() {
        return Err("Wait for the active job to finish".into());
    }
    state.models.start(&id).map_err(super::error_message)
}

#[tauri::command]
pub(super) fn cancel_model_download(state: State<'_, super::Desktop>) {
    let mut inner = state.models.inner.lock().unwrap();
    if inner.download.busy() {
        state.models.cancel.store(true, Ordering::Relaxed);
        inner.download.stage = "cancelling".into();
    }
}

#[tauri::command]
pub(super) fn use_model_path(
    kind: Kind,
    path: Option<PathBuf>,
    state: State<'_, super::Desktop>,
) -> Result<(), String> {
    let job = state.job.lock().unwrap();
    if job.status.phase.busy() {
        return Err("Wait for the active job to finish".into());
    }
    state
        .models
        .select(kind, path)
        .map_err(super::error_message)
}

#[tauri::command]
pub(super) async fn delete_model(
    id: String,
    state: State<'_, super::Desktop>,
) -> Result<(), String> {
    let models = Arc::clone(&state.models);
    let job = Arc::clone(&state.job);
    tauri::async_runtime::spawn_blocking(move || {
        let job = job.lock().unwrap();
        ensure!(
            !job.status.phase.busy(),
            "Wait for the active job to finish"
        );
        models.remove(&id)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(super::error_message)
}

#[tauri::command]
pub(super) async fn pick_model_file(
    kind: Kind,
    window: tauri::WebviewWindow,
) -> Result<Option<PathBuf>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dialog = window
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("Select a local model or engine");
        let dialog = match kind {
            Kind::WhisperModel => dialog.add_filter("Whisper model", &["bin"]),
            Kind::LlamaModel => dialog.add_filter("GGUF model", &["gguf"]),
            _ if cfg!(windows) => dialog.add_filter("Engine executable", &["exe"]),
            _ => dialog,
        };
        dialog
            .blocking_pick_file()
            .map(|file| file.into_path())
            .transpose()
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(root: &Path) -> ModelStore {
        ModelStore::new(
            root.into(),
            root.join("managed"),
            root.join("config/models.json"),
        )
        .unwrap()
    }

    #[test]
    fn catalog_has_unique_pinned_urls_and_checksums() {
        let mut ids = std::collections::HashSet::new();
        let assets: Vec<Asset> = serde_json::from_str(include_str!("model-catalog.json")).unwrap();
        for asset in assets {
            assert!(ids.insert(asset.id.clone()));
            for file in asset.files() {
                assert!(file.url.starts_with("https://"));
                assert!(!file.url.contains("/main/"));
                assert_eq!(file.sha256.len(), 64);
                assert!(file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()));
                assert!(file.size > 0);
            }
        }
    }

    #[test]
    fn engine_choices_match_platform_and_architecture_and_include_cpu_fallback() {
        for platform in ["windows", "linux"] {
            for architecture in ["x86_64", "aarch64"] {
                let assets = catalog_for(platform, architecture);
                let engines: Vec<_> = assets
                    .iter()
                    .filter(|asset| asset.kind == Kind::LlamaEngine)
                    .collect();
                assert!(
                    engines
                        .iter()
                        .all(|asset| asset.platform == platform
                            && asset.architecture == architecture)
                );
                for backend in ["cpu", "vulkan"] {
                    assert!(
                        engines.iter().any(|asset| asset.backend == backend),
                        "Missing {backend} on {platform} {architecture}"
                    );
                }
                for asset in &engines {
                    assert!(!asset.notes.is_empty());
                    if asset.backend.starts_with("cuda-") {
                        assert_eq!(asset.runtimes.len(), 1);
                        assert!(asset.runtimes[0].filename.starts_with("cudart-llama-"));
                        assert_eq!(asset.download_size(), asset.size + asset.runtimes[0].size);
                    }
                }
                if architecture == "x86_64" {
                    for backend in ["rocm", "openvino"] {
                        assert!(engines.iter().any(|asset| asset.backend == backend));
                    }
                    assert!(
                        engines
                            .iter()
                            .any(|asset| asset.backend.starts_with("sycl"))
                    );
                    // Keep IDs used by existing installations and saved paths.
                    assert!(
                        engines
                            .iter()
                            .any(|asset| asset.id == format!("llama-b12289-{platform}"))
                    );
                }
            }
        }
        assert!(
            catalog_for("linux", "riscv64")
                .iter()
                .all(|asset| asset.platform == "all")
        );
    }

    #[test]
    fn cuda_runtime_libraries_are_merged_beside_the_engine_without_overwriting_it() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let engine = temp.path().join("engine/bin");
        let runtime = temp.path().join("runtime/cudart-llama-b12289");
        fs::create_dir_all(&engine)?;
        fs::create_dir_all(&runtime)?;
        fs::write(engine.join("llama-server"), b"engine")?;
        for file in ["libcudart.so.12", "libcublas.so.12", "libcublasLt.so.12"] {
            fs::write(runtime.join(file), b"library")?;
        }
        merge_runtime_files(&temp.path().join("runtime"), &engine, || Ok(()))?;
        assert_eq!(fs::read(engine.join("libcublas.so.12"))?, b"library");
        assert_eq!(fs::read(engine.join("llama-server"))?, b"engine");
        fs::write(runtime.join("llama-server"), b"overwrite")?;
        assert!(merge_runtime_files(&runtime, &engine, || Ok(())).is_err());
        assert_eq!(fs::read(engine.join("llama-server"))?, b"engine");
        assert!(merge_runtime_files(&runtime, &engine, || bail!("Cancelled")).is_err());
        Ok(())
    }

    #[test]
    fn selections_persist_and_invalid_or_missing_files_leave_settings_unchanged() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = store(temp.path());
        let whisper = temp.path().join("my-whisper.bin");
        let llama = temp.path().join("my-summary.gguf");
        fs::write(&whisper, b"lmgg-whisper")?;
        fs::write(&llama, b"GGUF-summary")?;
        store.select(Kind::WhisperModel, Some(whisper.clone()))?;
        store.select(Kind::LlamaModel, Some(llama.clone()))?;
        assert!(
            store
                .select(Kind::LlamaModel, Some(whisper.clone()))
                .is_err()
        );
        assert!(
            store
                .select(Kind::WhisperModel, Some(temp.path().join("missing")))
                .is_err()
        );
        let reopened = self::store(temp.path());
        let snapshot = reopened.snapshot(false);
        assert_eq!(
            snapshot
                .active
                .iter()
                .find(|path| path.kind == Kind::WhisperModel)
                .unwrap()
                .path,
            whisper.canonicalize()?
        );
        assert_eq!(
            snapshot
                .active
                .iter()
                .find(|path| path.kind == Kind::LlamaModel)
                .unwrap()
                .path,
            llama.canonicalize()?
        );
        reopened.select(Kind::WhisperModel, None)?;
        assert!(whisper.is_file());
        assert!(
            !self::store(temp.path())
                .inner
                .lock()
                .unwrap()
                .paths
                .contains_key(&Kind::WhisperModel)
        );
        Ok(())
    }

    #[test]
    fn deletion_is_limited_to_catalog_downloads_and_preserves_external_files() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = store(temp.path());
        let asset = catalog()
            .into_iter()
            .find(|asset| asset.id == "whisper-tiny")
            .unwrap();
        let directory = store.directory.join(&asset.id);
        fs::create_dir_all(&directory)?;
        let managed = directory.join(&asset.filename);
        fs::write(&managed, b"lmgg-managed")?;
        let external = temp.path().join("external.bin");
        fs::write(&external, b"lmgg-external")?;
        store.select(Kind::WhisperModel, Some(external.clone()))?;
        assert!(store.remove("../external.bin").is_err());
        store.remove(&asset.id)?;
        assert!(external.is_file());
        assert_eq!(
            store.inner.lock().unwrap().paths[&Kind::WhisperModel],
            external.canonicalize()?
        );
        fs::create_dir_all(&directory)?;
        fs::write(&managed, b"lmgg-managed")?;
        store.select(Kind::WhisperModel, Some(managed))?;
        store.remove(&asset.id)?;
        assert!(!directory.exists());
        assert!(
            !store
                .inner
                .lock()
                .unwrap()
                .paths
                .contains_key(&Kind::WhisperModel)
        );
        Ok(())
    }

    #[test]
    fn streaming_rejects_truncation_corruption_oversize_and_cancellation() -> Result<()> {
        let data = b"GGUF-test-model";
        let checksum = format!("{:x}", Sha256::digest(data));
        let temp = tempfile::tempdir()?;
        let output = || fs::File::create(temp.path().join("download")).unwrap();
        let mut received = 0;
        copy_verified(&data[..], output(), data.len() as u64, &checksum, |bytes| {
            received = bytes;
            Ok(())
        })?;
        assert_eq!(received, data.len() as u64);
        assert!(
            copy_verified(&data[..4], output(), data.len() as u64, &checksum, |_| Ok(
                ()
            ))
            .is_err()
        );
        assert!(
            copy_verified(
                &data[..],
                output(),
                data.len() as u64,
                &"0".repeat(64),
                |_| Ok(())
            )
            .is_err()
        );
        assert!(copy_verified(&data[..], output(), 1, &checksum, |_| Ok(())).is_err());
        assert!(
            copy_verified(
                &data[..],
                output(),
                data.len() as u64,
                &checksum,
                |_| bail!("Cancelled")
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn zip_extraction_preserves_engine_libraries_and_rejects_traversal() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let archive = temp.path().join("engine.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive)?);
        for name in ["Release/whisper-cli.exe", "Release/whisper.dll"] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())?;
            zip.write_all(b"engine")?;
        }
        zip.finish()?;
        let destination = temp.path().join("installed");
        fs::create_dir(&destination)?;
        extract_archive(&archive, &destination, true, || Ok(()))?;
        assert!(find_executable(&destination, "whisper-cli.exe")?.is_file());
        assert!(destination.join("Release/whisper.dll").is_file());
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive)?);
        zip.start_file("../outside", zip::write::SimpleFileOptions::default())?;
        zip.write_all(b"invalid")?;
        zip.finish()?;
        assert!(extract_archive(&archive, &destination, true, || Ok(())).is_err());
        assert!(!temp.path().join("outside").exists());
        Ok(())
    }

    #[test]
    fn download_blocks_mutations_and_job_launches() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Arc::new(store(temp.path()));
        store.inner.lock().unwrap().download.stage = "downloading".into();
        assert!(store.downloading());
        assert!(store.select(Kind::WhisperModel, None).is_err());
        assert!(store.start("whisper-tiny").is_err());
        let desktop = super::super::Desktop {
            root: temp.path().into(),
            models: store,
            job: Arc::default(),
            level: Arc::default(),
            audio_inputs: Mutex::default(),
        };
        assert!(
            desktop
                .launch(super::super::Phase::Preparing, |_| Ok(()))
                .is_err()
        );
        Ok(())
    }

    #[test]
    #[ignore = "Downloads official engines and Whisper Tiny; requires internet"]
    fn official_downloads_install_and_select_working_engines() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = store(temp.path());
        for asset in catalog().into_iter().filter(|asset| {
            asset.id == "whisper-tiny"
                || asset.kind == Kind::WhisperEngine
                || (asset.kind == Kind::LlamaEngine && asset.backend == "cpu")
        }) {
            store
                .install(&asset)
                .with_context(|| format!("Could not install {}", asset.id))?;
            let snapshot = store.snapshot(false);
            let path = &snapshot
                .active
                .iter()
                .find(|path| path.kind == asset.kind)
                .unwrap()
                .path;
            assert!(path.is_file());
            assert!(!path.to_string_lossy().contains(".download-"));
            if asset.executable.is_some() {
                let mut command = std::process::Command::new(path);
                command.arg("--help");
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    command.creation_flags(0x08000000);
                }
                let output = command.output()?;
                ensure!(
                    output.status.success(),
                    "Engine could not start: {:?}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
        assert!(!fs::read_dir(&store.directory)?.any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".download-")
        }));
        Ok(())
    }
}
