use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, path::Path};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhisperTranscript {
    pub language: Option<String>,
    pub segments: Vec<TranscriptSegment>,
}

impl WhisperTranscript {
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    pub fn duration_ms(&self) -> u64 {
        self.segments
            .iter()
            .map(|segment| segment.end_ms)
            .max()
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

pub fn read_whisper_transcript(path: &Path) -> Result<WhisperTranscript, String> {
    let data = fs::read(path)
        .map_err(|error| format!("Could not read Whisper transcript {}: {error}", path.display()))?;
    let root: Value = serde_json::from_slice(&data)
        .map_err(|error| format!("Could not parse Whisper transcript {}: {error}", path.display()))?;
    parse_whisper_transcript(&root)
}

fn parse_whisper_transcript(root: &Value) -> Result<WhisperTranscript, String> {
    let language = root
        .get("result")
        .and_then(|result| result.get("language"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let entries = root
        .get("transcription")
        .and_then(Value::as_array)
        .ok_or("Whisper JSON-full output is missing the transcription array.")?;
    let mut segments = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let text = entry
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if text.is_empty() {
            continue;
        }
        let offsets = entry
            .get("offsets")
            .ok_or_else(|| format!("Whisper segment {} is missing offsets.", index + 1))?;
        let start_ms = offsets
            .get("from")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("Whisper segment {} has no start offset.", index + 1))?;
        let end_ms = offsets
            .get("to")
            .and_then(Value::as_u64)
            .ok_or_else(|| format!("Whisper segment {} has no end offset.", index + 1))?;
        if end_ms < start_ms {
            return Err(format!(
                "Whisper segment {} ends before it starts ({} < {} ms).",
                index + 1,
                end_ms,
                start_ms
            ));
        }
        segments.push(TranscriptSegment {
            start_ms,
            end_ms,
            text: text.to_string(),
        });
    }

    if segments.is_empty() {
        return Err("Whisper transcript contains no timed text segments.".into());
    }
    Ok(WhisperTranscript { language, segments })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_json_full_segment_offsets_as_milliseconds() {
        let value = json!({
            "result": { "language": "en" },
            "transcription": [
                {
                    "timestamps": { "from": "00:00:00,000", "to": "00:00:02,340" },
                    "offsets": { "from": 0, "to": 2340 },
                    "text": " Hello world."
                },
                {
                    "offsets": { "from": 2340, "to": 5170 },
                    "text": " Next sentence."
                }
            ]
        });
        let transcript = parse_whisper_transcript(&value).unwrap();
        assert_eq!(transcript.language.as_deref(), Some("en"));
        assert_eq!(transcript.segment_count(), 2);
        assert_eq!(transcript.duration_ms(), 5170);
        assert_eq!(transcript.segments[0].text, "Hello world.");
    }

    #[test]
    fn rejects_missing_segment_timing() {
        let value = json!({
            "transcription": [{ "text": "Missing offsets" }]
        });
        assert!(parse_whisper_transcript(&value).is_err());
    }
}
