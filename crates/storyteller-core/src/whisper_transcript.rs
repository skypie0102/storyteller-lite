use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, path::Path};

pub const DEFAULT_MAX_TRANSCRIPTION_CHUNK_MS: u64 = 60 * 60 * 1000;
pub const CHAPTER_BOUNDARY_TOLERANCE_MS: u64 = 10 * 60 * 1000;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptionChunk {
    pub index: usize,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl TranscriptionChunk {
    pub fn duration_ms(self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }
}

pub fn read_whisper_transcript(path: &Path) -> Result<WhisperTranscript, String> {
    let data = fs::read(path).map_err(|error| {
        format!(
            "Could not read Whisper transcript {}: {error}",
            path.display()
        )
    })?;
    let root: Value = serde_json::from_slice(&data).map_err(|error| {
        format!(
            "Could not parse Whisper transcript {}: {error}",
            path.display()
        )
    })?;

    let transcript = if root.get("segments").is_some() {
        serde_json::from_value::<WhisperTranscript>(root).map_err(|error| {
            format!(
                "Could not parse normalized Whisper transcript {}: {error}",
                path.display()
            )
        })?
    } else {
        parse_whisper_json_full(&root)?
    };
    validate_transcript(&transcript)?;
    Ok(transcript)
}

pub fn write_whisper_transcript(path: &Path, transcript: &WhisperTranscript) -> Result<(), String> {
    validate_transcript(transcript)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Could not create transcript directory {}: {error}",
                parent.display()
            )
        })?;
    }
    let json = serde_json::to_vec_pretty(transcript)
        .map_err(|error| format!("Could not serialize merged Whisper transcript: {error}"))?;
    fs::write(path, json).map_err(|error| {
        format!(
            "Could not write merged Whisper transcript {}: {error}",
            path.display()
        )
    })
}

pub fn plan_transcription_chunks(
    duration_ms: u64,
    chapter_boundaries_ms: &[u64],
    max_chunk_ms: u64,
) -> Result<Vec<TranscriptionChunk>, String> {
    if duration_ms == 0 {
        return Err("Audiobook duration must be positive before transcription chunking.".into());
    }
    if max_chunk_ms == 0 {
        return Err("Maximum transcription chunk duration must be positive.".into());
    }

    let tolerance_ms = CHAPTER_BOUNDARY_TOLERANCE_MS.min(max_chunk_ms / 2);
    let mut chapters = chapter_boundaries_ms
        .iter()
        .copied()
        .filter(|boundary| *boundary > 0 && *boundary < duration_ms)
        .collect::<Vec<_>>();
    chapters.sort_unstable();
    chapters.dedup();

    let mut chunks = Vec::new();
    let mut start_ms = 0u64;
    while start_ms < duration_ms {
        let remaining = duration_ms - start_ms;
        let soft_max = max_chunk_ms.saturating_add(tolerance_ms);
        let end_ms = if remaining <= soft_max {
            duration_ms
        } else {
            let target = start_ms.saturating_add(max_chunk_ms).min(duration_ms);
            let lower = target.saturating_sub(tolerance_ms).max(start_ms + 1);
            let upper = target.saturating_add(tolerance_ms).min(duration_ms - 1);
            chapters
                .iter()
                .copied()
                .filter(|boundary| *boundary >= lower && *boundary <= upper)
                .min_by_key(|boundary| boundary.abs_diff(target))
                .unwrap_or(target)
        };
        if end_ms <= start_ms {
            return Err("Transcription chunk planner produced a non-positive range.".into());
        }
        chunks.push(TranscriptionChunk {
            index: chunks.len(),
            start_ms,
            end_ms,
        });
        start_ms = end_ms;
    }
    validate_chunk_plan(duration_ms, &chunks)?;
    Ok(chunks)
}

pub fn validate_chunk_plan(duration_ms: u64, chunks: &[TranscriptionChunk]) -> Result<(), String> {
    if chunks.is_empty() {
        return Err("Transcription chunk plan is empty.".into());
    }
    if chunks[0].start_ms != 0 {
        return Err("Transcription chunk plan does not begin at the start of the audiobook.".into());
    }
    let mut previous_end = 0u64;
    for (position, chunk) in chunks.iter().enumerate() {
        if chunk.index != position {
            return Err("Transcription chunk indices are not contiguous.".into());
        }
        if chunk.start_ms != previous_end || chunk.end_ms <= chunk.start_ms {
            return Err("Transcription chunks must form one continuous positive timeline.".into());
        }
        if chunk.end_ms > duration_ms {
            return Err("Transcription chunk extends beyond the audiobook duration.".into());
        }
        previous_end = chunk.end_ms;
    }
    if previous_end != duration_ms {
        return Err("Transcription chunk plan does not cover the complete audiobook.".into());
    }
    Ok(())
}

pub fn merge_chunk_transcripts(
    duration_ms: u64,
    parts: &[(TranscriptionChunk, WhisperTranscript)],
) -> Result<WhisperTranscript, String> {
    let chunks = parts.iter().map(|(chunk, _)| *chunk).collect::<Vec<_>>();
    validate_chunk_plan(duration_ms, &chunks)?;

    let mut language = None::<String>;
    let mut segments = Vec::new();
    let mut previous_end_ms = 0u64;
    for (chunk, transcript) in parts {
        validate_transcript(transcript)?;
        if language.is_none() {
            language = transcript.language.clone();
        }
        let chunk_duration = chunk.duration_ms();
        for segment in &transcript.segments {
            if segment.end_ms > chunk_duration {
                return Err(format!(
                    "Whisper chunk {} contains a segment ending at {} ms beyond its {} ms duration.",
                    chunk.index + 1,
                    segment.end_ms,
                    chunk_duration
                ));
            }
            let start_ms = chunk
                .start_ms
                .checked_add(segment.start_ms)
                .ok_or("Merged Whisper segment start overflowed.")?;
            let end_ms = chunk
                .start_ms
                .checked_add(segment.end_ms)
                .ok_or("Merged Whisper segment end overflowed.")?;
            if !segments.is_empty() && start_ms < previous_end_ms {
                return Err(format!(
                    "Merged Whisper chunk {} overlaps the previous transcript at {} ms.",
                    chunk.index + 1,
                    start_ms
                ));
            }
            previous_end_ms = end_ms;
            segments.push(TranscriptSegment {
                start_ms,
                end_ms,
                text: segment.text.clone(),
            });
        }
    }

    let merged = WhisperTranscript { language, segments };
    validate_transcript(&merged)?;
    Ok(merged)
}

fn parse_whisper_json_full(root: &Value) -> Result<WhisperTranscript, String> {
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
        segments.push(TranscriptSegment {
            start_ms,
            end_ms,
            text: text.to_string(),
        });
    }
    Ok(WhisperTranscript { language, segments })
}

fn validate_transcript(transcript: &WhisperTranscript) -> Result<(), String> {
    let mut previous_end_ms = 0u64;
    let mut seen = 0usize;
    for (index, segment) in transcript.segments.iter().enumerate() {
        if segment.text.trim().is_empty() {
            return Err(format!("Whisper segment {} has blank text.", index + 1));
        }
        if segment.end_ms <= segment.start_ms {
            return Err(format!(
                "Whisper segment {} has a non-positive duration ({}..{} ms).",
                index + 1,
                segment.start_ms,
                segment.end_ms
            ));
        }
        if seen > 0 && segment.start_ms < previous_end_ms {
            return Err(format!(
                "Whisper segment {} overlaps or moves backward in time ({} ms starts before the previous end at {} ms).",
                index + 1,
                segment.start_ms,
                previous_end_ms
            ));
        }
        previous_end_ms = segment.end_ms;
        seen += 1;
    }
    if seen == 0 {
        return Err("Whisper transcript contains no timed text segments.".into());
    }
    Ok(())
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
        let transcript = parse_whisper_json_full(&value).unwrap();
        validate_transcript(&transcript).unwrap();
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
        assert!(parse_whisper_json_full(&value).is_err());
    }

    #[test]
    fn rejects_zero_duration_and_overlapping_segments() {
        let zero = WhisperTranscript {
            language: None,
            segments: vec![TranscriptSegment {
                start_ms: 100,
                end_ms: 100,
                text: "Zero duration".into(),
            }],
        };
        assert!(validate_transcript(&zero).is_err());

        let overlap = WhisperTranscript {
            language: None,
            segments: vec![
                TranscriptSegment {
                    start_ms: 0,
                    end_ms: 1000,
                    text: "First".into(),
                },
                TranscriptSegment {
                    start_ms: 900,
                    end_ms: 1500,
                    text: "Second".into(),
                },
            ],
        };
        assert!(validate_transcript(&overlap).is_err());
    }

    #[test]
    fn plans_chunks_near_chapter_boundaries_and_avoids_tiny_tail() {
        let hour = 60 * 60 * 1000;
        let duration = 125 * 60 * 1000;
        let chapters = vec![58 * 60 * 1000, 119 * 60 * 1000];
        let chunks = plan_transcription_chunks(duration, &chapters, hour).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].end_ms, 58 * 60 * 1000);
        assert_eq!(chunks[1].end_ms, duration);
    }

    #[test]
    fn merges_local_chunk_timestamps_into_global_timeline() {
        let chunks = plan_transcription_chunks(120_000, &[], 60_000).unwrap();
        let parts = vec![
            (
                chunks[0],
                WhisperTranscript {
                    language: Some("en".into()),
                    segments: vec![TranscriptSegment {
                        start_ms: 1_000,
                        end_ms: 10_000,
                        text: "First".into(),
                    }],
                },
            ),
            (
                chunks[1],
                WhisperTranscript {
                    language: Some("en".into()),
                    segments: vec![TranscriptSegment {
                        start_ms: 2_000,
                        end_ms: 8_000,
                        text: "Second".into(),
                    }],
                },
            ),
        ];
        let merged = merge_chunk_transcripts(120_000, &parts).unwrap();
        assert_eq!(merged.segments[0].start_ms, 1_000);
        assert_eq!(merged.segments[1].start_ms, 62_000);
        assert_eq!(merged.segments[1].end_ms, 68_000);
    }
}
