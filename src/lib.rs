pub mod audio;
pub mod captions;
pub mod job;
pub mod pipeline;
mod synthesis;
pub mod transcript;

use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use clap::{Args, ValueEnum};

const WHISPER: &str = if cfg!(windows) {
    "engines/whisper/Release/whisper-cli.exe"
} else {
    "engines/whisper/Release/whisper-cli"
};
const MODEL_WHISPER: &str = "models/ggml-small-q5_1.bin";
const LLAMA: &str = if cfg!(windows) {
    "engines/llama/llama-server.exe"
} else {
    "engines/llama/llama-server"
};
const MODEL_LLAMA: &str = "models/qwen2.5-3b-instruct-q4_k_m.gguf";

#[derive(Args)]
pub struct ProcessingOptions {
    #[command(flatten)]
    pub transcription: TranscriptionOptions,
    #[arg(long, default_value = LLAMA)]
    llama: PathBuf,
    #[arg(long = "llama-model", alias = "modelo-llama", default_value = MODEL_LLAMA)]
    modelo_llama: PathBuf,
}

#[derive(Args)]
pub struct TranscriptionOptions {
    #[arg(skip)]
    pub(crate) control: Option<std::sync::Arc<job::Control>>,
    /// Spoken language. Defaults to pt for new recordings.
    #[arg(long = "language", alias = "idioma", value_enum)]
    pub idioma: Option<Language>,
    /// Use the CPU for model inference.
    #[arg(long)]
    cpu: bool,
    #[arg(long, default_value = WHISPER)]
    whisper: PathBuf,
    #[arg(long = "whisper-model", alias = "modelo-whisper", default_value = MODEL_WHISPER)]
    modelo_whisper: PathBuf,
    #[cfg(windows)]
    #[arg(skip)]
    hide_engine_windows: bool,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Language {
    Pt,
    En,
    Auto,
}

impl Language {
    fn whisper_code(self) -> &'static str {
        match self {
            Self::Pt => "pt",
            Self::En => "en",
            Self::Auto => "auto",
        }
    }
}

impl ProcessingOptions {
    pub fn set_summary_paths(&mut self, engine: PathBuf, model: PathBuf) {
        self.llama = engine;
        self.modelo_llama = model;
    }

    pub fn set_control(&mut self, control: std::sync::Arc<job::Control>) {
        self.transcription.control = Some(control);
    }
    pub fn desktop(root: &Path, idioma: Option<Language>) -> Self {
        Self {
            transcription: TranscriptionOptions::desktop(root, idioma),
            llama: root.join(LLAMA),
            modelo_llama: root.join(MODEL_LLAMA),
        }
    }
}

impl TranscriptionOptions {
    pub fn set_paths(&mut self, engine: PathBuf, model: PathBuf) {
        self.whisper = engine;
        self.modelo_whisper = model;
    }

    pub fn desktop(root: &Path, idioma: Option<Language>) -> Self {
        Self {
            control: None,
            idioma,
            cpu: false,
            whisper: root.join(WHISPER),
            modelo_whisper: root.join(MODEL_WHISPER),
            #[cfg(windows)]
            hide_engine_windows: true,
        }
    }

    fn engine_command(&self, executable: PathBuf) -> Command {
        let command = Command::new(executable);
        #[cfg(windows)]
        let command = {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            let mut command = command;
            if self.hide_engine_windows {
                command.creation_flags(CREATE_NO_WINDOW);
            }
            command
        };
        command
    }
}

pub fn new_session(root: &Path) -> Result<PathBuf> {
    fs::create_dir_all(root)?;
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
    for suffix in 0.. {
        let session = root.join(format!("sessao-{timestamp}-{suffix}"));
        match fs::create_dir(&session) {
            Ok(()) => return Ok(session),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_created_in_the_same_root_get_distinct_directories() -> Result<()> {
        let workspace = tempfile::tempdir()?;
        let root = workspace.path().join("sessoes");

        let first = new_session(&root)?;
        let second = new_session(&root)?;

        assert_ne!(first, second);
        for session in [first, second] {
            assert!(session.is_dir());
            assert_eq!(session.parent(), Some(root.as_path()));
            assert!(
                session
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("sessao-")
            );
        }
        Ok(())
    }
}
