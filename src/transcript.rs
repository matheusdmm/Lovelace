//! Readable, verbatim transcripts. Timing stays in Whisper data and summary links.

use crate::pipeline::{Segment, markdown_text, parse_whisper};

pub struct Transcript {
    pub markdown: String,
    /// Zero-based paragraph index for each one-based summary segment ID.
    pub segment_paragraphs: Vec<usize>,
}

struct Part {
    from: u64,
    to: u64,
    markdown: String,
}

pub fn from_segments(segments: &[Segment]) -> Transcript {
    let parts = segments
        .iter()
        .map(|segment| {
            let (from, to) = segment.range_ms();
            // Preserve explicit paragraph breaks, while joining wrapped lines.
            let mut paragraphs = vec![String::new()];
            for line in segment.text.lines() {
                if line.trim().is_empty() {
                    if !paragraphs.last().unwrap().is_empty() {
                        paragraphs.push(String::new());
                    }
                } else {
                    let paragraph = paragraphs.last_mut().unwrap();
                    if !paragraph.is_empty() {
                        paragraph.push(' ');
                    }
                    paragraph.push_str(line.trim());
                }
            }
            Part {
                from,
                to,
                markdown: paragraphs
                    .iter()
                    .filter(|paragraph| !paragraph.is_empty())
                    .map(|paragraph| markdown_text(paragraph))
                    .collect::<Vec<_>>()
                    .join("\n\n"),
            }
        })
        .collect::<Vec<_>>();
    assemble(&parts)
}

/// Present old generated documents without their segment headings or timestamps.
/// Custom Markdown and the original files are left intact.
pub fn for_review(markdown: &str, whisper: Option<&[u8]>) -> Transcript {
    let normalized = markdown.replace("\r\n", "\n");
    if let Some(parts) = legacy_parts(&normalized) {
        return assemble(&parts);
    }
    if let Some(output) = whisper.and_then(|bytes| parse_whisper(bytes).ok()) {
        let transcript = from_segments(&output.transcription);
        if transcript.markdown.trim() == normalized.trim() {
            return Transcript {
                markdown: markdown.to_owned(),
                ..transcript
            };
        }
    }
    Transcript {
        markdown: markdown.to_owned(),
        segment_paragraphs: Vec::new(),
    }
}

fn assemble(parts: &[Part]) -> Transcript {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut segment_paragraphs = Vec::new();
    let mut previous_end = 0;
    for part in parts {
        let paragraph = paragraphs.last().map(String::as_str).unwrap_or("");
        let sentence_end = paragraph
            .trim_end_matches(['"', '\'', '”', '’', ')', ']'])
            .ends_with(['.', '!', '?', '…']);
        let length = paragraph.chars().count();
        let clause_end = paragraph.ends_with([',', ';', ':']);
        // Whisper boundaries often split a sentence. Only infer a paragraph at
        // a sentence boundary when possible. Some recordings have almost no
        // sentence punctuation, so long passages also break at clauses or,
        // as a fallback, between segments without changing any speech.
        let new_paragraph = paragraph.is_empty()
            || (sentence_end && (part.from.saturating_sub(previous_end) >= 1_500 || length >= 600))
            || (clause_end && length >= 800)
            || length >= 1_200;
        if new_paragraph {
            paragraphs.push(String::new());
        }
        segment_paragraphs.push(paragraphs.len() - 1);
        for (index, text) in part.markdown.split("\n\n").enumerate() {
            if index > 0 {
                paragraphs.push(String::new());
            }
            let paragraph = paragraphs.last_mut().unwrap();
            if !paragraph.is_empty() && !text.is_empty() {
                paragraph.push(' ');
            }
            paragraph.push_str(text);
        }
        previous_end = part.to;
    }
    let mut markdown = String::from("# Transcript\n\n");
    if parts.is_empty() {
        markdown.push_str("No speech segments found.\n");
    } else {
        markdown.push_str(&paragraphs.join("\n\n"));
        markdown.push('\n');
    }
    Transcript {
        markdown,
        segment_paragraphs,
    }
}

fn legacy_parts(markdown: &str) -> Option<Vec<Part>> {
    let body = markdown
        .strip_prefix("# Transcript\n\n")
        .or_else(|| markdown.strip_prefix("# Transcrição\n\n"))?;
    let blocks = body.trim_end().split("\n\n").collect::<Vec<_>>();
    if blocks.len() % 3 != 0 {
        return None;
    }
    let mut parts = Vec::new();
    for (index, block) in blocks.chunks_exact(3).enumerate() {
        let id = block[0]
            .strip_prefix("### Segment ")
            .or_else(|| block[0].strip_prefix("### Trecho "))?
            .parse::<usize>()
            .ok()?;
        if id != index + 1 || block[2].contains('\n') {
            return None;
        }
        let (from, to) = block[1]
            .strip_prefix("**")?
            .strip_suffix("**")?
            .split_once(" – ")?;
        let (from, to) = (parse_time(from)?, parse_time(to)?);
        if from > to {
            return None;
        }
        parts.push(Part {
            from,
            to,
            markdown: block[2].to_owned(),
        });
    }
    Some(parts)
}

fn parse_time(text: &str) -> Option<u64> {
    let (hours, rest) = text.split_once(':')?;
    let (minutes, rest) = rest.split_once(':')?;
    let (seconds, millis) = rest.split_once(['.', ','])?;
    let (hours, minutes, seconds, millis) = (
        hours.parse::<u64>().ok()?,
        minutes.parse::<u64>().ok()?,
        seconds.parse::<u64>().ok()?,
        millis.parse::<u64>().ok()?,
    );
    if minutes >= 60 || seconds >= 60 || millis >= 1_000 {
        return None;
    }
    hours
        .checked_mul(60)?
        .checked_add(minutes)?
        .checked_mul(60)?
        .checked_add(seconds)?
        .checked_mul(1_000)?
        .checked_add(millis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn segment(from: u64, to: u64, text: &str) -> Segment {
        serde_json::from_value(json!({
            "offsets": {"from": from, "to": to}, "text": text,
        }))
        .unwrap()
    }

    #[test]
    fn joins_fragments_without_rewriting_or_removing_speech() {
        let transcript = from_segments(&[
            segment(0, 1_000, " Cara, vamos lá então, falar um pouco mais "),
            segment(1_000, 2_000, "ou menos sobre o Lovelace, né?"),
            segment(2_000, 3_000, "É, é isso mesmo. *Literal* <texto>."),
        ]);
        assert_eq!(
            transcript.markdown,
            "# Transcript\n\nCara, vamos lá então, falar um pouco mais ou menos sobre o Lovelace, né? É, é isso mesmo. \\*Literal\\* \\<texto\\>.\n"
        );
        assert_eq!(transcript.segment_paragraphs, [0, 0, 0]);
    }

    #[test]
    fn paragraphs_follow_sentence_boundaries_and_preserve_explicit_breaks() {
        let transcript = from_segments(&[
            segment(0, 1_000, "Uma frase que"),
            segment(4_000, 5_000, "continua apesar da pausa."),
            segment(
                7_000,
                8_000,
                "Outro assunto.\n\nMais um parágrafo.\nLinha quebrada.",
            ),
            segment(8_000, 9_000, "Ainda aqui."),
        ]);
        assert_eq!(
            transcript.markdown,
            "# Transcript\n\nUma frase que continua apesar da pausa.\n\nOutro assunto.\n\nMais um parágrafo. Linha quebrada. Ainda aqui.\n"
        );
        assert_eq!(transcript.segment_paragraphs, [0, 0, 1, 2]);
    }

    #[test]
    fn continuous_speech_uses_sentence_endings_to_form_readable_paragraphs() {
        let long_sentence = format!("{}fim.", "palavra ".repeat(80));
        let transcript = from_segments(&[
            segment(0, 1_000, &long_sentence),
            segment(1_000, 2_000, "Próxima frase completa."),
        ]);
        assert_eq!(
            transcript.markdown,
            format!("# Transcript\n\n{long_sentence}\n\nPróxima frase completa.\n")
        );
        assert_eq!(transcript.segment_paragraphs, [0, 1]);
    }

    #[test]
    fn sparsely_punctuated_speech_still_has_paragraphs_without_losing_words() {
        let clause = format!("{}né,", "palavra ".repeat(105));
        let unpunctuated = "fala ".repeat(245).trim().to_owned();
        let transcript = from_segments(&[
            segment(0, 1_000, &clause),
            segment(1_000, 2_000, &unpunctuated),
            segment(2_000, 3_000, "continua sem pontuação"),
        ]);
        assert_eq!(
            transcript.markdown,
            format!("# Transcript\n\n{clause}\n\n{unpunctuated}\n\ncontinua sem pontuação\n")
        );
        assert_eq!(transcript.segment_paragraphs, [0, 1, 2]);
    }

    #[test]
    fn old_documents_keep_all_speech_and_segment_targets_in_both_languages() {
        for (title, heading) in [("Transcript", "Segment"), ("Transcrição", "Trecho")] {
            let markdown = format!(
                "# {title}\n\n### {heading} 0001\n\n**00:00:00.000 – 00:00:01.000**\n\nIdeia \\*literal\\*\n\n### {heading} 0002\n\n**00:00:01.000 – 00:00:02.000**\n\ncontinua.\n\n### {heading} 0003\n\n**00:00:04.000 – 00:00:05.000**\n\nOutra ideia.\n\n"
            );
            let transcript = for_review(&markdown.replace('\n', "\r\n"), None);
            assert_eq!(
                transcript.markdown,
                "# Transcript\n\nIdeia \\*literal\\* continua.\n\nOutra ideia.\n"
            );
            assert_eq!(transcript.segment_paragraphs, [0, 0, 1]);
        }
    }

    #[test]
    fn review_maps_new_documents_but_preserves_custom_edits() {
        let whisper = json!({"transcription": [
            {"offsets": {"from": 0, "to": 1000}, "text": "Olá."},
            {"offsets": {"from": 3000, "to": 4000}, "text": "Tudo bem?"},
        ]})
        .to_string();
        let markdown = "# Transcript\n\nOlá.\n\nTudo bem?\n";
        let transcript = for_review(markdown, Some(whisper.as_bytes()));
        assert_eq!(transcript.markdown, markdown);
        assert_eq!(transcript.segment_paragraphs, [0, 1]);

        for bytes in [whisper.as_bytes(), b"invalid JSON", b""] {
            let edited = "# Transcript\n\nTexto revisado.\n\n00:00:01 é parte da fala.\n";
            let transcript = for_review(edited, Some(bytes));
            assert_eq!(transcript.markdown, edited);
            assert!(transcript.segment_paragraphs.is_empty());
        }
    }

    #[test]
    fn partial_or_custom_segment_documents_are_not_destructively_reformatted() {
        let markdown = "# Transcript\n\n### Segment 0001\n\n**00:00:00.000 – 00:00:01.000**\n\nFala.\n\nUma anotação extra.\n";
        assert_eq!(for_review(markdown, None).markdown, markdown);
    }
}
