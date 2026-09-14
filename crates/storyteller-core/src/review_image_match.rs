use crate::{
    review_image_text_evidence, AudioReviewImageCandidate, AudioReviewImageEvidenceSource,
    AudioReviewImageTextEvidence, CancellationToken,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};

const MIN_EVIDENCE_WORDS: usize = 5;
const MAX_EVIDENCE_WORDS: usize = 120;
const SHORT_EVIDENCE_WORDS: usize = 12;
const MIN_COVERAGE_MILLI: u16 = 360;
const MIN_SHORT_SCORE_MILLI: u16 = 770;
const MIN_LONG_SCORE_MILLI: u16 = 690;
const MIN_DISTINCTIVE_MATCHES: usize = 2;
const MIN_WINNER_MARGIN_MILLI: u16 = 80;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioReviewImageMatchCandidate {
    pub candidate: AudioReviewImageCandidate,
    pub evidence: AudioReviewImageTextEvidence,
    /// Fraction of distinct evidence words also present in the transcript, in 0..=1000.
    pub coverage_milli: u16,
    /// Overlap coefficient between distinct evidence and transcript words, in 0..=1000.
    pub similarity_milli: u16,
    /// Deterministic aggregate score, in 0..=1000.
    pub score_milli: u16,
    pub distinctive_matches: usize,
    pub evidence_word_count: usize,
    pub qualifies: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioReviewImageMatchResult {
    pub ranked: Vec<AudioReviewImageMatchCandidate>,
    /// Index into `ranked` only when one candidate is both strong and unambiguous.
    pub recommended_index: Option<usize>,
}

impl AudioReviewImageMatchResult {
    pub fn recommended(&self) -> Option<&AudioReviewImageMatchCandidate> {
        self.recommended_index
            .and_then(|index| self.ranked.get(index))
    }
}

/// Scores already-bounded EPUB image candidates against one unmatched transcript segment.
///
/// Embedded EPUB text is evaluated first. If it yields a strong, unambiguous winner, OCR is not
/// attempted. Only when embedded evidence cannot do that are candidates without embedded text
/// eligible for the optional Tesseract fallback.
pub fn review_image_matches(
    epub_path: &Path,
    transcript_text: &str,
    candidates: &[AudioReviewImageCandidate],
    tesseract_executable: Option<&Path>,
    cancellation: &CancellationToken,
) -> Result<AudioReviewImageMatchResult, String> {
    if cancellation.is_requested() {
        return Err("EPUB image evidence scoring was cancelled.".into());
    }

    let mut scored = Vec::new();
    for candidate in candidates
        .iter()
        .filter(|candidate| !candidate.embedded_text.is_empty())
    {
        if cancellation.is_requested() {
            return Err("EPUB image evidence scoring was cancelled.".into());
        }
        if let Some(evidence) =
            review_image_text_evidence(epub_path, candidate, None, cancellation)?
        {
            if let Some(candidate) = score_candidate(transcript_text, candidate, evidence) {
                scored.push(candidate);
            }
        }
    }

    rank_candidates(&mut scored);
    let embedded_result = result_from_ranked(scored.clone());
    if embedded_result.recommended_index.is_some() || tesseract_executable.is_none() {
        return Ok(embedded_result);
    }

    let tesseract_executable = tesseract_executable.expect("checked above");
    for candidate in candidates
        .iter()
        .filter(|candidate| candidate.embedded_text.is_empty())
    {
        if cancellation.is_requested() {
            return Err("EPUB image evidence scoring was cancelled.".into());
        }
        if let Some(evidence) = review_image_text_evidence(
            epub_path,
            candidate,
            Some(tesseract_executable),
            cancellation,
        )? {
            if let Some(candidate) = score_candidate(transcript_text, candidate, evidence) {
                scored.push(candidate);
            }
        }
    }

    rank_candidates(&mut scored);
    Ok(result_from_ranked(scored))
}

fn score_candidate(
    transcript_text: &str,
    candidate: &AudioReviewImageCandidate,
    evidence: AudioReviewImageTextEvidence,
) -> Option<AudioReviewImageMatchCandidate> {
    let transcript_words = normalized_words(transcript_text);
    let evidence_words = normalized_words(&evidence.lines.join(" "));
    let evidence_word_count = evidence_words.len();
    if transcript_words.is_empty()
        || !(MIN_EVIDENCE_WORDS..=MAX_EVIDENCE_WORDS).contains(&evidence_word_count)
    {
        return None;
    }

    let matches = evidence_words.intersection(&transcript_words).count();
    let coverage_milli = ratio_milli(matches, evidence_words.len());
    let similarity_milli = ratio_milli(matches, evidence_words.len().min(transcript_words.len()));
    let score_milli = ((u32::from(coverage_milli) * 2 + u32::from(similarity_milli)) / 3) as u16;
    let distinctive_matches = evidence_words
        .intersection(&transcript_words)
        .filter(|word| is_distinctive(word))
        .count();
    let minimum_score = if evidence_word_count <= SHORT_EVIDENCE_WORDS {
        MIN_SHORT_SCORE_MILLI
    } else {
        MIN_LONG_SCORE_MILLI
    };
    let qualifies = coverage_milli >= MIN_COVERAGE_MILLI
        && score_milli >= minimum_score
        && distinctive_matches >= MIN_DISTINCTIVE_MATCHES;

    Some(AudioReviewImageMatchCandidate {
        candidate: candidate.clone(),
        evidence,
        coverage_milli,
        similarity_milli,
        score_milli,
        distinctive_matches,
        evidence_word_count,
        qualifies,
    })
}

fn result_from_ranked(ranked: Vec<AudioReviewImageMatchCandidate>) -> AudioReviewImageMatchResult {
    let recommended_index = ranked.first().and_then(|winner| {
        if !winner.qualifies {
            return None;
        }
        let runner_up = ranked.iter().skip(1).find(|candidate| candidate.qualifies);
        if runner_up.is_some_and(|candidate| {
            winner.score_milli.saturating_sub(candidate.score_milli) < MIN_WINNER_MARGIN_MILLI
        }) {
            return None;
        }
        Some(0)
    });
    AudioReviewImageMatchResult {
        ranked,
        recommended_index,
    }
}

fn rank_candidates(candidates: &mut [AudioReviewImageMatchCandidate]) {
    candidates.sort_by(|left, right| {
        right
            .qualifies
            .cmp(&left.qualifies)
            .then_with(|| right.score_milli.cmp(&left.score_milli))
            .then_with(|| right.coverage_milli.cmp(&left.coverage_milli))
            .then_with(|| {
                evidence_source_rank(left.evidence.source)
                    .cmp(&evidence_source_rank(right.evidence.source))
            })
            .then_with(|| {
                left.candidate
                    .document_spine_index
                    .cmp(&right.candidate.document_spine_index)
            })
            .then_with(|| left.candidate.image_ordinal.cmp(&right.candidate.image_ordinal))
            .then_with(|| left.candidate.image_href.cmp(&right.candidate.image_href))
    });
}

const fn evidence_source_rank(source: AudioReviewImageEvidenceSource) -> u8 {
    match source {
        AudioReviewImageEvidenceSource::Embedded => 0,
        AudioReviewImageEvidenceSource::Tesseract => 1,
    }
}

fn ratio_milli(numerator: usize, denominator: usize) -> u16 {
    if denominator == 0 {
        return 0;
    }
    ((numerator.saturating_mul(1000) / denominator).min(1000)) as u16
}

fn normalized_words(text: &str) -> BTreeSet<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter_map(|word| {
            let normalized = word.trim().to_lowercase();
            (!normalized.is_empty()).then_some(normalized)
        })
        .collect()
}

fn is_distinctive(word: &str) -> bool {
    if word.chars().count() < 4 {
        return false;
    }
    !matches!(
        word,
        "about"
            | "after"
            | "again"
            | "also"
            | "been"
            | "before"
            | "being"
            | "could"
            | "from"
            | "have"
            | "into"
            | "just"
            | "more"
            | "most"
            | "other"
            | "over"
            | "some"
            | "such"
            | "than"
            | "that"
            | "their"
            | "them"
            | "then"
            | "there"
            | "these"
            | "they"
            | "this"
            | "those"
            | "through"
            | "very"
            | "were"
            | "what"
            | "when"
            | "where"
            | "which"
            | "while"
            | "with"
            | "would"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(
        document_spine_index: usize,
        image_ordinal: usize,
        image_href: &str,
        hints: &[&str],
    ) -> AudioReviewImageCandidate {
        AudioReviewImageCandidate {
            document_href: format!("OPS/Text/{document_spine_index}.xhtml"),
            image_href: image_href.into(),
            media_type: "image/png".into(),
            byte_size: 12,
            document_spine_index,
            image_ordinal,
            embedded_text: hints.iter().map(|value| (*value).to_string()).collect(),
        }
    }

    fn embedded(lines: &[&str]) -> AudioReviewImageTextEvidence {
        AudioReviewImageTextEvidence {
            source: AudioReviewImageEvidenceSource::Embedded,
            lines: lines.iter().map(|value| (*value).to_string()).collect(),
            confidence_percent: None,
        }
    }

    #[test]
    fn strong_embedded_match_is_recommended_without_opening_epub_or_ocr() {
        let candidates = vec![
            candidate(
                3,
                0,
                "OPS/Images/tree.png",
                &["Family tree showing Alice Robert Clara and Daniel"],
            ),
            candidate(4, 0, "OPS/Images/no-hints.png", &[]),
        ];
        let result = review_image_matches(
            Path::new("does-not-need-to-exist.epub"),
            "The family tree showing Alice Robert Clara and Daniel is read aloud here.",
            &candidates,
            Some(Path::new("definitely-missing-tesseract")),
            &CancellationToken::default(),
        )
        .unwrap();

        let recommended = result.recommended().unwrap();
        assert_eq!(recommended.candidate.image_href, "OPS/Images/tree.png");
        assert_eq!(
            recommended.evidence.source,
            AudioReviewImageEvidenceSource::Embedded
        );
        assert!(recommended.qualifies);
    }

    #[test]
    fn ambiguous_strong_embedded_matches_are_not_recommended() {
        let candidates = vec![
            candidate(
                3,
                0,
                "OPS/Images/tree-a.png",
                &["Family tree showing Alice Robert Clara and Daniel"],
            ),
            candidate(
                4,
                0,
                "OPS/Images/tree-b.png",
                &["Family tree showing Alice Robert Clara and Daniel"],
            ),
        ];
        let result = review_image_matches(
            Path::new("does-not-need-to-exist.epub"),
            "Family tree showing Alice Robert Clara and Daniel",
            &candidates,
            None,
            &CancellationToken::default(),
        )
        .unwrap();

        assert_eq!(result.ranked.len(), 2);
        assert!(result.ranked.iter().all(|candidate| candidate.qualifies));
        assert!(result.recommended().is_none());
    }

    #[test]
    fn weak_or_generic_overlap_stays_unqualified() {
        let scored = score_candidate(
            "This is where the other people were before they went there.",
            &candidate(
                1,
                0,
                "OPS/Images/generic.png",
                &["This is where the other people were before"],
            ),
            embedded(&["This is where the other people were before"]),
        )
        .unwrap();

        assert!(!scored.qualifies);
        assert!(scored.distinctive_matches < MIN_DISTINCTIVE_MATCHES);
    }

    #[test]
    fn evidence_outside_phrase_bounds_is_ignored() {
        assert!(score_candidate(
            "Alpha beta gamma delta",
            &candidate(1, 0, "OPS/Images/short.png", &["Alpha beta"]),
            embedded(&["Alpha beta"]),
        )
        .is_none());
    }

    #[test]
    fn embedded_source_wins_deterministic_tie() {
        let source = candidate(2, 0, "OPS/Images/tie.png", &[]);
        let mut ranked = vec![
            score_candidate(
                "Alpha bravo charlie delta echo foxtrot",
                &source,
                AudioReviewImageTextEvidence {
                    source: AudioReviewImageEvidenceSource::Tesseract,
                    lines: vec!["Alpha bravo charlie delta echo foxtrot".into()],
                    confidence_percent: Some(90),
                },
            )
            .unwrap(),
            score_candidate(
                "Alpha bravo charlie delta echo foxtrot",
                &source,
                embedded(&["Alpha bravo charlie delta echo foxtrot"]),
            )
            .unwrap(),
        ];
        rank_candidates(&mut ranked);

        assert_eq!(ranked[0].evidence.source, AudioReviewImageEvidenceSource::Embedded);
    }
}
