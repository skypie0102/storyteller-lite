use crate::{transcript::validate_transcript, Transcript, TranscriptSegment};
use serde::Deserialize;

#[derive(Deserialize)]
struct WhisperOutput {
    params: Parameters,
    result: Language,
    transcription: Vec<Segment>,
}

#[derive(Deserialize)]
struct Parameters {
    language: String,
    translate: bool,
}

#[derive(Deserialize)]
struct Language {
    language: String,
}

#[derive(Deserialize)]
struct Segment {
    offsets: Offsets,
    text: String,
}

#[derive(Deserialize)]
struct Offsets {
    from: u64,
    to: u64,
}

/// Parses whisper.cpp's native JSON. Offsets are milliseconds, not seconds.
/// Empty chunks are valid; the shared merger rejects an entirely empty book.
pub fn parse_whisper_transcript(json: &str, duration_ms: u64) -> Result<Transcript, String> {
    if duration_ms == 0 || duration_ms > crate::WHISTLE_MAX_CHUNK_MS {
        return Err("Whisper chunk input must be positive and at most 30 seconds.".into());
    }
    let output: WhisperOutput = serde_json::from_str(json)
        .map_err(|error| format!("Whisper did not return valid timestamped JSON: {error}"))?;
    if output.params.language != "en" || output.result.language != "en" || output.params.translate {
        return Err("Whisper must return English transcription without translation.".into());
    }
    let mut segments = Vec::new();
    for segment in output.transcription {
        let text = segment.text.trim();
        if text.is_empty() || matches!(text, "[BLANK_AUDIO]" | "[Silence]" | "[silence]") {
            continue;
        }
        // Native times have 10 ms resolution; permit at most one frame past the tail.
        if segment.offsets.from >= duration_ms
            || segment.offsets.to > duration_ms.saturating_add(10)
        {
            return Err("Whisper returned timestamps outside the audio chunk.".into());
        }
        segments.push(TranscriptSegment {
            start_ms: segment.offsets.from,
            end_ms: segment.offsets.to.min(duration_ms),
            text: text.into(),
        });
    }
    let transcript = Transcript {
        language: Some("en".into()),
        segments,
    };
    validate_transcript(&transcript, false)?;
    Ok(transcript)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native(segments: &str) -> String {
        format!(
            r#"{{"params":{{"language":"en","translate":false}},"result":{{"language":"en"}},"transcription":{segments}}}"#
        )
    }

    #[test]
    fn native_offsets_are_milliseconds_and_silence_is_empty() {
        let parsed = parse_whisper_transcript(
            &native(r#"[{"offsets":{"from":120,"to":1850},"text":" Hello world. "}]"#),
            2000,
        )
        .unwrap();
        assert_eq!(
            parsed.segments[0],
            TranscriptSegment {
                start_ms: 120,
                end_ms: 1850,
                text: "Hello world.".into()
            }
        );
        assert!(parse_whisper_transcript(&native("[]"), 2000)
            .unwrap()
            .segments
            .is_empty());
        assert!(parse_whisper_transcript(
            &native(r#"[{"offsets":{"from":0,"to":1000},"text":"[BLANK_AUDIO]"}]"#),
            2000
        )
        .unwrap()
        .segments
        .is_empty());
    }

    #[test]
    fn invalid_language_translation_and_times_fail() {
        let good = native(r#"[{"offsets":{"from":120,"to":1850},"text":"Hello"}]"#);
        for invalid in [
            good.replace("\"en\"", "\"fr\""),
            good.replace("false", "true"),
            good.replace("1850", "4000"),
            good.replace("1850", "0"),
            good.replace("120", "-1"),
        ] {
            assert!(parse_whisper_transcript(&invalid, 2000).is_err());
        }
        assert!(parse_whisper_transcript("{}", 2000).is_err());
        assert!(parse_whisper_transcript(&good, 30_001).is_err());
    }
}
