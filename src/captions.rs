use std::{
    fmt::Write as _,
    io::Write as _,
    path::{self, Path, PathBuf},
    process::Stdio,
};

use anyhow::{Context, Result, ensure};
use clap::Args;

use crate::{TranscriptionOptions, pipeline};

#[derive(Args)]
pub struct MediaOptions {
    /// FFmpeg executable.
    #[arg(long, default_value = "ffmpeg")]
    ffmpeg: PathBuf,
    /// Audio track in the file, starting at 1.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    audio_track: u32,
}

impl MediaOptions {
    pub fn new(audio_track: u32) -> Self {
        Self {
            ffmpeg: "ffmpeg".into(),
            audio_track,
        }
    }
}

pub enum Stage {
    Extracting,
    Transcribing,
}

pub fn generate(
    media: &Path,
    destination: Option<&Path>,
    media_options: &MediaOptions,
    options: &TranscriptionOptions,
    progress: impl Fn(Stage),
) -> Result<PathBuf> {
    let media = path::absolute(media)?;
    ensure!(media.is_file(), "Media file missing at {}", media.display());
    let destination = match destination {
        Some(path) => path::absolute(path)?,
        None => media.with_extension("srt"),
    };
    let directory = destination
        .parent()
        .context("Invalid subtitle destination")?;
    ensure!(
        destination
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("srt")),
        "The subtitle destination must have an .srt extension"
    );
    ensure!(
        !destination.try_exists()?,
        "Subtitle destination already exists at {}",
        destination.display()
    );
    let mut file = tempfile::NamedTempFile::new_in(directory).with_context(|| {
        format!(
            "Could not prepare subtitle output in {}",
            directory.display()
        )
    })?;

    let track = media_options
        .audio_track
        .checked_sub(1)
        .context("Audio tracks start at 1")?;
    let scratch = tempfile::tempdir()?;
    let wav = scratch.path().join("audio.wav");
    progress(Stage::Extracting);
    let extraction = options
        .engine_command(media_options.ffmpeg.clone())
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-protocol_whitelist",
            "file",
            "-i",
        ])
        .arg(&media)
        .arg("-map")
        .arg(format!("0:a:{track}"))
        .args([
            "-af",
            "aresample=16000:async=1:first_pts=0",
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(&wav)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .output()
        .context("Could not start FFmpeg. Install it on PATH or pass --ffmpeg in the CLI")?;
    ensure!(
        extraction.status.success(),
        "FFmpeg could not extract audio track {}: {}",
        media_options.audio_track,
        String::from_utf8_lossy(&extraction.stderr).trim(),
    );
    ensure!(
        hound::WavReader::open(&wav)?.duration() > 0,
        "The audio track is empty"
    );

    progress(Stage::Transcribing);
    let json = pipeline::transcribe(
        scratch.path(),
        &wav,
        options,
        &["--max-len", "72", "--split-on-word"],
    )?;
    let segments = pipeline::parse_whisper(&json)?.transcription;
    let subtitles = render_srt(&segments);
    ensure!(!subtitles.is_empty(), "Whisper found no speech to subtitle");
    file.write_all(subtitles.as_bytes())?;
    file.persist_noclobber(&destination)
        .with_context(|| format!("Could not save subtitles to {}", destination.display()))?;
    Ok(destination)
}

fn render_srt(segments: &[pipeline::Segment]) -> String {
    let mut subtitles = String::new();
    let mut cue = 0;
    for segment in segments {
        let (from, to) = segment.range_ms();
        if from == to {
            continue;
        }
        cue += 1;
        let start = pipeline::timestamp(from, ',');
        let end = pipeline::timestamp(to, ',');
        let text = segment
            .text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        writeln!(subtitles, "{cue}\n{start} --> {end}\n{text}\n").unwrap();
    }
    subtitles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srt_keeps_milliseconds_and_speech_in_valid_cues() -> Result<()> {
        let output = pipeline::parse_whisper(
            r#"{"transcription":[
                {"offsets":{"from":0,"to":0},"text":"zero duration"},
                {"offsets":{"from":0,"to":100},"text":" "},
                {"offsets":{"from":1234,"to":4567},"text":" Olá!\n\n Tudo bem? "},
                {"offsets":{"from":3600000,"to":3600040},"text":"Yes."}
            ]}"#
            .as_bytes(),
        )?;
        assert_eq!(
            render_srt(&output.transcription),
            "1\n00:00:01,234 --> 00:00:04,567\nOlá! Tudo bem?\n\n2\n01:00:00,000 --> 01:00:00,040\nYes.\n\n"
        );
        Ok(())
    }
}
