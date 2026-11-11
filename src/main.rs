use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use lovelace::{ProcessingOptions, audio, new_session, pipeline};

#[derive(Parser)]
#[command(
    version,
    about = "Grave ideias e gere uma síntese local com referências à fala."
)]
struct Cli {
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Grava pelo microfone. Pressione Enter para encerrar.
    Gravar {
        /// Índice mostrado por `microfones`. Por padrão, usa a entrada do sistema.
        #[arg(long)]
        microfone: Option<usize>,
        /// Grava o WAV para processar depois.
        #[arg(long)]
        so_audio: bool,
        #[command(flatten)]
        options: ProcessingOptions,
    },
    /// Lista as entradas de áudio disponíveis.
    Microfones,
    /// Processa uma sessão já gravada e reaproveita a transcrição, se houver.
    Processar {
        pasta: PathBuf,
        #[command(flatten)]
        options: ProcessingOptions,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Action::Gravar {
            microfone,
            so_audio,
            options,
        } => {
            let device = audio::input_device(microfone)?;
            let session = new_session(Path::new("sessoes"))?;
            let wav = session.join("audio.wav");
            audio::record(&device, &wav)?;
            println!("Gravação salva em {}", wav.display());
            if !so_audio {
                pipeline::process(&session, &options).with_context(|| {
                    format!(
                        "Áudio preservado. Para tentar de novo, use processar \"{}\"",
                        session.display()
                    )
                })?;
            }
            Ok(())
        }
        Action::Microfones => audio::list_inputs(),
        Action::Processar { pasta, options } => pipeline::process(&pasta, &options),
    }
}
