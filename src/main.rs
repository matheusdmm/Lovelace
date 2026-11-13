use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use lovelace::{ProcessingOptions, TranscriptionOptions, audio, captions, new_session, pipeline};

#[derive(Parser)]
#[command(
    version,
    about = "Record ideas and generate a local summary with links to the transcript."
)]
struct Cli {
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Record from a microphone. Press Enter to stop.
    #[command(alias = "gravar")]
    Record {
        /// Input index from `microphones`. Defaults to the system input.
        #[arg(long, alias = "microfone")]
        microphone: Option<usize>,
        /// Save the WAV file for processing later.
        #[arg(long, alias = "so-audio")]
        audio_only: bool,
        #[command(flatten)]
        options: ProcessingOptions,
    },
    /// List available audio inputs.
    #[command(alias = "microfones")]
    Microphones,
    /// Process a recorded session using its existing transcript when available.
    #[command(alias = "processar")]
    Process {
        session: PathBuf,
        #[command(flatten)]
        options: ProcessingOptions,
    },
    /// Generate an SRT subtitle file from local media.
    Captions {
        media: PathBuf,
        /// SRT destination. Defaults to the media file with a .srt extension.
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[command(flatten)]
        media_options: captions::MediaOptions,
        #[command(flatten)]
        transcription: TranscriptionOptions,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Action::Record {
            microphone,
            audio_only,
            options,
        } => {
            let device = audio::input_device(microphone)?;
            let session = new_session(Path::new("sessoes"))?;
            let wav = session.join("audio.wav");
            audio::record(&device, &wav)?;
            println!("Recording saved to {}", wav.display());
            if !audio_only {
                pipeline::process(&session, &options).with_context(|| {
                    format!(
                        "Audio saved. To retry, run lovelace process \"{}\"",
                        session.display()
                    )
                })?;
            }
            Ok(())
        }
        Action::Microphones => audio::list_inputs(),
        Action::Process { session, options } => pipeline::process(&session, &options),
        Action::Captions {
            media,
            output,
            media_options,
            transcription,
        } => {
            let path = captions::generate(
                &media,
                output.as_deref(),
                &media_options,
                &transcription,
                |_| {},
            )?;
            println!("Captions saved to {}", path.display());
            Ok(())
        }
    }
}
