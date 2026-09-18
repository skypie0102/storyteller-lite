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
/// attempted. Only when embedded evidence cannot do that are candidates without usable embedded
/// text eligible for the optional Tesseract fallback.
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

    let transcript_words = normalized_words(transcript_text);
    if transcript_words.is_empty() {
        return Ok(result_from_ranked(Vec::new()));
    }

    let mut scored = Vec::new();
    let mut ocr_candidates = Vec::new();
    for candidate in candidates {
        if cancellation.is_requested() {
            return Err("EPUB image evidence scoring was cancelled.".into());
        }
        match review_image_text_evidence(epub_path, candidate, None, cancellation)? {
            Some(evidence) => {
                if let Some(candidate) = score_candidate(&transcript_words, candidate, evidence) {
                    scored.push(candidate);
                }
            }
            None => ocr_candidates.push(candidate),
        }
    }

    rank_candidates(&mut scored);
    if recommended_index(&scored).is_some() {
        return Ok(result_from_ranked(scored));
    }
    let Some(tesseract_executable) = tesseract_executable else {
        return Ok(result_from_ranked(scored));
    };

    for candidate in ocr_candidates {
        if cancellation.is_requested() {
            return Err("EPUB image evidence scoring was cancelled.".into());
        }
        if let Some(evidence) = review_image_text_evidence(
            epub_path,
            candidate,
            Some(tesseract_executable),
            cancellation,
        )? {
            if let Some(candidate) = score_candidate(&transcript_words, candidate, evidence) {
                scored.push(candidate);
            }
        }
    }

    rank_candidates(&mut scored);
    Ok(result_from_ranked(scored))
}

fn score_candidate(
    transcript_words: &BTreeSet<String>,
    candidate: &AudioReviewImageCandidate,
    evidence: AudioReviewImageTextEvidence,
) -> Option<AudioReviewImageMatchCandidate> {
    let evidence_text = evidence.lines.join(" ");
    let evidence_word_sequence = normalized_word_sequence(&evidence_text);
    let evidence_word_count = evidence_word_sequence.len();
    if !(MIN_EVIDENCE_WORDS..=MAX_EVIDENCE_WORDS).contains(&evidence_word_count) {
        return None;
    }
    let evidence_words = evidence_word_sequence.into_iter().collect::<BTreeSet<_>>();

    let matches = evidence_words.intersection(transcript_words).count();
    let coverage_milli = ratio_milli(matches, evidence_words.len());
    let similarity_milli = ratio_milli(matches, evidence_words.len().min(transcript_words.len()));
    let score_milli = ((u32::from(coverage_milli) * 2 + u32::from(similarity_milli)) / 3) as u16;
    let distinctive_matches = evidence_words
        .intersection(transcript_words)
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
    let recommended_index = recommended_index(&ranked);
    AudioReviewImageMatchResult {
        ranked,
        recommended_index,
    }
}

fn recommended_index(ranked: &[AudioReviewImageMatchCandidate]) -> Option<usize> {
    let winner = ranked.first()?;
    if !winner.qualifies {
        return None;
    }
    if ranked.get(1).is_some_and(|runner_up| {
        winner.score_milli.saturating_sub(runner_up.score_milli) < MIN_WINNER_MARGIN_MILLI
    }) {
        return None;
    }
    Some(0)
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
            .then_with(|| {
                left.candidate
                    .image_ordinal
                    .cmp(&right.candidate.image_ordinal)
            })
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
    normalized_word_sequence(text).into_iter().collect()
}

fn normalized_word_sequence(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|word| !word.is_empty())
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

    fn score(
        transcript_text: &str,
        candidate: &AudioReviewImageCandidate,
        evidence: AudioReviewImageTextEvidence,
    ) -> Option<AudioReviewImageMatchCandidate> {
        score_candidate(&normalized_words(transcript_text), candidate, evidence)
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
    fn empty_transcript_short_circuits_before_epub_or_ocr() {
        let result = review_image_matches(
            Path::new("does-not-need-to-exist.epub"),
            "   ",
            &[candidate(4, 0, "OPS/Images/no-hints.png", &[])],
            Some(Path::new("definitely-missing-tesseract")),
            &CancellationToken::default(),
        )
        .unwrap();

        assert!(result.ranked.is_empty());
        assert!(result.recommended().is_none());
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
    fn near_miss_runner_up_keeps_result_ambiguous() {
        let source = candidate(1, 0, "OPS/Images/winner.png", &[]);
        let evidence = embedded(&["Alpha bravo charlie delta echo"]);
        let winner = AudioReviewImageMatchCandidate {
            candidate: source.clone(),
            evidence: evidence.clone(),
            coverage_milli: 800,
            similarity_milli: 800,
            score_milli: 800,
            distinctive_matches: 4,
            evidence_word_count: 5,
            qualifies: true,
        };
        let runner_up = AudioReviewImageMatchCandidate {
            candidate: source,
            evidence,
            coverage_milli: 750,
            similarity_milli: 750,
            score_milli: 750,
            distinctive_matches: 1,
            evidence_word_count: 5,
            qualifies: false,
        };

        let result = result_from_ranked(vec![winner, runner_up]);
        assert!(result.recommended().is_none());
    }

    #[test]
    fn weak_or_generic_overlap_stays_unqualified() {
        let scored = score(
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
    fn phrase_bounds_use_total_words_not_only_unique_words() {
        let scored = score(
            "Alpha alpha bravo charlie delta",
            &candidate(
                1,
                0,
                "OPS/Images/repeated.png",
                &["Alpha alpha bravo charlie delta"],
            ),
            embedded(&["Alpha alpha bravo charlie delta"]),
        )
        .unwrap();

        assert_eq!(scored.evidence_word_count, 5);
    }

    #[test]
    fn evidence_outside_phrase_bounds_is_ignored() {
        assert!(score(
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
            score(
                "Alpha bravo charlie delta echo foxtrot",
                &source,
                AudioReviewImageTextEvidence {
                    source: AudioReviewImageEvidenceSource::Tesseract,
                    lines: vec!["Alpha bravo charlie delta echo foxtrot".into()],
                    confidence_percent: Some(90),
                },
            )
            .unwrap(),
            score(
                "Alpha bravo charlie delta echo foxtrot",
                &source,
                embedded(&["Alpha bravo charlie delta echo foxtrot"]),
            )
            .unwrap(),
        ];
        rank_candidates(&mut ranked);

        assert_eq!(
            ranked[0].evidence.source,
            AudioReviewImageEvidenceSource::Embedded
        );
    }
}
