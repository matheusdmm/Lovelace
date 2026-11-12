#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::{
    env, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    thread,
};

use anyhow::{Context, Result, bail, ensure};
use lovelace::{Language, ProcessingOptions, audio, new_session, pipeline};
use pulldown_cmark::{Event, Parser, Tag, TagEnd, html};
use serde::Serialize;
use tauri::{Emitter, Manager, State, WindowEvent};

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    #[default]
    Idle,
    Starting,
    Recording,
    Saving,
    Preparing,
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
}

#[derive(Default)]
struct Job {
    status: Status,
    stop: Option<mpsc::SyncSender<Result<()>>>,
}

struct Desktop {
    root: PathBuf,
    job: Arc<Mutex<Job>>,
    level: Arc<AtomicU32>,
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
            fs::symlink_metadata(&path)?.is_dir() && path.canonicalize()?.parent() == Some(&root),
            "Invalid session folder"
        );
        Ok(path)
    }

    fn launch(
        &self,
        phase: Phase,
        task: impl FnOnce(&Mutex<Job>) -> Result<()> + Send + 'static,
    ) -> Result<()> {
        let mut job = self.job.lock().unwrap();
        ensure!(!job.status.phase.busy(), "A session is already in progress");
        let previous = job.status.clone();
        job.status = Status {
            phase,
            ..Status::default()
        };
        self.level.store(0, Ordering::Relaxed);

        let shared = Arc::clone(&self.job);
        if let Err(error) = thread::Builder::new().name("sessao".into()).spawn(move || {
            let result = task(&shared);
            let mut job = shared.lock().unwrap();
            job.stop = None;
            match result {
                Ok(()) => job.status.phase = Phase::Done,
                Err(error) => {
                    job.status.phase = Phase::Failed;
                    job.status.error = Some(format!("{error:#}"));
                }
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
}

#[derive(Serialize)]
struct Document {
    markdown: String,
    html: String,
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
                    has_summary: entry.path().join("sintese.md").is_file(),
                    has_transcript: entry.path().join("transcricao.md").is_file(),
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
    }))
}

#[tauri::command]
async fn history(state: State<'_, Desktop>) -> Result<Vec<Session>, String> {
    list_sessions(&state).map_err(error_message)
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
    pipeline::process_with_progress(session, options, |stage| {
        job.lock().unwrap().status.phase = match stage {
            pipeline::Stage::Transcribing => Phase::Transcribing,
            pipeline::Stage::Synthesizing => Phase::Synthesizing,
        };
    })
    .context("Audio saved. You can try processing the session again")
}

#[tauri::command]
fn start_recording(language: String, state: State<'_, Desktop>) -> Result<(), String> {
    let language = selected_language(Some(&language)).map_err(error_message)?;
    let options = ProcessingOptions::desktop(&state.root, language);
    let sessions = state.sessions();
    let level = Arc::clone(&state.level);
    state
        .launch(Phase::Starting, move |job| {
            let device = audio::input_device(None)?;
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
            recorded?;
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
    let session = state.session(&id).map_err(error_message)?;
    wave_file(&session).map_err(error_message)?;
    let language = if session.join("whisper.json").is_file() {
        None
    } else {
        selected_language(Some(&language)).map_err(error_message)?
    };
    let options = ProcessingOptions::desktop(&state.root, language);
    state
        .launch(Phase::Preparing, move |job| {
            job.lock().unwrap().status.session = Some(id);
            process(job, &session, &options)
        })
        .map_err(error_message)
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
        .setup(|app| {
            app.manage(Desktop {
                root: project_root()?,
                job: Arc::default(),
                level: Arc::default(),
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<Desktop>();
                if state.job.lock().unwrap().status.phase.busy() {
                    api.prevent_close();
                    let _ = window.emit("session-active", ());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            history,
            review,
            status,
            start_recording,
            stop_recording,
            retry
        ])
        .run(tauri::generate_context!())
        .expect("Could not start Lovelace");
}

#[cfg(test)]
#[path = "desktop/tests.rs"]
mod tests;
