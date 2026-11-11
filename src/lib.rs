pub mod audio;
pub mod pipeline;
mod synthesis;

use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use clap::{Args, ValueEnum};

const WHISPER: &str = "engines/whisper/Release/whisper-cli.exe";
const MODEL_WHISPER: &str = "models/ggml-small-q5_1.bin";
const LLAMA: &str = "engines/llama/llama-server.exe";
const MODEL_LLAMA: &str = "models/qwen2.5-3b-instruct-q4_k_m.gguf";

#[derive(Args)]
pub struct ProcessingOptions {
    /// Idioma da fala. O padrão para gravações novas é pt.
    #[arg(long, value_enum)]
    pub idioma: Option<Language>,
    /// Usa CPU nas duas etapas.
    #[arg(long)]
    cpu: bool,
    #[arg(long, default_value = WHISPER)]
    whisper: PathBuf,
    #[arg(long, default_value = MODEL_WHISPER)]
    modelo_whisper: PathBuf,
    #[arg(long, default_value = LLAMA)]
    llama: PathBuf,
    #[arg(long, default_value = MODEL_LLAMA)]
    modelo_llama: PathBuf,
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
    pub fn desktop(root: &Path, idioma: Option<Language>) -> Self {
        Self {
            idioma,
            cpu: false,
            whisper: root.join(WHISPER),
            modelo_whisper: root.join(MODEL_WHISPER),
            llama: root.join(LLAMA),
            modelo_llama: root.join(MODEL_LLAMA),
            hide_engine_windows: true,
        }
    }

    fn engine_command(&self, executable: PathBuf) -> Command {
        let mut command = Command::new(executable);
        #[cfg(windows)]
        if self.hide_engine_windows {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
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
