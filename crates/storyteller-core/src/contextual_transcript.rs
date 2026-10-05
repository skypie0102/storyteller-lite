use crate::{Transcript, TranscriptSegment, TranscriptionBackend, TranscriptionChunk};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_OWNED_WHISTLE_CHUNK_MS: u64 = 27_000;
pub const MAX_TRANSCRIPTION_CONTEXT_MS: u64 = 2_500;
const MATCH_DRIFT_MS: u64 = 1_000;
const MAX_BOUNDARY_WORDS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptionChunkPolicy {
    pub target_ms: u64,
    pub max_owned_ms: u64,
    pub max_input_ms: u64,
}

impl TranscriptionChunkPolicy {
    pub fn for_backend(backend: TranscriptionBackend) -> Self {
        match backend {
            TranscriptionBackend::Whistle => Self {
                target_ms: crate::DEFAULT_MAX_TRANSCRIPTION_CHUNK_MS,
                max_owned_ms: MAX_OWNED_WHISTLE_CHUNK_MS,
                max_input_ms: crate::WHISTLE_MAX_CHUNK_MS,
            },
            // Application memory/progress bounds; Whisper itself accepts longer files.
            TranscriptionBackend::WhisperCuda => Self {
                target_ms: 300_000,
                max_owned_ms: 302_500,
                max_input_ms: 307_500,
            },
        }
    }
}

pub fn plan_transcription_chunks_for_backend(
    duration_ms: u64,
    chapter_boundaries_ms: &[u64],
    backend: TranscriptionBackend,
) -> Result<Vec<TranscriptionChunk>, String> {
    let policy = TranscriptionChunkPolicy::for_backend(backend);
    crate::transcript::plan_transcription_chunks_with_limit(
        duration_ms,
        chapter_boundaries_ms,
        policy.target_ms,
        policy.max_owned_ms,
    )
}

pub fn validate_chunk_plan_for_backend(
    duration_ms: u64,
    chunks: &[TranscriptionChunk],
    backend: TranscriptionBackend,
) -> Result<(), String> {
    crate::transcript::validate_chunk_plan_with_limit(
        duration_ms,
        chunks,
        TranscriptionChunkPolicy::for_backend(backend).max_owned_ms,
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimedWord {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranscriptionWindow {
    pub owned: TranscriptionChunk,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl TranscriptionWindow {
    pub fn duration_ms(self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }
    pub fn input_chunk(self) -> TranscriptionChunk {
        TranscriptionChunk {
            index: self.owned.index,
            start_ms: self.start_ms,
            end_ms: self.end_ms,
        }
    }
}

pub fn contextual_transcription_windows(
    duration_ms: u64,
    chunks: &[TranscriptionChunk],
    backend: TranscriptionBackend,
) -> Result<Vec<TranscriptionWindow>, String> {
    validate_chunk_plan_for_backend(duration_ms, chunks, backend)?;
    let policy = TranscriptionChunkPolicy::for_backend(backend);
    chunks
        .iter()
        .map(|&owned| {
            if owned.duration_ms() > policy.max_owned_ms {
                return Err(
                    "Transcription ownership ranges must leave room for speech context.".into(),
                );
            }
            let context =
                ((policy.max_input_ms - owned.duration_ms()) / 2).min(MAX_TRANSCRIPTION_CONTEXT_MS);
            Ok(TranscriptionWindow {
                owned,
                start_ms: owned.start_ms.saturating_sub(context),
                end_ms: owned.end_ms.saturating_add(context).min(duration_ms),
            })
        })
        .collect()
}

pub(crate) fn text_identity(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub(crate) fn normalize_timed_words(
    words: &[TimedWord],
    language: Option<String>,
) -> Result<Transcript, String> {
    let mut groups = Vec::<(TranscriptSegment, usize)>::new();
    for word in words {
        let split = groups.last().is_some_and(|(current, count)| {
            current.end_ms > current.start_ms
                && word.start_ms >= current.end_ms
                && (*count >= 24
                    || word.start_ms.saturating_sub(current.end_ms) >= 700
                    || current
                        .text
                        .trim_end_matches(['\"', '\'', '”', '’', ')', ']'])
                        .ends_with(['.', '!', '?']))
        });
        if groups.is_empty() || split {
            groups.push((
                TranscriptSegment {
                    start_ms: word.start_ms,
                    end_ms: word.end_ms,
                    text: word.text.clone(),
                },
                1,
            ));
        } else {
            let (current, count) = groups.last_mut().expect("checked group");
            current.text.push(' ');
            current.text.push_str(&word.text);
            current.start_ms = current.start_ms.min(word.start_ms);
            current.end_ms = current.end_ms.max(word.end_ms);
            *count += 1;
        }
        // Fused attention times can overlap an earlier sentence. Preserve word
        // order and all text by merging the intervals, rather than trimming text.
        while groups.len() > 1
            && groups[groups.len() - 1].0.start_ms < groups[groups.len() - 2].0.end_ms
        {
            let (last, count) = groups.pop().expect("checked group");
            let (previous, previous_count) = groups.last_mut().expect("checked previous group");
            previous.text.push(' ');
            previous.text.push_str(&last.text);
            previous.start_ms = previous.start_ms.min(last.start_ms);
            previous.end_ms = previous.end_ms.max(last.end_ms);
            *previous_count += count;
        }
    }
    let transcript = Transcript {
        language,
        segments: groups.into_iter().map(|(s, _)| s).collect(),
    };
    crate::transcript::validate_transcript(&transcript, false)?;
    Ok(transcript)
}

#[derive(Clone)]
struct Candidate {
    window: usize,
    position: usize,
    word: TimedWord,
}

fn midpoint(word: &TimedWord) -> u64 {
    word.start_ms + (word.end_ms - word.start_ms) / 2
}

fn root(parents: &mut [usize], mut index: usize) -> usize {
    while parents[index] != index {
        parents[index] = parents[parents[index]];
        index = parents[index];
    }
    index
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Score {
    matches: usize,
    drift: u64,
}
impl Score {
    fn matched(self, drift: u64) -> Self {
        Self {
            matches: self.matches + 1,
            drift: self.drift + drift,
        }
    }
    fn better(self, other: Self) -> bool {
        self.matches > other.matches || (self.matches == other.matches && self.drift < other.drift)
    }
}

fn reconcile_pair(
    candidates: &[Candidate],
    left: &[usize],
    right: &[usize],
    parents: &mut [usize],
) -> Result<(), String> {
    if left.len() > MAX_BOUNDARY_WORDS || right.len() > MAX_BOUNDARY_WORDS {
        return Err(
            "Too many uncertain word times at an audio boundary to reconcile safely.".into(),
        );
    }
    let left_keys = left
        .iter()
        .map(|&i| text_identity(&candidates[i].word.text))
        .collect::<Vec<_>>();
    let right_keys = right
        .iter()
        .map(|&i| text_identity(&candidates[i].word.text))
        .collect::<Vec<_>>();
    let width = right.len() + 1;
    let mut scores = vec![Score::default(); (left.len() + 1) * width];
    let drift = |i: usize, j: usize| {
        midpoint(&candidates[left[i]].word).abs_diff(midpoint(&candidates[right[j]].word))
    };
    let matches = |i: usize, j: usize| {
        !left_keys[i].is_empty() && left_keys[i] == right_keys[j] && drift(i, j) <= MATCH_DRIFT_MS
    };
    for i in (0..left.len()).rev() {
        for j in (0..right.len()).rev() {
            let mut best = scores[(i + 1) * width + j];
            let skip_right = scores[i * width + j + 1];
            if skip_right.better(best) {
                best = skip_right;
            }
            if matches(i, j) {
                let take = scores[(i + 1) * width + j + 1].matched(drift(i, j));
                if take.better(best) {
                    best = take;
                }
            }
            scores[i * width + j] = best;
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        let current = scores[i * width + j];
        if matches(i, j) && scores[(i + 1) * width + j + 1].matched(drift(i, j)) == current {
            let a = root(parents, left[i]);
            let b = root(parents, right[j]);
            parents[b] = a;
            i += 1;
            j += 1;
        } else if scores[(i + 1) * width + j] == current {
            i += 1;
        } else {
            j += 1;
        }
    }
    Ok(())
}

pub fn merge_contextual_word_transcripts(
    duration_ms: u64,
    parts: &[(TranscriptionWindow, Vec<TimedWord>)],
    backend: TranscriptionBackend,
) -> Result<Transcript, String> {
    let chunks = parts
        .iter()
        .map(|(window, _)| window.owned)
        .collect::<Vec<_>>();
    let expected = contextual_transcription_windows(duration_ms, &chunks, backend)?;
    let mut candidates = Vec::new();
    let mut ranges = Vec::new();
    for (index, (window, words)) in parts.iter().enumerate() {
        if *window != expected[index] {
            return Err(
                "Transcription context does not match its validated ownership plan.".into(),
            );
        }
        let begin = candidates.len();
        let mut previous = 0;
        for (position, word) in words.iter().enumerate() {
            if word.text.trim().is_empty()
                || word.end_ms < word.start_ms
                || word.end_ms > window.duration_ms()
                || word.start_ms < previous
            {
                return Err("Transcription returned invalid or unordered word times.".into());
            }
            previous = word.start_ms;
            candidates.push(Candidate {
                window: index,
                position,
                word: TimedWord {
                    start_ms: window.start_ms + word.start_ms,
                    end_ms: window.start_ms + word.end_ms,
                    text: word.text.trim().into(),
                },
            });
        }
        ranges.push(begin..candidates.len());
    }
    let mut parents = (0..candidates.len()).collect::<Vec<_>>();
    for index in 0..parts.len().saturating_sub(1) {
        let start = parts[index]
            .0
            .start_ms
            .max(parts[index + 1].0.start_ms)
            .saturating_sub(MATCH_DRIFT_MS);
        let end = parts[index]
            .0
            .end_ms
            .min(parts[index + 1].0.end_ms)
            .saturating_add(MATCH_DRIFT_MS);
        let near = |range: std::ops::Range<usize>| {
            range
                .filter(|&i| (start..=end).contains(&midpoint(&candidates[i].word)))
                .collect::<Vec<_>>()
        };
        reconcile_pair(
            &candidates,
            &near(ranges[index].clone()),
            &near(ranges[index + 1].clone()),
            &mut parents,
        )?;
    }
    let mut groups = BTreeMap::<usize, Vec<usize>>::new();
    for index in 0..candidates.len() {
        groups
            .entry(root(&mut parents, index))
            .or_default()
            .push(index);
    }
    let mut owned_words = Vec::new();
    for group in groups.values() {
        let average = |end: bool| {
            (group
                .iter()
                .map(|&i| {
                    if end {
                        candidates[i].word.end_ms as u128
                    } else {
                        candidates[i].word.start_ms as u128
                    }
                })
                .sum::<u128>()
                / group.len() as u128) as u64
        };
        let mut fused = TimedWord {
            start_ms: average(false),
            end_ms: average(true),
            text: String::new(),
        };
        let point = midpoint(&fused).min(duration_ms - 1);
        let owner = chunks.partition_point(|chunk| point >= chunk.end_ms);
        // Only the owning input supplies text. Unmatched fragments from another
        // input's context cannot inject truncated edge words into the result.
        if let Some(&index) = group.iter().find(|&&i| candidates[i].window == owner) {
            fused.text = candidates[index].word.text.clone();
            owned_words.push((owner, candidates[index].position, fused));
        }
    }
    owned_words.sort_by_key(|(owner, position, _)| (*owner, *position));
    let words = owned_words
        .into_iter()
        .map(|(_, _, word)| word)
        .collect::<Vec<_>>();
    let transcript = normalize_timed_words(&words, Some("en".into()))?;
    crate::transcript::validate_transcript(&transcript, true)?;
    Ok(transcript)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn windows() -> Vec<TranscriptionWindow> {
        contextual_transcription_windows(
            50_000,
            &crate::plan_transcription_chunks(50_000, &[], 25_000).unwrap(),
            TranscriptionBackend::Whistle,
        )
        .unwrap()
    }
    fn word(start: u64, end: u64, text: &str) -> TimedWord {
        TimedWord {
            start_ms: start,
            end_ms: end,
            text: text.into(),
        }
    }
    fn text(transcript: &Transcript) -> String {
        transcript
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn context_covers_both_sides_without_exceeding_thirty_seconds_or_the_source() {
        let chunks = (0..3)
            .map(|index| TranscriptionChunk {
                index,
                start_ms: index as u64 * 27_000,
                end_ms: (index as u64 + 1) * 27_000,
            })
            .collect::<Vec<_>>();
        let windows =
            contextual_transcription_windows(81_000, &chunks, TranscriptionBackend::Whistle)
                .unwrap();
        assert_eq!(windows[0].start_ms, 0);
        assert_eq!(windows[2].end_ms, 81_000);
        assert_eq!(windows[1].duration_ms(), 30_000);
        assert_eq!(windows[1].start_ms, 25_500);
        assert_eq!(windows[1].end_ms, 55_500);
        let too_large = [TranscriptionChunk {
            index: 0,
            start_ms: 0,
            end_ms: 30_000,
        }];
        assert!(contextual_transcription_windows(
            30_000,
            &too_large,
            TranscriptionBackend::Whistle
        )
        .is_err());
    }
    #[test]
    fn whisper_uses_five_minute_ownership_instead_of_whistles_file_limit() {
        let backend = TranscriptionBackend::WhisperCuda;
        let chunks = plan_transcription_chunks_for_backend(3_600_001, &[], backend).unwrap();
        assert_eq!(chunks.len(), 13);
        assert_eq!(chunks[0].duration_ms(), 300_000);
        let windows = contextual_transcription_windows(3_600_001, &chunks, backend).unwrap();
        assert_eq!(windows[1].duration_ms(), 305_000);
        assert!(windows.iter().all(|w| w.duration_ms() <= 307_500));
        assert_eq!(windows.last().unwrap().end_ms, 3_600_001);
        assert!(
            validate_chunk_plan_for_backend(3_600_001, &chunks, TranscriptionBackend::Whistle)
                .is_err()
        );
        let parts = windows
            .into_iter()
            .enumerate()
            .map(|(index, window)| {
                let start = window.owned.start_ms - window.start_ms + 100;
                (
                    window,
                    vec![word(start, start + 200, &format!("Word{index}."))],
                )
            })
            .collect::<Vec<_>>();
        // The last 1 ms tail cannot contain the synthetic 200 ms word.
        let mut parts = parts;
        parts.last_mut().unwrap().1.clear();
        let merged = merge_contextual_word_transcripts(3_600_001, &parts, backend).unwrap();
        assert!(merged.segments.last().unwrap().start_ms > 3_000_000);
    }

    #[test]
    fn joint_timing_recovers_a_word_that_both_independent_owners_would_discard() {
        let w = windows();
        let merged = merge_contextual_word_transcripts(
            50_000,
            &[
                (w[0], vec![word(24_900, 25_300, "Lighthouse")]),
                (w[1], vec![word(2_350, 2_550, "Lighthouse.")]),
            ],
            TranscriptionBackend::Whistle,
        )
        .unwrap();
        assert_eq!(text(&merged), "Lighthouse.");
        assert_eq!(merged.segments[0].start_ms, 24_875);
        assert_eq!(merged.segments[0].end_ms, 25_175);
    }
    #[test]
    fn intentional_repetition_is_matched_one_to_one_in_time() {
        let w = windows();
        let merged = merge_contextual_word_transcripts(
            50_000,
            &[
                (
                    w[0],
                    vec![word(24_400, 24_700, "very"), word(25_100, 25_500, "very")],
                ),
                (
                    w[1],
                    vec![word(1_925, 2_225, "very"), word(2_625, 3_025, "very")],
                ),
            ],
            TranscriptionBackend::Whistle,
        )
        .unwrap();
        assert_eq!(text(&merged), "very very");
    }
    #[test]
    fn equal_words_at_separate_times_and_partial_halo_words_are_not_conflated() {
        let w = windows();
        let merged = merge_contextual_word_transcripts(
            50_000,
            &[
                (
                    w[0],
                    vec![word(24_000, 24_200, "again"), word(26_400, 26_600, "ligh")],
                ),
                (
                    w[1],
                    vec![
                        word(3_500, 3_700, "again"),
                        word(3_900, 4_100, "lighthouse"),
                    ],
                ),
            ],
            TranscriptionBackend::Whistle,
        )
        .unwrap();
        assert_eq!(text(&merged), "again again lighthouse");
    }
    #[test]
    fn small_ownership_ranges_can_share_context_with_several_neighbors() {
        let chunks = (0..4)
            .map(|index| TranscriptionChunk {
                index,
                start_ms: index as u64 * 500,
                end_ms: (index as u64 + 1) * 500,
            })
            .collect::<Vec<_>>();
        let windows =
            contextual_transcription_windows(2_000, &chunks, TranscriptionBackend::Whistle)
                .unwrap();
        let parts = windows
            .into_iter()
            .map(|w| (w, vec![word(900, 1100, "Hello.")]))
            .collect::<Vec<_>>();
        assert_eq!(
            text(
                &merge_contextual_word_transcripts(2_000, &parts, TranscriptionBackend::Whistle)
                    .unwrap()
            ),
            "Hello."
        );
    }
    #[test]
    fn silence_bad_times_and_unbounded_boundary_matching_fail_explicitly() {
        let w = windows();
        assert!(merge_contextual_word_transcripts(
            50_000,
            &[(w[0], vec![]), (w[1], vec![])],
            TranscriptionBackend::Whistle
        )
        .is_err());
        assert!(merge_contextual_word_transcripts(
            50_000,
            &[(w[0], vec![word(100, 99, "bad")]), (w[1], vec![])],
            TranscriptionBackend::Whistle
        )
        .is_err());
        let many = vec![word(24_000, 24_100, "repeat"); 257];
        let many_right = vec![word(1_500, 1_600, "repeat"); 257];
        assert!(merge_contextual_word_transcripts(
            50_000,
            &[(w[0], many), (w[1], many_right)],
            TranscriptionBackend::Whistle
        )
        .unwrap_err()
        .contains("Too many"));
    }
}
