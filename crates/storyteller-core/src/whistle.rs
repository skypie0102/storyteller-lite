use crate::{transcript::validate_transcript, Transcript, TranscriptSegment};
use serde::Deserialize;

pub const WHISTLE_MAX_CHUNK_MS: u64 = 30_000;
pub const WHISTLE_LANGUAGE: &str = "en";
const FRAME_MS: u64 = 80;
const MAX_PHRASE_WORDS: usize = 24;

pub fn validate_whistle_language(language: &str) -> Result<(), String> {
    let language = language.trim().to_ascii_lowercase();
    if language == WHISTLE_LANGUAGE {
        Ok(())
    } else {
        Err(format!(
            "This app's Whistle integration is English-only (en); requested language: {language}."
        ))
    }
}

#[derive(Deserialize)]
struct WhistleOutput {
    text: String,
    language: String,
    words: Vec<WhistleWord>,
}

#[derive(Deserialize)]
struct WhistleWord {
    word: String,
    start: f64,
    end: f64,
    probability: f64,
}

/// Converts Whistle's attention-derived word times into phrase-level alignment input.
/// Silence is a valid chunk; speech without usable timestamps is an error.
pub fn parse_whistle_transcript(json: &str, duration_ms: u64) -> Result<Transcript, String> {
    if duration_ms == 0 || duration_ms > WHISTLE_MAX_CHUNK_MS {
        return Err("Whistle audio input must be positive and at most 30 seconds.".into());
    }
    let output: WhistleOutput = serde_json::from_str(json)
        .map_err(|error| format!("Whistle did not return valid timestamped JSON: {error}"))?;
    let language = output.language.trim();
    if !language.is_empty() && language != WHISTLE_LANGUAGE {
        return Err(format!(
            "Whistle returned language {language}; English (en) is required."
        ));
    }
    if output.text.trim().is_empty() {
        if !output.words.is_empty() {
            return Err("Whistle returned timed words for an empty transcript.".into());
        }
        return Ok(Transcript {
            language: None,
            segments: Vec::new(),
        });
    }
    if language.is_empty() || output.words.is_empty() {
        return Err("Whistle returned speech without a language or word timestamps.".into());
    }
    let timed_text = output
        .words
        .iter()
        .map(|word| word.word.as_str())
        .collect::<String>();
    if text_identity(&timed_text) != text_identity(&output.text) {
        return Err("Whistle word timestamps do not cover the complete transcript text.".into());
    }

    let mut segments = Vec::<TranscriptSegment>::new();
    let mut phrase = None::<TranscriptSegment>;
    let mut word_count = 0;
    let mut previous_start = 0.0;
    for (index, word) in output.words.iter().enumerate() {
        if word.word.trim().is_empty()
            || !word.start.is_finite()
            || !word.end.is_finite()
            || word.start < 0.0
            || word.end < word.start
            || word.start < previous_start
            || word.end * 1000.0 > (duration_ms + FRAME_MS) as f64
            || !word.probability.is_finite()
            || !(0.0..=1.0).contains(&word.probability)
        {
            return Err(format!(
                "Whistle word {} has invalid text, timing, or probability.",
                index + 1
            ));
        }
        previous_start = word.start;
        // Quantized attention may extend the final boundary by one encoder frame.
        // Clamp that boundary to the real clip; never synthesize times for missing words.
        let start_ms = (word.start * 1000.0).round() as u64;
        let end_ms = ((word.end * 1000.0).round() as u64).min(duration_ms);
        if start_ms > duration_ms || end_ms < start_ms {
            return Err(format!(
                "Whistle word {} starts beyond the audio clip.",
                index + 1
            ));
        }
        let should_split = phrase.as_ref().is_some_and(|current| {
            start_ms >= current.end_ms
                && (word_count >= MAX_PHRASE_WORDS
                    || start_ms.saturating_sub(current.end_ms) >= 700
                    || sentence_ended(&current.text))
        });
        if should_split {
            segments.push(phrase.take().expect("checked phrase"));
            word_count = 0;
        }
        match &mut phrase {
            Some(current) => {
                current.text.push(' ');
                current.text.push_str(word.word.trim());
                current.end_ms = current.end_ms.max(end_ms);
            }
            None => {
                phrase = Some(TranscriptSegment {
                    start_ms,
                    end_ms,
                    text: word.word.trim().into(),
                })
            }
        }
        word_count += 1;
    }
    if let Some(phrase) = phrase {
        segments.push(phrase);
    }
    let transcript = Transcript {
        language: Some(language.into()),
        segments,
    };
    validate_transcript(&transcript, true)?;
    Ok(transcript)
}

fn text_identity(text: &str) -> String {
    text.chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn sentence_ended(text: &str) -> bool {
    text.trim_end_matches(['\"', '\'', '”', '’', ')', ']'])
        .ends_with(['.', '!', '?'])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn speech() -> serde_json::Value {
        json!({"text":"Hello world. Next sentence.","language":"en","words":[
            {"word":"Hello","start":0.1,"end":0.4,"probability":0.98},
            {"word":"world.","start":0.4,"end":1.0,"probability":0.97},
            {"word":"Next","start":1.2,"end":1.5,"probability":0.99},
            {"word":"sentence.","start":1.5,"end":2.0,"probability":0.98}
        ]})
    }

    #[test]
    fn normalizes_seconds_to_phrase_milliseconds() {
        let transcript = parse_whistle_transcript(&speech().to_string(), 2_000).unwrap();
        assert_eq!(transcript.segments.len(), 2);
        assert_eq!(
            transcript.segments[0],
            TranscriptSegment {
                start_ms: 100,
                end_ms: 1000,
                text: "Hello world.".into()
            }
        );
        assert_eq!(transcript.segments[1].start_ms, 1200);
    }

    #[test]
    fn accepts_silence_without_inventing_text_or_timing() {
        let silence = r#"{"text":"","language":"","words":[],"ttft_ms":0,"decode_tps":0}"#;
        assert!(parse_whistle_transcript(silence, 1_000)
            .unwrap()
            .segments
            .is_empty());
    }

    #[test]
    fn rejects_missing_times_lost_words_and_invalid_ranges() {
        let mut value = speech();
        value.as_object_mut().unwrap().remove("words");
        assert!(parse_whistle_transcript(&value.to_string(), 2_000).is_err());
        let mut value = speech();
        value["words"].as_array_mut().unwrap().pop();
        assert!(parse_whistle_transcript(&value.to_string(), 2_000).is_err());
        for (field, number) in [("start", -1.0), ("end", 31.0), ("probability", 1.1)] {
            let mut value = speech();
            value["words"][0][field] = json!(number);
            assert!(parse_whistle_transcript(&value.to_string(), 2_000).is_err());
        }
    }

    #[test]
    fn overlapping_attention_words_share_one_phrase() {
        let mut value = speech();
        value["words"][2]["start"] = json!(0.9);
        let transcript = parse_whistle_transcript(&value.to_string(), 2_000).unwrap();
        assert_eq!(transcript.segments.len(), 1);
        assert_eq!(transcript.segments[0].text, "Hello world. Next sentence.");
    }

    #[test]
    fn rejects_non_english_requests_and_output() {
        for language in ["de", "fr", "es", "it", "nl", "pl", "ja", "auto", ""] {
            assert!(validate_whistle_language(language).is_err());
            if !language.is_empty() {
                let mut value = speech();
                value["language"] = json!(language);
                assert!(parse_whistle_transcript(&value.to_string(), 2_000).is_err());
            }
        }
        assert!(validate_whistle_language("EN").is_ok());
    }

    #[test]
    fn rejects_oversize_clips() {
        assert!(parse_whistle_transcript(&speech().to_string(), 30_001).is_err());
    }
}
