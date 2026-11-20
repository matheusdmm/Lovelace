use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write as _},
    path::Path,
    process::Stdio,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{Language, ProcessingOptions, TranscriptionOptions, synthesis};

#[derive(Deserialize)]
pub(crate) struct WhisperOutput {
    pub(crate) transcription: Vec<Segment>,
    result: Option<WhisperResult>,
}

#[derive(Deserialize)]
struct WhisperResult {
    language: String,
}

#[derive(Deserialize)]
pub struct Segment {
    offsets: Offsets,
    pub text: String,
}

#[derive(Deserialize)]
struct Offsets {
    from: u64,
    to: u64,
}

impl Segment {
    pub fn start(&self) -> String {
        timestamp(self.offsets.from, '.')
    }

    pub(crate) fn range_ms(&self) -> (u64, u64) {
        (self.offsets.from, self.offsets.to)
    }
}

pub fn process(session: &Path, options: &ProcessingOptions) -> Result<()> {
    process_with_progress(session, options, |_| {})
}

#[derive(Clone, Copy)]
pub enum Stage {
    Transcribing,
    Synthesizing,
}

pub fn process_with_progress(
    session: &Path,
    options: &ProcessingOptions,
    progress: impl Fn(Stage),
) -> Result<()> {
    process_mode_with_progress(session, options, false, progress)
}

pub fn process_mode_with_progress(
    session: &Path,
    options: &ProcessingOptions,
    restart: bool,
    progress: impl Fn(Stage),
) -> Result<()> {
    let wav = session.join("audio.wav");
    ensure!(wav.is_file(), "Audio file missing at {}", wav.display());
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(session.join(".processar.lock"))?;
    lock.try_lock()
        .context("Could not lock the session for processing")?;

    if let Some(control) = &options.transcription.control {
        control.check()?;
    }
    if restart {
        archive_outputs(session)?;
    }

    let json = session.join("whisper.json");
    let output = match fs::read(&json) {
        Ok(bytes) => parse_whisper(&bytes)?,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            progress(Stage::Transcribing);
            let bytes = transcribe(session, &wav, &options.transcription, &[])?;
            let output = parse_whisper(&bytes)?;
            save(&json, &bytes)?;
            output
        }
        Err(error) => return Err(error).context("Could not read whisper.json"),
    };
    let segments = &output.transcription;

    let transcript = session.join("transcricao.md");
    let segment_anchor = if transcript.try_exists()? {
        let markdown = fs::read_to_string(&transcript).context("Could not read the transcript")?;
        if markdown.lines().any(|line| line.starts_with("### Trecho ")) {
            "trecho"
        } else {
            "segment"
        }
    } else {
        save(&transcript, render_transcript(segments).as_bytes())?;
        "segment"
    };
    println!("Transcript saved to {}", transcript.display());
    if segments.is_empty() {
        println!("Whisper found no speech segments.");
    }

    let detected = output
        .result
        .as_ref()
        .map(|result| result.language.as_str());
    let language = match (options.transcription.idioma, detected) {
        (Some(Language::Pt), _) => Language::Pt,
        (Some(Language::En), _) => Language::En,
        (_, Some("pt")) => Language::Pt,
        (_, Some("en")) => Language::En,
        _ => Language::Auto,
    };
    progress(Stage::Synthesizing);
    if let Some(control) = &options.transcription.control {
        control.check()?;
    }
    let summary = synthesis::generate(session, segments, segment_anchor, language, options)?;
    if let Some(control) = &options.transcription.control {
        control.check()?;
    }
    let summary_path = session.join("sintese.md");
    save(&summary_path, summary.as_bytes())?;
    println!("Summary saved to {}", summary_path.display());
    Ok(())
}

pub(crate) fn transcribe(
    directory: &Path,
    wav: &Path,
    options: &TranscriptionOptions,
    extra_args: &[&str],
) -> Result<Vec<u8>> {
    ensure!(
        options.whisper.is_file(),
        "whisper-cli missing at {}",
        options.whisper.display()
    );
    ensure!(
        options.modelo_whisper.is_file(),
        "Whisper model missing at {}",
        options.modelo_whisper.display()
    );

    let scratch = tempfile::tempdir_in(directory)?;
    let output = scratch.path().join("whisper");
    let mut command = options.engine_command(options.whisper.canonicalize()?);
    command
        .arg("--model")
        .arg(&options.modelo_whisper)
        .arg("--file")
        .arg(wav)
        .arg("--language")
        .arg(options.idioma.unwrap_or(Language::Pt).whisper_code())
        .arg("--output-json")
        .arg("--output-file")
        .arg(&output)
        .args(extra_args)
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    if options.cpu {
        command.arg("--no-gpu");
    }

    println!("Transcribing with Whisper...");
    let logs = OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("whisper.log"))?;
    command.arg("--print-progress").stderr(logs);
    let child = command.spawn().context("Could not start whisper-cli")?;
    let engine = crate::job::Engine::new(child, options.control.clone())?;
    let status = engine.wait()?;
    ensure!(status.success(), "whisper-cli exited with {status}");
    fs::read(output.with_extension("json")).context("whisper-cli did not produce the expected JSON")
}

fn archive_outputs(session: &Path) -> Result<()> {
    let backup = tempfile::Builder::new()
        .prefix("restart-backup-")
        .tempdir_in(session)?
        .keep();
    let mut copied = Vec::new();
    for name in ["whisper.json", "transcricao.md", "sintese.md"] {
        let source = session.join(name);
        if source.try_exists()? {
            fs::copy(&source, backup.join(name)).context("Could not back up previous results")?;
            copied.push(source);
        }
    }
    // Only invalidate results after every backup has succeeded.
    for source in copied {
        fs::remove_file(source)?;
    }
    Ok(())
}

pub(crate) fn parse_whisper(bytes: &[u8]) -> Result<WhisperOutput> {
    let mut output: WhisperOutput =
        serde_json::from_slice(bytes).context("Invalid Whisper JSON")?;
    output
        .transcription
        .retain(|segment| !segment.text.trim().is_empty());
    for segment in &output.transcription {
        ensure!(
            segment.offsets.from <= segment.offsets.to,
            "Invalid timestamp range in a Whisper segment"
        );
    }
    Ok(output)
}

fn render_transcript(segments: &[Segment]) -> String {
    crate::transcript::from_segments(segments).markdown
}

pub fn markdown_text(text: &str) -> String {
    let text = text.trim();
    let mut escaped = String::with_capacity(text.len());
    for (index, character) in text.char_indices() {
        let ordered_list = matches!(character, '.' | ')')
            && (1..=9).contains(&index)
            && text[..index].bytes().all(|byte| byte.is_ascii_digit());
        if matches!(
            character,
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '&' | '#' | '~' | '|'
        ) || (index == 0 && matches!(character, '-' | '+' | '='))
            || ordered_list
        {
            escaped.push('\\');
        }
        escaped.push(match character {
            '\r' | '\n' => ' ',
            _ => character,
        });
    }
    escaped
}

pub(crate) fn timestamp(ms: u64, separator: char) -> String {
    let seconds = ms / 1000;
    format!(
        "{:02}:{:02}:{:02}{separator}{:03}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        ms % 1000
    )
}

fn save(path: &Path, bytes: &[u8]) -> Result<()> {
    let directory = path.parent().context("Output path has no parent folder")?;
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    file.write_all(bytes)?;
    file.persist(path)
        .with_context(|| format!("Could not save {}", path.display()))?;
    Ok(())
}
