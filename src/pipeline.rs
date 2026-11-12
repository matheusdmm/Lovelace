use std::{
    fmt::Write as _,
    fs::{self, OpenOptions},
    io::{ErrorKind, Write as _},
    path::Path,
    process::Stdio,
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{Language, ProcessingOptions, synthesis};

#[derive(Deserialize)]
struct WhisperOutput {
    transcription: Vec<Segment>,
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
        timestamp(self.offsets.from)
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

    let json = session.join("whisper.json");
    let output = match fs::read(&json) {
        Ok(bytes) => parse_whisper(&bytes)?,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            progress(Stage::Transcribing);
            let bytes = transcribe(session, &wav, options)?;
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
        if markdown
            .lines()
            .any(|line| line.starts_with("### Segment "))
        {
            "segment"
        } else {
            "trecho"
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
    let language = match (options.idioma, detected) {
        (Some(Language::Pt), _) => Language::Pt,
        (Some(Language::En), _) => Language::En,
        (_, Some("pt")) => Language::Pt,
        (_, Some("en")) => Language::En,
        _ => Language::Auto,
    };
    progress(Stage::Synthesizing);
    let summary = synthesis::generate(session, segments, segment_anchor, language, options)?;
    let summary_path = session.join("sintese.md");
    save(&summary_path, summary.as_bytes())?;
    println!("Summary saved to {}", summary_path.display());
    Ok(())
}

fn transcribe(session: &Path, wav: &Path, options: &ProcessingOptions) -> Result<Vec<u8>> {
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

    let scratch = tempfile::tempdir_in(session)?;
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
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    if options.cpu {
        command.arg("--no-gpu");
    }

    println!("Transcribing with Whisper...");
    let status = command.status().context("Could not start whisper-cli")?;
    ensure!(status.success(), "whisper-cli exited with {status}");
    fs::read(output.with_extension("json")).context("whisper-cli did not produce the expected JSON")
}

fn parse_whisper(bytes: &[u8]) -> Result<WhisperOutput> {
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
    let mut text = String::from("# Transcript\n\n");
    if segments.is_empty() {
        text.push_str("No speech segments found.\n");
    }
    for (index, segment) in segments.iter().enumerate() {
        let id = index + 1;
        let end = timestamp(segment.offsets.to);
        let start = segment.start();
        let speech = markdown_text(&segment.text);
        writeln!(
            text,
            "### Segment {id:04}\n\n**{start} – {end}**\n\n{speech}\n"
        )
        .unwrap();
    }
    text
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

fn timestamp(ms: u64) -> String {
    let seconds = ms / 1000;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
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
