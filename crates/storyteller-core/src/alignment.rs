use crate::{
    read_epub_corpus, read_whisper_transcript, CancellationToken, EpubCorpus, WhisperTranscript,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    fs,
    hash::{Hash, Hasher},
    path::Path,
};

const MAX_SEARCH_AHEAD_TOKENS: usize = 6000;
const MAX_BACKTRACK_TOKENS: usize = 12;
const MAX_CANDIDATES: usize = 12;
const CANDIDATE_START_FUZZ: usize = 3;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignmentDocument {
    pub algorithm: String,
    pub language: Option<String>,
    pub total_segments: usize,
    pub matched_segments: usize,
    pub match_percent: f64,
    pub segments: Vec<AlignmentSegment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignmentSegment {
    pub audio_start_ms: u64,
    pub audio_end_ms: u64,
    pub transcript_text: String,
    pub status: AlignmentStatus,
    pub match_percent: Option<f64>,
    pub book_start: Option<CorpusPosition>,
    pub book_end: Option<CorpusPosition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlignmentStatus {
    Matched,
    Unmatched,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorpusPosition {
    pub href: String,
    pub line_index: usize,
    pub char_offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AlignmentSummary {
    pub total_segments: usize,
    pub matched_segments: usize,
    pub match_percent: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AlignmentProgress {
    pub processed_segments: usize,
    pub total_segments: usize,
    pub match_percent: f64,
}

pub fn align_transcript_to_corpus(
    corpus_path: &Path,
    transcript_path: &Path,
    destination: &Path,
    cancellation: &CancellationToken,
    observer: &mut dyn FnMut(AlignmentProgress) -> Result<(), String>,
) -> Result<AlignmentSummary, String> {
    let corpus = read_epub_corpus(corpus_path)?;
    let transcript = read_whisper_transcript(transcript_path)?;
    let document = align_loaded(&corpus, &transcript, cancellation, observer)?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Could not create alignment destination {}: {error}",
                parent.display()
            )
        })?;
    }
    let json = serde_json::to_vec_pretty(&document)
        .map_err(|error| format!("Could not serialize alignment map: {error}"))?;
    fs::write(destination, json).map_err(|error| {
        format!(
            "Could not write alignment map {}: {error}",
            destination.display()
        )
    })?;
    Ok(AlignmentSummary {
        total_segments: document.total_segments,
        matched_segments: document.matched_segments,
        match_percent: document.match_percent,
    })
}

fn align_loaded(
    corpus: &EpubCorpus,
    transcript: &WhisperTranscript,
    cancellation: &CancellationToken,
    observer: &mut dyn FnMut(AlignmentProgress) -> Result<(), String>,
) -> Result<AlignmentDocument, String> {
    let book_tokens = tokenize_corpus(corpus, cancellation)?;
    if book_tokens.is_empty() {
        return Err("EPUB corpus has no alignable word tokens.".into());
    }
    let indexes = NgramIndexes::build(&book_tokens, cancellation)?;
    let mut cursor = 0usize;
    let mut segments = Vec::with_capacity(transcript.segments.len());
    let mut matched_segments = 0usize;
    let mut weighted_match = 0.0f64;
    let mut weighted_tokens = 0usize;

    for (segment_index, segment) in transcript.segments.iter().enumerate() {
        if cancellation.is_requested() {
            return Err("Transcript alignment was cancelled.".into());
        }
        let transcript_tokens = tokenize_text(&segment.text);
        let token_weight = transcript_tokens.len();
        let matched = find_segment_match(&book_tokens, &indexes, &transcript_tokens, cursor);
        if let Some(found) = matched {
            cursor = cursor.max(found.end_token);
            matched_segments += 1;
            weighted_match += found.score * token_weight as f64;
            weighted_tokens += token_weight;
            let first = &book_tokens[found.start_token];
            let last = &book_tokens[found.end_token - 1];
            segments.push(AlignmentSegment {
                audio_start_ms: segment.start_ms,
                audio_end_ms: segment.end_ms,
                transcript_text: segment.text.clone(),
                status: AlignmentStatus::Matched,
                match_percent: Some(found.score * 100.0),
                book_start: Some(CorpusPosition {
                    href: corpus.sections[first.section_index].href.clone(),
                    line_index: first.line_index,
                    char_offset: first.start_char,
                }),
                book_end: Some(CorpusPosition {
                    href: corpus.sections[last.section_index].href.clone(),
                    line_index: last.line_index,
                    char_offset: last.end_char,
                }),
            });
        } else {
            weighted_tokens += token_weight;
            segments.push(AlignmentSegment {
                audio_start_ms: segment.start_ms,
                audio_end_ms: segment.end_ms,
                transcript_text: segment.text.clone(),
                status: AlignmentStatus::Unmatched,
                match_percent: None,
                book_start: None,
                book_end: None,
            });
        }

        let match_percent = percent(weighted_match, weighted_tokens);
        observer(AlignmentProgress {
            processed_segments: segment_index + 1,
            total_segments: transcript.segments.len(),
            match_percent,
        })?;
    }

    Ok(AlignmentDocument {
        algorithm: "monotonic-ngram-edit-v2-block-safe".into(),
        language: transcript.language.clone(),
        total_segments: transcript.segments.len(),
        matched_segments,
        match_percent: percent(weighted_match, weighted_tokens),
        segments,
    })
}

#[derive(Debug, Clone)]
struct BookToken {
    normalized: String,
    section_index: usize,
    line_index: usize,
    start_char: usize,
    end_char: usize,
}

fn tokenize_corpus(
    corpus: &EpubCorpus,
    cancellation: &CancellationToken,
) -> Result<Vec<BookToken>, String> {
    let mut tokens = Vec::new();
    for (section_index, section) in corpus.sections.iter().enumerate() {
        if cancellation.is_requested() {
            return Err("Transcript alignment was cancelled.".into());
        }
        tokens.extend(tokenize_section(&section.text, section_index));
    }
    Ok(tokens)
}

fn tokenize_section(text: &str, section_index: usize) -> Vec<BookToken> {
    let mut tokens = Vec::new();
    let mut base_char = 0usize;
    for (line_index, line) in text.split('\n').enumerate() {
        tokens.extend(tokenize_fragment(
            line,
            section_index,
            line_index,
            base_char,
        ));
        base_char = base_char.saturating_add(line.chars().count());
        if base_char < text.chars().count() {
            base_char = base_char.saturating_add(1);
        }
    }
    tokens
}

fn tokenize_fragment(
    text: &str,
    section_index: usize,
    line_index: usize,
    base_char: usize,
) -> Vec<BookToken> {
    let mut tokens = Vec::new();
    let mut normalized = String::new();
    let mut start_char = None;
    let mut end_char = 0usize;

    for (char_offset, character) in text.chars().enumerate() {
        if character.is_alphanumeric() {
            start_char.get_or_insert(char_offset);
            for lowercase in character.to_lowercase() {
                normalized.push(lowercase);
            }
            end_char = char_offset + 1;
        } else if is_word_joiner(character) && !normalized.is_empty() {
            continue;
        } else if let Some(start_char) = start_char.take() {
            tokens.push(BookToken {
                normalized: std::mem::take(&mut normalized),
                section_index,
                line_index,
                start_char: base_char + start_char,
                end_char: base_char + end_char,
            });
        }
    }
    if let Some(start_char) = start_char {
        tokens.push(BookToken {
            normalized,
            section_index,
            line_index,
            start_char: base_char + start_char,
            end_char: base_char + end_char,
        });
    }
    tokens
}

fn tokenize_text(text: &str) -> Vec<String> {
    tokenize_fragment(text, 0, 0, 0)
        .into_iter()
        .map(|token| token.normalized)
        .collect()
}

fn is_word_joiner(character: char) -> bool {
    matches!(character, '\'' | '’' | '-')
}

#[derive(Debug)]
struct NgramIndexes {
    unigram: HashMap<u64, Vec<usize>>,
    bigram: HashMap<u64, Vec<usize>>,
    trigram: HashMap<u64, Vec<usize>>,
}

impl NgramIndexes {
    fn build(tokens: &[BookToken], cancellation: &CancellationToken) -> Result<Self, String> {
        let mut indexes = Self {
            unigram: HashMap::new(),
            bigram: HashMap::new(),
            trigram: HashMap::new(),
        };
        for index in 0..tokens.len() {
            if index % 4096 == 0 && cancellation.is_requested() {
                return Err("Transcript alignment was cancelled.".into());
            }
            indexes
                .unigram
                .entry(hash_ngram_book(tokens, index, 1))
                .or_default()
                .push(index);
            if same_block_window(tokens, index, 2) {
                indexes
                    .bigram
                    .entry(hash_ngram_book(tokens, index, 2))
                    .or_default()
                    .push(index);
            }
            if same_block_window(tokens, index, 3) {
                indexes
                    .trigram
                    .entry(hash_ngram_book(tokens, index, 3))
                    .or_default()
                    .push(index);
            }
        }
        Ok(indexes)
    }

    fn get(&self, size: usize, hash: u64) -> Option<&[usize]> {
        match size {
            1 => self.unigram.get(&hash),
            2 => self.bigram.get(&hash),
            3 => self.trigram.get(&hash),
            _ => None,
        }
        .map(Vec::as_slice)
    }
}

fn same_block_window(tokens: &[BookToken], start: usize, size: usize) -> bool {
    let Some(end) = start.checked_add(size) else {
        return false;
    };
    if end > tokens.len() {
        return false;
    }
    let first = &tokens[start];
    let last = &tokens[end - 1];
    first.section_index == last.section_index && first.line_index == last.line_index
}

#[derive(Debug, Clone, Copy)]
struct SegmentMatch {
    start_token: usize,
    end_token: usize,
    score: f64,
}

fn find_segment_match(
    book: &[BookToken],
    indexes: &NgramIndexes,
    transcript: &[String],
    cursor: usize,
) -> Option<SegmentMatch> {
    if transcript.len() < 2 || book.is_empty() {
        return None;
    }

    let min_position = cursor.saturating_sub(MAX_BACKTRACK_TOKENS);
    let max_position = cursor
        .saturating_add(MAX_SEARCH_AHEAD_TOKENS)
        .min(book.len().saturating_sub(1));
    let mut votes =
        collect_candidate_votes(book, indexes, transcript, 3, min_position, max_position);
    if votes.is_empty() {
        votes = collect_candidate_votes(book, indexes, transcript, 2, min_position, max_position);
    }
    if votes.is_empty() && transcript.len() == 2 {
        votes = collect_candidate_votes(book, indexes, transcript, 1, min_position, max_position);
    }
    if votes.is_empty() {
        return None;
    }

    let mut ranked = votes.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|(left_start, left_votes), (right_start, right_votes)| {
        right_votes.cmp(left_votes).then_with(|| {
            left_start
                .abs_diff(cursor)
                .cmp(&right_start.abs_diff(cursor))
        })
    });
    ranked.truncate(MAX_CANDIDATES);

    let mut best = None;
    for (candidate, _) in ranked {
        let lower = candidate
            .saturating_sub(CANDIDATE_START_FUZZ)
            .max(min_position);
        let upper = candidate
            .saturating_add(CANDIDATE_START_FUZZ)
            .min(max_position);
        for start in lower..=upper {
            let Some(candidate_match) = best_flexible_window(book, transcript, start) else {
                continue;
            };
            if candidate_match.start_token < min_position
                || candidate_match.start_token > max_position
            {
                continue;
            }
            if best.as_ref().map_or(true, |current: &SegmentMatch| {
                candidate_match.score > current.score
            }) {
                best = Some(candidate_match);
            }
        }
    }

    let best = best?;
    if best.score >= acceptance_threshold(transcript.len()) {
        Some(best)
    } else {
        None
    }
}

fn collect_candidate_votes(
    book: &[BookToken],
    indexes: &NgramIndexes,
    transcript: &[String],
    ngram_size: usize,
    min_position: usize,
    max_position: usize,
) -> HashMap<usize, usize> {
    if transcript.len() < ngram_size {
        return HashMap::new();
    }
    let mut votes = HashMap::new();
    for transcript_offset in 0..=transcript.len() - ngram_size {
        let hash = hash_ngram_text(transcript, transcript_offset, ngram_size);
        let Some(positions) = indexes.get(ngram_size, hash) else {
            continue;
        };
        for &book_position in positions {
            if book_position < transcript_offset {
                continue;
            }
            let candidate_start = book_position - transcript_offset;
            if candidate_start < min_position || candidate_start > max_position {
                continue;
            }
            if !ngram_equals(
                book,
                book_position,
                transcript,
                transcript_offset,
                ngram_size,
            ) {
                continue;
            }
            *votes.entry(candidate_start).or_insert(0) += 1;
        }
    }
    votes
}

fn best_flexible_window(
    book: &[BookToken],
    transcript: &[String],
    start: usize,
) -> Option<SegmentMatch> {
    if start >= book.len() || transcript.is_empty() {
        return None;
    }
    let variation = (transcript.len() / 4).clamp(1, 8);
    let minimum = transcript.len().saturating_sub(variation).max(1);
    let maximum = transcript.len().saturating_add(variation);
    let mut best = None;
    for length in minimum..=maximum {
        let end = start.saturating_add(length);
        if end > book.len() {
            break;
        }
        if !same_block_window(book, start, length) {
            continue;
        }
        let score = token_similarity(book, start, end, transcript);
        let candidate = SegmentMatch {
            start_token: start,
            end_token: end,
            score,
        };
        if best.as_ref().map_or(true, |current: &SegmentMatch| {
            candidate.score > current.score
        }) {
            best = Some(candidate);
        }
    }
    best
}

fn token_similarity(book: &[BookToken], start: usize, end: usize, transcript: &[String]) -> f64 {
    let book_slice = &book[start..end];
    let distance = levenshtein_distance(book_slice, transcript);
    let denominator = book_slice.len().max(transcript.len());
    if denominator == 0 {
        0.0
    } else {
        1.0 - distance as f64 / denominator as f64
    }
}

fn levenshtein_distance(book: &[BookToken], transcript: &[String]) -> usize {
    let mut previous = (0..=transcript.len()).collect::<Vec<_>>();
    let mut current = vec![0usize; transcript.len() + 1];
    for (book_index, book_token) in book.iter().enumerate() {
        current[0] = book_index + 1;
        for (transcript_index, transcript_token) in transcript.iter().enumerate() {
            let substitution = previous[transcript_index]
                + if book_token.normalized != *transcript_token {
                    1
                } else {
                    0
                };
            let insertion = current[transcript_index] + 1;
            let deletion = previous[transcript_index + 1] + 1;
            current[transcript_index + 1] = substitution.min(insertion.min(deletion));
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[transcript.len()]
}

fn acceptance_threshold(token_count: usize) -> f64 {
    match token_count {
        0 | 1 => 1.0,
        2 | 3 => 0.90,
        4..=7 => 0.72,
        _ => 0.60,
    }
}

fn ngram_equals(
    book: &[BookToken],
    book_start: usize,
    transcript: &[String],
    transcript_start: usize,
    size: usize,
) -> bool {
    (0..size).all(|offset| {
        book.get(book_start + offset)
            .zip(transcript.get(transcript_start + offset))
            .is_some_and(|(left, right)| left.normalized == *right)
    })
}

fn hash_ngram_book(tokens: &[BookToken], start: usize, size: usize) -> u64 {
    let mut hasher = DefaultHasher::new();
    for token in tokens.iter().skip(start).take(size) {
        token.normalized.hash(&mut hasher);
    }
    hasher.finish()
}

fn hash_ngram_text(tokens: &[String], start: usize, size: usize) -> u64 {
    let mut hasher = DefaultHasher::new();
    for token in tokens.iter().skip(start).take(size) {
        token.hash(&mut hasher);
    }
    hasher.finish()
}

fn percent(weighted_match: f64, weighted_tokens: usize) -> f64 {
    if weighted_tokens == 0 {
        0.0
    } else {
        (weighted_match / weighted_tokens as f64 * 100.0).clamp(0.0, 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EpubSection, TranscriptSegment};

    fn corpus(text: &str) -> EpubCorpus {
        EpubCorpus {
            package_path: "OPS/package.opf".into(),
            sections: vec![EpubSection {
                href: "OPS/chapter.xhtml".into(),
                text: text.into(),
            }],
        }
    }

    fn transcript(segments: &[&str]) -> WhisperTranscript {
        WhisperTranscript {
            language: Some("en".into()),
            segments: segments
                .iter()
                .enumerate()
                .map(|(index, text)| TranscriptSegment {
                    start_ms: index as u64 * 1000,
                    end_ms: (index as u64 + 1) * 1000,
                    text: (*text).into(),
                })
                .collect(),
        }
    }

    #[test]
    fn aligns_monotonically_with_small_asr_errors() {
        let book =
            corpus("The quick brown fox jumps over the lazy dog. A second sentence appears here.");
        let spoken = transcript(&[
            "The quick brown fox jumps over a lazy dog.",
            "A second sentence appears here.",
        ]);
        let mut observed = Vec::new();
        let result = align_loaded(
            &book,
            &spoken,
            &CancellationToken::default(),
            &mut |progress| {
                observed.push(progress);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(result.matched_segments, 2);
        assert!(result.match_percent > 80.0);
        assert!(result.segments.iter().all(|segment| {
            segment.status == AlignmentStatus::Matched && segment.book_start.is_some()
        }));
        assert_eq!(observed.len(), 2);
    }

    #[test]
    fn weak_segment_is_left_unmatched_instead_of_forced() {
        let book = corpus("This book contains ordinary prose and nothing about rockets.");
        let spoken = transcript(&["quantum banana orchestra travels sideways"]);
        let result = align_loaded(&book, &spoken, &CancellationToken::default(), &mut |_| {
            Ok(())
        })
        .unwrap();
        assert_eq!(result.matched_segments, 0);
        assert_eq!(result.segments[0].status, AlignmentStatus::Unmatched);
        assert!(result.segments[0].book_start.is_none());
    }

    #[test]
    fn match_cannot_cross_corpus_block_boundary() {
        let book = corpus("first paragraph words\nsecond paragraph words");
        let spoken = transcript(&["paragraph words second paragraph"]);
        let result = align_loaded(&book, &spoken, &CancellationToken::default(), &mut |_| {
            Ok(())
        })
        .unwrap();
        assert_eq!(result.matched_segments, 0);
    }
}
