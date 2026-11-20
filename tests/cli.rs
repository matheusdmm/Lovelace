use std::{fs, path::Path, process::Command};

use anyhow::Result;
use serde_json::json;
use tempfile::TempDir;

fn workspace() -> Result<TempDir> {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"));
    fs::create_dir_all(root)?;
    Ok(tempfile::tempdir_in(root)?)
}

fn cli(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_lovelace"));
    command.current_dir(root);
    command
}

fn write_audio(session: &Path) -> Result<()> {
    let mut wav = hound::WavWriter::create(
        session.join("audio.wav"),
        hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    wav.write_sample(0_i16)?;
    wav.finalize()?;
    Ok(())
}

#[test]
fn cli_help_lists_the_available_commands() -> Result<()> {
    let root = workspace()?;
    let output = cli(root.path()).arg("--help").output()?;
    let help = String::from_utf8(output.stdout)?;
    assert!(output.status.success());
    for command in ["record", "microphones", "process", "captions"] {
        assert!(
            help.lines()
                .any(|line| line.split_whitespace().next() == Some(command)),
            "{help}"
        );
    }
    Ok(())
}

#[test]
fn captions_refuses_to_replace_an_existing_subtitle() -> Result<()> {
    let root = workspace()?;
    fs::write(root.path().join("recording.mkv"), "")?;
    let subtitle = root.path().join("recording.srt");
    fs::write(&subtitle, "Edited subtitle")?;

    let output = cli(root.path())
        .args(["captions", "recording.mkv"])
        .output()?;

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Subtitle destination already exists")
    );
    assert_eq!(fs::read_to_string(subtitle)?, "Edited subtitle");
    Ok(())
}

#[test]
fn cli_rejects_an_unsupported_language_before_recording() -> Result<()> {
    let root = workspace()?;
    let output = cli(root.path())
        .args(["gravar", "--idioma", "es"])
        .output()?;
    let error = String::from_utf8(output.stderr)?;
    assert_eq!(output.status.code(), Some(2), "{error}");
    assert!(error.contains("--language"), "{error}");
    assert!(!root.path().join("sessoes").exists());
    Ok(())
}

#[test]
fn processing_requires_audio() -> Result<()> {
    let session = workspace()?;
    let output = cli(session.path()).args(["process", "."]).output()?;
    let error = String::from_utf8(output.stderr)?;
    assert!(!output.status.success());
    assert!(error.contains("Audio file missing"), "{error}");
    assert!(error.contains("audio.wav"), "{error}");
    assert_eq!(fs::read_dir(session.path())?.count(), 0);
    Ok(())
}

#[test]
fn processing_preserves_the_recording_when_whisper_is_missing() -> Result<()> {
    let session = workspace()?;
    let path = session.path();
    write_audio(path)?;
    let audio = fs::read(path.join("audio.wav"))?;

    let output = cli(path)
        .args(["process", ".", "--whisper", "missing-whisper"])
        .output()?;
    let error = String::from_utf8(output.stderr)?;
    assert!(!output.status.success());
    assert!(error.contains("whisper-cli missing"), "{error}");
    assert_eq!(fs::read(path.join("audio.wav"))?, audio);
    for filename in ["whisper.json", "transcricao.md", "sintese.md"] {
        assert!(!path.join(filename).exists(), "{filename}");
    }
    Ok(())
}

#[test]
fn processing_silence_creates_both_documents_and_preserves_audio() -> Result<()> {
    let session = workspace()?;
    let path = session.path();
    write_audio(path)?;
    let audio = fs::read(path.join("audio.wav"))?;
    let whisper = json!({
        "transcription": [
            {"offsets": {"from": 0, "to": 100}, "text": " \n "}
        ]
    });
    fs::write(path.join("whisper.json"), whisper.to_string())?;

    let output = cli(path).args(["processar", "."]).output()?;

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(path.join("transcricao.md"))?,
        "# Transcript\n\nNo speech segments found.\n"
    );
    assert_eq!(
        fs::read_to_string(path.join("sintese.md"))?,
        "# Recording summary\n\nNo speech segments found.\n"
    );
    assert_eq!(fs::read(path.join("audio.wav"))?, audio);
    assert_eq!(
        fs::read_to_string(path.join("whisper.json"))?,
        whisper.to_string()
    );
    Ok(())
}

#[test]
fn retry_reuses_whisper_output_and_preserves_an_edited_transcript() -> Result<()> {
    let session = workspace()?;
    let path = session.path();
    write_audio(path)?;
    let whisper = json!({
        "result": {"language": "pt"},
        "transcription": [
            {"offsets": {"from": 0, "to": 10}, "text": "  "},
            {"offsets": {"from": 62_345, "to": 63_456}, "text": " Ideia *principal*\ncontinua. "}
        ]
    })
    .to_string();
    fs::write(path.join("whisper.json"), &whisper)?;
    let transcript = path.join("transcricao.md");
    let mut process = cli(path);
    process.args(["process", ".", "--llama", "missing-llama"]);

    let first = process.output()?;
    let error = String::from_utf8(first.stderr)?;
    assert!(!first.status.success());
    assert!(error.contains("llama-server missing"), "{error}");
    assert_eq!(
        fs::read_to_string(&transcript)?,
        "# Transcript\n\nIdeia \\*principal\\* continua.\n"
    );
    assert!(!path.join("sintese.md").exists());

    let edited = "# Transcrição revisada\n";
    fs::write(&transcript, edited)?;
    let retry = process.output()?;
    let error = String::from_utf8(retry.stderr)?;
    assert!(!retry.status.success());
    assert!(error.contains("llama-server missing"), "{error}");
    assert_eq!(fs::read_to_string(transcript)?, edited);
    assert_eq!(fs::read_to_string(path.join("whisper.json"))?, whisper);
    Ok(())
}

#[test]
fn processing_rejects_invalid_whisper_timestamps_without_creating_documents() -> Result<()> {
    let session = workspace()?;
    let path = session.path();
    write_audio(path)?;
    let whisper = json!({
        "transcription": [
            {"offsets": {"from": 200, "to": 100}, "text": "Ideia"}
        ]
    });
    fs::write(path.join("whisper.json"), whisper.to_string())?;

    let output = cli(path).args(["process", "."]).output()?;
    let error = String::from_utf8(output.stderr)?;
    assert!(!output.status.success());
    assert!(error.contains("Invalid timestamp range"), "{error}");
    assert!(!path.join("transcricao.md").exists());
    assert!(!path.join("sintese.md").exists());
    Ok(())
}

#[test]
fn restart_backs_up_all_results_and_preserves_audio_even_if_engine_is_missing() -> Result<()> {
    let session = workspace()?;
    let path = session.path();
    write_audio(path)?;
    let audio = fs::read(path.join("audio.wav"))?;
    for name in ["whisper.json", "transcricao.md", "sintese.md"] {
        fs::write(path.join(name), format!("previous {name}"))?;
    }
    let options = lovelace::ProcessingOptions::desktop(path, Some(lovelace::Language::Pt));
    let error =
        lovelace::pipeline::process_mode_with_progress(path, &options, true, |_| {}).unwrap_err();
    assert!(error.to_string().contains("whisper-cli missing"));
    assert_eq!(fs::read(path.join("audio.wav"))?, audio);
    let backup = fs::read_dir(path)?
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("restart-backup-")
        })
        .unwrap()
        .path();
    for name in ["whisper.json", "transcricao.md", "sintese.md"] {
        assert!(!path.join(name).exists());
        assert_eq!(
            fs::read_to_string(backup.join(name))?,
            format!("previous {name}")
        );
    }
    Ok(())
}

#[test]
fn restart_does_not_touch_results_while_session_is_locked() -> Result<()> {
    let session = workspace()?;
    let path = session.path();
    write_audio(path)?;
    fs::write(path.join("transcricao.md"), "Keep this transcript")?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.join(".processar.lock"))?;
    lock.try_lock()?;
    let options = lovelace::ProcessingOptions::desktop(path, None);
    let error =
        lovelace::pipeline::process_mode_with_progress(path, &options, true, |_| {}).unwrap_err();
    assert!(error.to_string().contains("Could not lock"));
    assert_eq!(
        fs::read_to_string(path.join("transcricao.md"))?,
        "Keep this transcript"
    );
    assert!(!fs::read_dir(path)?.any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("restart-backup-")
    }));
    Ok(())
}
