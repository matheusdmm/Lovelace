#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::{
    collections::HashMap,
    env, fs,
    io::{ErrorKind, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    thread,
};

use anyhow::{Context, Result, bail, ensure};
use cpal::traits::DeviceTrait;
use lovelace::{Language, ProcessingOptions, audio, captions, new_session, pipeline};
use pulldown_cmark::{Event, Parser, Tag, TagEnd, html};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State, WindowEvent};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_window_state::StateFlags;

#[path = "desktop/models.rs"]
mod models;
#[path = "desktop/settings.rs"]
mod settings;

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    #[default]
    Idle,
    Starting,
    Recording,
    Saving,
    Preparing,
    Extracting,
    Transcribing,
    Synthesizing,
    Done,
    Failed,
}

impl Phase {
    fn busy(self) -> bool {
        !matches!(self, Self::Idle | Self::Done | Self::Failed)
    }
}

#[derive(Clone, Default, Serialize)]
struct Status {
    phase: Phase,
    session: Option<String>,
    error: Option<String>,
    level: f32,
    captions: bool,
    subtitle_path: Option<PathBuf>,
    refresh_audio_inputs: bool,
    restarting: bool,
    attempt: u64,
    engine_log: String,
}

#[derive(Default)]
struct Job {
    status: Status,
    stop: Option<mpsc::SyncSender<Result<()>>>,
    control: Arc<lovelace::job::Control>,
    restart_language: Option<Language>,
}

struct Desktop {
    root: PathBuf,
    models: Arc<models::ModelStore>,
    job: Arc<Mutex<Job>>,
    level: Arc<AtomicU32>,
    audio_inputs: Mutex<AudioInputs>,
}

#[derive(Default)]
struct AudioInputs {
    revision: u64,
    devices: HashMap<String, cpal::Device>,
}

#[derive(Serialize)]
struct AudioInput {
    id: String,
    name: String,
}

impl Desktop {
    fn sessions(&self) -> PathBuf {
        self.root.join("sessoes")
    }

    fn session(&self, id: &str) -> Result<PathBuf> {
        let (timestamp, suffix) = id
            .strip_prefix("sessao-")
            .and_then(|value| value.split_once('-'))
            .context("Invalid session ID")?;
        ensure!(
            timestamp.parse::<u64>().is_ok() && suffix.parse::<u64>().is_ok(),
            "Invalid session ID"
        );
        let root = self.sessions().canonicalize()?;
        let path = root.join(id);
        ensure!(
            fs::symlink_metadata(&path)?.is_dir() && path.canonicalize()? == path,
            "Invalid session folder"
        );
        Ok(path)
    }

    fn set_favorite(&self, id: &str, favorite: bool) -> Result<()> {
        let path = self.session(id)?;
        wave_file(&path)?;
        let marker = path.join(".favorite");
        if favorite {
            match fs::File::create_new(&marker) {
                Ok(_) => Ok(()),
                Err(error)
                    if error.kind() == ErrorKind::AlreadyExists && is_regular_file(&marker) =>
                {
                    Ok(())
                }
                Err(error) => Err(error).context("Could not favorite the session"),
            }
        } else {
            match fs::remove_file(&marker) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error).context("Could not remove the favorite"),
            }
        }
    }

    fn delete_session(&self, id: &str) -> Result<()> {
        let job = self.job.lock().unwrap();
        ensure!(
            !job.status.phase.busy(),
            "Wait for the active job to finish before deleting a session"
        );
        let path = self.session(id)?;
        wave_file(&path)?;
        fs::remove_dir_all(path).context("Could not delete all session files")
    }

    fn launch(
        &self,
        phase: Phase,
        task: impl FnOnce(&Mutex<Job>) -> Result<()> + Send + 'static,
    ) -> Result<()> {
        self.launch_job(
            Status {
                phase,
                ..Status::default()
            },
            task,
        )
    }

    fn launch_job(
        &self,
        status: Status,
        task: impl FnOnce(&Mutex<Job>) -> Result<()> + Send + 'static,
    ) -> Result<()> {
        let mut job = self.job.lock().unwrap();
        ensure!(!job.status.phase.busy(), "A session is already in progress");
        ensure!(
            !self.models.downloading(),
            "Finish or cancel the model download in Settings first"
        );
        let previous = job.status.clone();
        job.status = status;
        job.status.attempt = previous.attempt + 1;
        job.control = Arc::default();
        job.restart_language = None;
        self.level.store(0, Ordering::Relaxed);

        let shared = Arc::clone(&self.job);
        let root = self.root.clone();
        let models = Arc::clone(&self.models);
        if let Err(error) = thread::Builder::new().name("sessao".into()).spawn(move || {
            let mut result = task(&shared);
            loop {
                let mut job = shared.lock().unwrap();
                if let Some(language) = job.restart_language.take() {
                    let id = job.status.session.clone().unwrap();
                    job.control = Arc::default();
                    job.status.phase = Phase::Preparing;
                    job.status.attempt += 1;
                    job.status.restarting = false;
                    job.status.error = None;
                    let mut options = models.processing_options(Some(language));
                    options.set_control(Arc::clone(&job.control));
                    drop(job);
                    result = process_mode(&shared, &root.join("sessoes").join(id), &options, true);
                    continue;
                }
                job.stop = None;
                match result {
                    Ok(()) => job.status.phase = Phase::Done,
                    Err(error) => {
                        job.status.phase = Phase::Failed;
                        job.status.error = Some(format!("{error:#}"));
                    }
                }
                break;
            }
        }) {
            job.status = previous;
            return Err(error).context("Could not start the session");
        }
        Ok(())
    }
}

#[derive(Serialize)]
struct Session {
    id: String,
    timestamp: u64,
    has_summary: bool,
    has_transcript: bool,
    favorite: bool,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SessionSource {
    Folder,
    Summary,
    Transcript,
}

impl SessionSource {
    fn path(self, folder: PathBuf) -> Result<PathBuf> {
        let name = match self {
            Self::Folder => return Ok(folder),
            Self::Summary => "sintese.md",
            Self::Transcript => "transcricao.md",
        };
        let path = folder.join(name);
        ensure!(is_regular_file(&path), "Session source file is unavailable");
        Ok(path)
    }
}

#[derive(Serialize)]
struct Document {
    markdown: String,
    html: String,
    segment_paragraphs: Vec<usize>,
}

#[derive(Serialize)]
struct Review {
    audio_path: PathBuf,
    summary: Option<Document>,
    transcript: Option<Document>,
}

fn error_message(error: anyhow::Error) -> String {
    format!("{error:#}")
}

fn is_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
}

fn list_sessions(state: &Desktop) -> Result<Vec<Session>> {
    let entries = match fs::read_dir(state.sessions()) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("Could not open session history"),
    };
    let mut sessions = Vec::new();
    for entry in entries {
        let entry = entry?;
        let id = entry.file_name().to_string_lossy().into_owned();
        let Some((timestamp, suffix)) =
            id.strip_prefix("sessao-").and_then(|id| id.split_once('-'))
        else {
            continue;
        };
        let (Ok(timestamp), Ok(suffix)) = (timestamp.parse(), suffix.parse::<u64>()) else {
            continue;
        };
        if entry.file_type()?.is_dir() && wave_file(&entry.path()).is_ok() {
            sessions.push((
                suffix,
                Session {
                    id,
                    timestamp,
                    has_summary: is_regular_file(&entry.path().join("sintese.md")),
                    has_transcript: is_regular_file(&entry.path().join("transcricao.md")),
                    favorite: is_regular_file(&entry.path().join(".favorite")),
                },
            ));
        }
    }
    sessions
        .sort_unstable_by(|(a, left), (b, right)| (right.timestamp, b).cmp(&(left.timestamp, a)));
    Ok(sessions.into_iter().map(|(_, session)| session).collect())
}

fn wave_file(session: &Path) -> Result<PathBuf> {
    let path = session.join("audio.wav");
    let metadata = fs::symlink_metadata(&path).context("Session audio is unavailable")?;
    ensure!(
        metadata.is_file() && metadata.len() > 44,
        "Session audio is incomplete"
    );
    Ok(path)
}

fn read_document(path: PathBuf) -> Result<Option<Document>> {
    match fs::symlink_metadata(&path) {
        Ok(metadata) => ensure!(metadata.is_file(), "Invalid session document"),
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let markdown = fs::read_to_string(&path)?;
    let (markdown, segment_paragraphs) = if path
        .file_name()
        .is_some_and(|name| name == "transcricao.md")
    {
        let whisper = fs::read(path.with_file_name("whisper.json")).ok();
        let transcript = lovelace::transcript::for_review(&markdown, whisper.as_deref());
        (transcript.markdown, transcript.segment_paragraphs)
    } else {
        (markdown, Vec::new())
    };
    let mut rendered = String::new();
    let events = Parser::new(&markdown).filter_map(|event| match event {
        Event::Html(text) | Event::InlineHtml(text) => Some(Event::Text(text)),
        Event::Start(Tag::Image { .. }) | Event::End(TagEnd::Image) => None,
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let valid = dest_url
                .strip_prefix("transcricao.md#")
                .and_then(|fragment| {
                    fragment
                        .strip_prefix("segment-")
                        .or_else(|| fragment.strip_prefix("trecho-"))
                })
                .is_some_and(|id| !id.is_empty() && id.bytes().all(|digit| digit.is_ascii_digit()));
            Some(Event::Start(Tag::Link {
                link_type,
                dest_url: if valid { dest_url } else { "#".into() },
                title,
                id,
            }))
        }
        event => Some(event),
    });
    html::push_html(&mut rendered, events);
    Ok(Some(Document {
        markdown,
        html: rendered,
        segment_paragraphs,
    }))
}

#[tauri::command]
async fn history(state: State<'_, Desktop>) -> Result<Vec<Session>, String> {
    list_sessions(&state).map_err(error_message)
}

#[tauri::command]
fn set_favorite(id: String, favorite: bool, state: State<'_, Desktop>) -> Result<(), String> {
    state.set_favorite(&id, favorite).map_err(error_message)
}

#[tauri::command]
async fn delete_session(id: String, state: State<'_, Desktop>) -> Result<(), String> {
    state.delete_session(&id).map_err(error_message)
}

#[tauri::command]
async fn open_session_source(
    id: String,
    source: SessionSource,
    state: State<'_, Desktop>,
) -> Result<(), String> {
    let path = state
        .session(&id)
        .and_then(|folder| source.path(folder))
        .map_err(error_message)?;
    tauri::async_runtime::spawn_blocking(move || open_in_system(&path))
        .await
        .map_err(|error| error.to_string())?
        .map_err(error_message)
}

#[cfg(windows)]
fn open_in_system(path: &Path) -> Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr::null_mut};
    use windows_sys::Win32::{
        System::Com::{
            COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
        },
        UI::{
            Shell::{SE_ERR_NOASSOC, ShellExecuteW},
            WindowsAndMessaging::SW_SHOWNORMAL,
        },
    };

    let path = dunce::simplified(path);
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // Initialize COM for shell extensions on this worker thread. The path
    // stays null-terminated and alive until ShellExecuteW returns.
    let result = unsafe {
        let initialized = CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        );
        ensure!(
            initialized >= 0,
            "Could not initialize the Windows file opener (0x{initialized:08x})"
        );
        let result = ShellExecuteW(
            null_mut(),
            std::ptr::null(),
            wide.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
        CoUninitialize();
        result as usize
    };
    if result == SE_ERR_NOASSOC as usize && path.extension().is_some_and(|ext| ext == "md") {
        std::process::Command::new("notepad.exe")
            .arg(path)
            .spawn()
            .context("Could not open the Markdown file")?;
        return Ok(());
    }
    ensure!(
        result > 32,
        "Windows could not open the session source (code {result})"
    );
    Ok(())
}

#[cfg(not(windows))]
fn open_in_system(path: &Path) -> Result<()> {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let output = std::process::Command::new(opener)
        .arg(path)
        .output()
        .context("Could not start the system file opener")?;
    ensure!(
        output.status.success(),
        "Could not open the session source: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

#[tauri::command]
async fn review(
    id: String,
    app: tauri::AppHandle,
    state: State<'_, Desktop>,
) -> Result<Review, String> {
    let load = || -> Result<Review> {
        let session = state.session(&id)?;
        let audio_path = wave_file(&session)?.canonicalize()?;
        app.asset_protocol_scope().allow_file(&audio_path)?;
        Ok(Review {
            audio_path,
            summary: read_document(session.join("sintese.md"))?,
            transcript: read_document(session.join("transcricao.md"))?,
        })
    };
    load().map_err(error_message)
}

#[tauri::command]
fn status(state: State<'_, Desktop>) -> Status {
    let mut status = state.job.lock().unwrap().status.clone();
    if let Some(id) = &status.session {
        if let Ok(mut file) = fs::File::open(state.sessions().join(id).join("whisper.log")) {
            if let Ok(metadata) = file.metadata() {
                let _ = file.seek(SeekFrom::Start(metadata.len().saturating_sub(8192)));
                let mut bytes = Vec::new();
                if file.take(8192).read_to_end(&mut bytes).is_ok() {
                    status.engine_log = String::from_utf8_lossy(&bytes).into_owned();
                }
            }
        }
    }
    if matches!(status.phase, Phase::Recording) {
        status.level = f32::from_bits(state.level.swap(0, Ordering::Relaxed));
    }
    status
}

fn selected_language(language: Option<&str>) -> Result<Option<Language>> {
    Ok(match language {
        Some("pt") => Some(Language::Pt),
        Some("en") => Some(Language::En),
        Some("auto") => Some(Language::Auto),
        None => None,
        _ => bail!("Invalid spoken language"),
    })
}

fn process(job: &Mutex<Job>, session: &Path, options: &ProcessingOptions) -> Result<()> {
    process_mode(job, session, options, false)
}

fn process_mode(
    job: &Mutex<Job>,
    session: &Path,
    options: &ProcessingOptions,
    restart: bool,
) -> Result<()> {
    pipeline::process_mode_with_progress(session, options, restart, |stage| {
        job.lock().unwrap().status.phase = match stage {
            pipeline::Stage::Transcribing => Phase::Transcribing,
            pipeline::Stage::Synthesizing => Phase::Synthesizing,
        };
    })
    .context("Audio saved. You can try processing the session again")
}

#[tauri::command]
async fn pick_caption_media(window: tauri::WebviewWindow) -> Result<Option<PathBuf>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        window
            .dialog()
            .file()
            .set_parent(&window)
            .set_title("Select audio or video")
            .add_filter(
                "Audio and video",
                &[
                    "wav", "mp3", "m4a", "aac", "flac", "ogg", "opus", "mp4", "mkv", "mov", "webm",
                    "avi", "m4v", "mpeg", "ts",
                ],
            )
            .add_filter("All files", &["*"])
            .blocking_pick_file()
            .map(|file| file.into_path())
            .transpose()
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn generate_captions(
    media: PathBuf,
    language: String,
    audio_track: u32,
    state: State<'_, Desktop>,
) -> Result<(), String> {
    let language = selected_language(Some(&language)).map_err(error_message)?;
    let models = Arc::clone(&state.models);
    state
        .launch_job(
            Status {
                phase: Phase::Extracting,
                captions: true,
                ..Status::default()
            },
            move |job| {
                let transcription = models.transcription_options(language);
                let subtitle = captions::generate(
                    &media,
                    None,
                    &captions::MediaOptions::new(audio_track),
                    &transcription,
                    |stage| {
                        job.lock().unwrap().status.phase = match stage {
                            captions::Stage::Extracting => Phase::Extracting,
                            captions::Stage::Transcribing => Phase::Transcribing,
                        };
                    },
                )?;
                job.lock().unwrap().status.subtitle_path = Some(subtitle);
                Ok(())
            },
        )
        .map_err(error_message)
}

#[tauri::command]
async fn list_audio_inputs(state: State<'_, Desktop>) -> Result<Vec<AudioInput>, String> {
    let devices = tauri::async_runtime::spawn_blocking(|| {
        Ok::<_, anyhow::Error>(
            audio::available_inputs()?
                .enumerate()
                .map(|(index, device)| {
                    let name = device
                        .name()
                        .unwrap_or_else(|_| format!("Audio input {}", index + 1));
                    (name, device)
                })
                .collect::<Vec<_>>(),
        )
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(error_message)?;

    let mut inputs = state.audio_inputs.lock().unwrap();
    inputs.revision += 1;
    let revision = inputs.revision;
    inputs.devices.clear();
    Ok(devices
        .into_iter()
        .enumerate()
        .map(|(index, (name, device))| {
            // IDs belong to one scan. A later scan can never redirect an old ID.
            let id = format!("{revision}:{index}");
            inputs.devices.insert(id.clone(), device);
            AudioInput { id, name }
        })
        .collect())
}

#[tauri::command]
fn start_recording(
    language: String,
    input_id: Option<String>,
    state: State<'_, Desktop>,
) -> Result<(), String> {
    let language = selected_language(Some(&language)).map_err(error_message)?;
    let models = Arc::clone(&state.models);
    let sessions = state.sessions();
    let level = Arc::clone(&state.level);
    let input = input_id.map(|id| {
        state
            .audio_inputs
            .lock()
            .unwrap()
            .devices
            .get(&id)
            .cloned()
            .context("The selected audio input is no longer available")
    });
    state
        .launch(Phase::Starting, move |job| {
            let explicit_input = input.is_some();
            let device = match input {
                Some(device) => device.inspect_err(|_| {
                    job.lock().unwrap().status.refresh_audio_inputs = true;
                })?,
                None => audio::input_device(None)?,
            };
            let session = new_session(&sessions)?;
            let path = session.join("audio.wav");
            let recorded = audio::record_until(&device, &path, Some(level), |stop| {
                let mut job = job.lock().unwrap();
                job.stop = Some(stop);
                job.status.phase = Phase::Recording;
            });
            if wave_file(&session).is_ok() {
                job.lock().unwrap().status.session = session
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_owned);
            }
            recorded.inspect_err(|error| {
                if explicit_input
                    && (error.is::<cpal::DefaultStreamConfigError>()
                        || error.is::<cpal::BuildStreamError>()
                        || error.is::<cpal::PlayStreamError>()
                        || error.is::<cpal::StreamError>())
                {
                    job.lock().unwrap().status.refresh_audio_inputs = true;
                }
            })?;
            let mut options = models.processing_options(language);
            options.set_control(Arc::clone(&job.lock().unwrap().control));
            process(job, &session, &options)
        })
        .map_err(error_message)
}

#[tauri::command]
fn stop_recording(state: State<'_, Desktop>) -> Result<(), String> {
    let mut job = state.job.lock().unwrap();
    if !matches!(job.status.phase, Phase::Recording) {
        return Err("Recording has already stopped".into());
    }
    job.status.phase = Phase::Saving;
    if let Some(stop) = job.stop.take() {
        // The worker reports any capture error that is already queued.
        let _ = stop.try_send(Ok(()));
    }
    Ok(())
}

#[tauri::command]
fn retry(id: String, language: String, state: State<'_, Desktop>) -> Result<(), String> {
    start_session_processing(id, language, false, &state).map_err(error_message)
}

#[tauri::command]
fn restart(id: String, language: String, state: State<'_, Desktop>) -> Result<(), String> {
    start_session_processing(id, language, true, &state).map_err(error_message)
}

fn start_session_processing(
    id: String,
    language: String,
    restart: bool,
    state: &Desktop,
) -> Result<()> {
    let session = state.session(&id)?;
    wave_file(&session)?;
    let selected = selected_language(Some(&language))?;
    {
        let mut job = state.job.lock().unwrap();
        if job.status.phase.busy() {
            ensure!(
                restart
                    && !job.status.captions
                    && job.status.session.as_deref() == Some(&id)
                    && matches!(
                        job.status.phase,
                        Phase::Preparing | Phase::Transcribing | Phase::Synthesizing
                    ),
                "Wait for the active job to finish"
            );
            ensure!(!job.status.restarting, "Restart already requested");
            job.restart_language = selected;
            job.status.restarting = true;
            job.control.cancel()?;
            return Ok(());
        }
    }
    let language = if !restart && session.join("whisper.json").is_file() {
        None
    } else {
        selected
    };
    let models = Arc::clone(&state.models);
    state.launch_job(
        Status {
            phase: Phase::Preparing,
            session: Some(id),
            ..Status::default()
        },
        move |job| {
            let mut options = models.processing_options(language);
            options.set_control(Arc::clone(&job.lock().unwrap().control));
            process_mode(job, &session, &options, restart)
        },
    )
}

fn project_root() -> Result<PathBuf> {
    let executable = env::current_exe()?;
    let directory = executable
        .parent()
        .context("Application folder is unavailable")?;
    Ok(directory
        .ancestors()
        .find(|path| path.join("tauri.conf.json").is_file() && path.join("Cargo.toml").is_file())
        .unwrap_or(directory)
        .to_path_buf())
}

fn main() {
    tauri::Builder::default()
        // The single-instance plugin must be registered first.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let root = project_root()?;
            let models = Arc::new(models::ModelStore::new(
                root.clone(),
                app.path().app_data_dir()?.join("resources"),
                app.path().app_config_dir()?.join("models.json"),
            )?);
            app.manage(Desktop {
                root,
                models,
                job: Arc::default(),
                level: Arc::default(),
                audio_inputs: Mutex::default(),
            });
            settings::setup(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<Desktop>();
                if state.job.lock().unwrap().status.phase.busy() {
                    api.prevent_close();
                    let _ = window.emit("session-active", ());
                } else if state.models.downloading() {
                    api.prevent_close();
                    let _ = window.emit("model-download-active", ());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            settings::get_settings,
            settings::save_settings,
            models::get_models,
            models::download_model,
            models::cancel_model_download,
            models::use_model_path,
            models::delete_model,
            models::pick_model_file,
            history,
            set_favorite,
            delete_session,
            open_session_source,
            review,
            status,
            list_audio_inputs,
            start_recording,
            stop_recording,
            pick_caption_media,
            generate_captions,
            retry,
            restart
        ])
        .run(tauri::generate_context!())
        .expect("Could not start Lovelace");
}

#[cfg(test)]
#[path = "desktop/tests.rs"]
mod tests;
