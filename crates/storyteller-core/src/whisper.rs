use crate::{
    contextual_transcript::{normalize_timed_words, text_identity},
    transcript::validate_transcript,
    TimedWord, Transcript, TranscriptSegment,
};
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
    tokens: Option<Vec<Token>>,
}

#[derive(Clone, Copy, Deserialize)]
struct Offsets {
    from: u64,
    to: u64,
}

#[derive(Deserialize)]
struct Token {
    text: String,
    offsets: Option<Offsets>,
    p: Option<f64>,
}

fn is_silence(text: &str) -> bool {
    text.is_empty() || matches!(text, "[BLANK_AUDIO]" | "[Silence]" | "[silence]")
}

/// Parses whisper.cpp's native JSON. Offsets are milliseconds, not seconds.
/// Empty chunks are valid; the shared merger rejects an entirely empty book.
pub fn parse_whisper_transcript(json: &str, duration_ms: u64) -> Result<Transcript, String> {
    if duration_ms == 0 {
        return Err("Whisper audio input must have a positive duration.".into());
    }
    let output: WhisperOutput = serde_json::from_str(json)
        .map_err(|error| format!("Whisper did not return valid timestamped JSON: {error}"))?;
    if output.params.language != "en" || output.result.language != "en" || output.params.translate {
        return Err("Whisper must return English transcription without translation.".into());
    }
    let mut segments = Vec::new();
    for segment in output.transcription {
        let text = segment.text.trim();
        if is_silence(text) {
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

#[derive(Default)]
struct WordBuilder {
    text: String,
    offsets: Option<Offsets>,
}

impl WordBuilder {
    fn append(&mut self, text: &str, offsets: Option<Offsets>) {
        self.text.push_str(text);
        if let Some(offsets) = offsets {
            self.offsets = Some(match self.offsets {
                Some(current) => Offsets {
                    from: current.from.min(offsets.from),
                    to: current.to.max(offsets.to),
                },
                None => offsets,
            });
        }
    }

    fn finish(&mut self, words: &mut Vec<TimedWord>) -> Result<(), String> {
        if self.text.is_empty() {
            return Ok(());
        }
        let Some(offsets) = self.offsets else {
            if !text_identity(&self.text).is_empty() {
                return Err("Whisper returned a word without native token timestamps.".into());
            }
            // Untimed punctuation can extend an existing word. Leading punctuation
            // stays in the builder until a timed word follows it.
            if let Some(previous) = words.last_mut() {
                previous.text.push_str(&std::mem::take(&mut self.text));
            }
            return Ok(());
        };
        words.push(TimedWord {
            start_ms: offsets.from,
            end_ms: offsets.to,
            text: std::mem::take(&mut self.text),
        });
        self.offsets = None;
        Ok(())
    }
}

/// Reads whisper.cpp --output-json-full, joining BPE continuations into whole words.
/// Every word must retain a native timed fragment; segment-level guesses are rejected.
pub fn parse_whisper_words(json: &str, duration_ms: u64) -> Result<Vec<TimedWord>, String> {
    // Retain the native language, silence, segment ordering and tail checks.
    parse_whisper_transcript(json, duration_ms)?;
    let output: WhisperOutput = serde_json::from_str(json)
        .map_err(|error| format!("Whisper did not return valid timestamped JSON: {error}"))?;
    let mut words = Vec::new();
    for segment in output.transcription {
        if is_silence(segment.text.trim()) {
            continue;
        }
        let tokens = segment
            .tokens
            .ok_or("Whisper speech requires full token timestamp JSON.")?;
        let first_word = words.len();
        let mut builder = WordBuilder::default();
        for token in tokens {
            if token
                .p
                .is_some_and(|p| !p.is_finite() || !(0.0..=1.0).contains(&p))
            {
                return Err("Whisper returned an invalid token probability.".into());
            }
            if token.text.starts_with("[_") && token.text.ends_with(']') {
                continue;
            }
            let offsets = token
                .offsets
                .map(|offsets| -> Result<Offsets, String> {
                    if offsets.from > duration_ms
                        || offsets.to < offsets.from
                        || offsets.to > duration_ms.saturating_add(10)
                    {
                        return Err(
                            "Whisper returned token timestamps outside the audio chunk.".into()
                        );
                    }
                    Ok(Offsets {
                        from: offsets.from,
                        to: offsets.to.min(duration_ms),
                    })
                })
                .transpose()?;
            // Whitespace begins a word; all continuation and punctuation pieces
            // keep their original text and expand only by native token times.
            for fragment in token.text.split_inclusive(char::is_whitespace) {
                let text = fragment.trim();
                if !text.is_empty() {
                    builder.append(text, offsets);
                }
                if fragment.ends_with(char::is_whitespace) {
                    builder.finish(&mut words)?;
                }
            }
        }
        builder.finish(&mut words)?;
        let timed_text = words[first_word..]
            .iter()
            .map(|word| word.text.as_str())
            .collect::<String>();
        if words.len() == first_word
            || !builder.text.is_empty()
            || text_identity(&timed_text) != text_identity(&segment.text)
        {
            return Err(
                "Whisper token timestamps do not cover the complete transcript text.".into(),
            );
        }
    }
    if words
        .windows(2)
        .any(|pair| pair[1].start_ms < pair[0].start_ms)
    {
        return Err("Whisper returned unordered word timestamps.".into());
    }
    normalize_timed_words(&words, Some("en".into()))?;
    Ok(words)
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
        assert!(parse_whisper_transcript(&good, 30_001).is_ok());
    }

    #[test]
    fn full_json_joins_continuations_and_keeps_native_word_times() {
        let json = native(
            r#"[{"offsets":{"from":0,"to":1000},"text":" The lighthouse stands.","tokens":[
            {"text":"[_BEG_]"},
            {"text":" The","offsets":{"from":100,"to":200},"p":0.9},
            {"text":" light","offsets":{"from":200,"to":350}},
            {"text":"house","offsets":{"from":350,"to":600}},
            {"text":" stands","offsets":{"from":600,"to":900}},
            {"text":"."}, {"text":"[_TT_100]"}
        ]}]"#,
        );
        assert_eq!(
            parse_whisper_words(&json, 1000).unwrap(),
            vec![
                TimedWord {
                    start_ms: 100,
                    end_ms: 200,
                    text: "The".into()
                },
                TimedWord {
                    start_ms: 200,
                    end_ms: 600,
                    text: "lighthouse".into()
                },
                TimedWord {
                    start_ms: 600,
                    end_ms: 900,
                    text: "stands.".into()
                },
            ]
        );
    }

    #[test]
    fn full_json_accepts_native_words_beyond_thirty_seconds() {
        let json = native(
            r#"[{"offsets":{"from":60100,"to":61500},"text":" Later speech.","tokens":[
            {"text":" Later","offsets":{"from":60100,"to":60700}},
            {"text":" speech.","offsets":{"from":60700,"to":61500}}
        ]}]"#,
        );
        let words = parse_whisper_words(&json, 300_000).unwrap();
        assert_eq!(words[1].end_ms, 61_500);
        assert!(parse_whisper_words(&json, 30_000).is_err());
    }

    #[test]
    fn full_json_requires_complete_word_times_and_valid_token_ranges() {
        let good = native(
            r#"[{"offsets":{"from":0,"to":1000},"text":" Hello.","tokens":[{"text":" Hello","offsets":{"from":100,"to":900},"p":0.9},{"text":"."}]}]"#,
        );
        for invalid in [
            good.replace("\"tokens\"", "\"ignored\""),
            good.replace(",\"offsets\":{\"from\":100,\"to\":900}", ""),
            good.replace("\"text\":\" Hello.\"", "\"text\":\" Hello world.\""),
            good.replace("\"to\":900", "\"to\":1200"),
            good.replace("\"to\":900", "\"to\":99"),
            good.replace("\"p\":0.9", "\"p\":1.1"),
        ] {
            assert!(parse_whisper_words(&invalid, 1000).is_err(), "{invalid}");
        }
        assert!(parse_whisper_words(&native("[]"), 1000).unwrap().is_empty());
    }

    #[test]
    fn zero_width_native_fragments_and_untimed_punctuation_keep_all_text() {
        let json = native(
            r#"[{"offsets":{"from":0,"to":1000},"text":" Very, very quiet!","tokens":[
            {"text":" Very","offsets":{"from":100,"to":300}}, {"text":","},
            {"text":" very","offsets":{"from":400,"to":400}},
            {"text":" quiet","offsets":{"from":400,"to":800}}, {"text":"!"}
        ]}]"#,
        );
        let words = parse_whisper_words(&json, 1000).unwrap();
        assert_eq!(
            words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(),
            vec!["Very,", "very", "quiet!"]
        );
        assert_eq!(words[1].start_ms, words[1].end_ms);
    }
}
