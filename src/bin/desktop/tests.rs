use super::*;

fn desktop(root: &Path) -> Desktop {
    Desktop {
        root: root.into(),
        job: Arc::default(),
        level: Arc::default(),
    }
}

fn session(state: &Desktop, id: &str) -> Result<PathBuf> {
    let path = state.sessions().join(id);
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn write_audio(path: &Path, has_samples: bool) -> Result<()> {
    let mut wav = hound::WavWriter::create(
        path.join("audio.wav"),
        hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    if has_samples {
        wav.write_sample(0_i16)?;
    }
    wav.finalize()?;
    Ok(())
}

#[test]
fn history_is_empty_before_the_first_recording() -> Result<()> {
    let root = tempfile::tempdir()?;
    assert!(list_sessions(&desktop(root.path()))?.is_empty());
    Ok(())
}

#[test]
fn history_sorts_completed_recordings_and_reports_available_documents() -> Result<()> {
    let root = tempfile::tempdir()?;
    let state = desktop(root.path());
    let recorded = session(&state, "sessao-900-0")?;
    let transcribed = session(&state, "sessao-1000-2")?;
    let summarized = session(&state, "sessao-1000-10")?;
    for path in [&recorded, &transcribed, &summarized] {
        write_audio(path, true)?;
    }
    fs::write(transcribed.join("transcricao.md"), "# Transcrição")?;
    fs::write(summarized.join("transcricao.md"), "# Transcrição")?;
    fs::write(summarized.join("sintese.md"), "# Síntese")?;
    session(&state, "sessao-2000-0")?;
    write_audio(&session(&state, "sessao-2000-1")?, false)?;
    write_audio(&session(&state, "rascunho")?, true)?;

    let sessions = list_sessions(&state)?;
    let entries: Vec<_> = sessions
        .iter()
        .map(|item| (item.id.as_str(), item.has_transcript, item.has_summary))
        .collect();

    assert_eq!(
        entries,
        [
            ("sessao-1000-10", true, true),
            ("sessao-1000-2", true, false),
            ("sessao-900-0", false, false),
        ]
    );
    Ok(())
}

#[test]
fn opening_a_session_requires_a_known_session_directory() -> Result<()> {
    let root = tempfile::tempdir()?;
    let state = desktop(root.path());
    let path = session(&state, "sessao-1000-0")?;
    fs::write(state.sessions().join("sessao-1000-1"), "arquivo")?;

    assert_eq!(
        state.session("sessao-1000-0")?.canonicalize()?,
        path.canonicalize()?
    );
    for id in ["rascunho", "sessao-1000-1", "sessao-1000-2"] {
        assert!(state.session(id).is_err(), "{id}");
    }
    Ok(())
}

#[test]
fn review_keeps_markdown_and_renders_transcript_links() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("sintese.md");
    let markdown = concat!(
        "# Síntese\n\n",
        "[00:00:01.000](transcricao.md#segment-0001)\n\n",
        "[00:00:02.000](transcricao.md#trecho-0002)\n\n",
        "[Link externo](https://example.com/)\n\n",
        "<b>Texto literal</b>"
    );
    assert!(read_document(path.clone())?.is_none());
    fs::write(&path, markdown)?;

    let document = read_document(path)?.expect("o documento deve existir");

    assert_eq!(document.markdown, markdown);
    assert!(document.html.contains("<h1>Síntese</h1>"));
    assert!(
        document
            .html
            .contains(r#"<a href="transcricao.md#segment-0001">00:00:01.000</a>"#)
    );
    assert!(
        document
            .html
            .contains(r#"<a href="transcricao.md#trecho-0002">00:00:02.000</a>"#)
    );
    assert!(document.html.contains(r##"<a href="#">Link externo</a>"##));
    assert!(document.html.contains("&lt;b&gt;Texto literal&lt;/b&gt;"));
    Ok(())
}
