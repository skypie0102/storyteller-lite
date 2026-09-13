use crate::{
    AlignmentDocument, AlignmentStatus, AudioReviewDecision, AudioReviewReport, CorpusPosition,
    EpubCorpus,
};
use std::collections::{BTreeSet, HashMap};

pub const DEFAULT_REVIEW_CANDIDATE_LIMIT: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioReviewTextCandidate {
    pub href: String,
    pub line_index: usize,
    pub text: String,
    /// Deterministic lexical-overlap score in the range 0..=1000.
    pub score_milli: u16,
}

#[derive(Debug, Clone)]
struct CorpusBlock {
    href: String,
    line_index: usize,
    text: String,
}

pub fn review_text_candidates(
    alignment: &AlignmentDocument,
    corpus: &EpubCorpus,
    alignment_index: usize,
    limit: usize,
) -> Result<Vec<AudioReviewTextCandidate>, String> {
    if limit == 0 {
        return Err("Audio review candidate limit must be greater than zero.".into());
    }
    let segment = alignment
        .segments
        .get(alignment_index)
        .ok_or_else(|| format!("Alignment segment {alignment_index} does not exist."))?;
    if segment.status != AlignmentStatus::Unmatched {
        return Err(format!(
            "Alignment segment {alignment_index} is already matched and does not need review."
        ));
    }

    let blocks = corpus_blocks(corpus);
    if blocks.is_empty() {
        return Err("EPUB corpus contains no reviewable text blocks.".into());
    }
    let positions = corpus_position_map(&blocks);
    let previous = nearest_previous_position(alignment, alignment_index, &positions)?;
    let next = nearest_next_position(alignment, alignment_index, &positions)?;
    let minimum = previous.unwrap_or(0);
    let maximum = next.unwrap_or(blocks.len().saturating_sub(1));
    if minimum > maximum {
        return Err("Neighboring alignment positions leave no monotonic review window.".into());
    }

    let mut ranked = blocks[minimum..=maximum]
        .iter()
        .enumerate()
        .map(|(offset, block)| {
            let absolute_index = minimum + offset;
            (
                lexical_overlap_milli(&segment.transcript_text, &block.text),
                absolute_index,
            )
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    ranked.truncate(limit.min(ranked.len()));
    ranked.sort_by_key(|(_, absolute_index)| *absolute_index);

    Ok(ranked
        .into_iter()
        .map(|(score_milli, absolute_index)| {
            let block = &blocks[absolute_index];
            AudioReviewTextCandidate {
                href: block.href.clone(),
                line_index: block.line_index,
                text: block.text.clone(),
                score_milli,
            }
        })
        .collect())
}

pub fn apply_audio_review_to_alignment(
    alignment: &AlignmentDocument,
    corpus: &EpubCorpus,
    review: &AudioReviewReport,
) -> Result<AlignmentDocument, String> {
    if !review.is_complete() {
        return Err("Audio review still contains pending decisions.".into());
    }
    if alignment.segments.len() != alignment.total_segments {
        return Err("Alignment map segment count is inconsistent.".into());
    }
    if review.total_segments != alignment.total_segments {
        return Err("Audio review no longer matches the alignment segment count.".into());
    }
    if review.matched_segments != alignment.matched_segments {
        return Err("Audio review no longer matches the source alignment matched count.".into());
    }
    let expected_unmatched = alignment
        .segments
        .iter()
        .filter(|segment| segment.status == AlignmentStatus::Unmatched)
        .count();
    if review.unmatched.len() != expected_unmatched {
        return Err("Audio review does not contain a decision record for every unmatched alignment segment.".into());
    }

    let blocks = corpus_blocks(corpus);
    let positions = corpus_position_map(&blocks);
    let mut block_by_key = HashMap::<(String, usize), &CorpusBlock>::new();
    for block in &blocks {
        block_by_key.insert((block.href.clone(), block.line_index), block);
    }

    let mut effective = alignment.clone();
    let mut seen_indexes = BTreeSet::new();
    for item in &review.unmatched {
        if !seen_indexes.insert(item.alignment_index) {
            return Err(format!(
                "Audio review contains duplicate alignment index {}.",
                item.alignment_index
            ));
        }
        let original = alignment
            .segments
            .get(item.alignment_index)
            .ok_or_else(|| {
                format!(
                    "Audio review references missing alignment segment {}.",
                    item.alignment_index
                )
            })?;
        if original.status != AlignmentStatus::Unmatched {
            return Err(format!(
                "Audio review segment {} is no longer unmatched in the source alignment.",
                item.alignment_index
            ));
        }
        if original.audio_start_ms != item.audio_start_ms
            || original.audio_end_ms != item.audio_end_ms
            || original.transcript_text != item.transcript_text
        {
            return Err(format!(
                "Audio review segment {} no longer matches the source alignment content.",
                item.alignment_index
            ));
        }

        match &item.decision {
            AudioReviewDecision::Pending => {
                return Err(format!(
                    "Audio review segment {} still has a pending decision.",
                    item.alignment_index
                ));
            }
            AudioReviewDecision::Excluded { .. } => {}
            AudioReviewDecision::Assigned { destination, .. } => {
                if destination.supplemental.is_some() {
                    if !corpus
                        .sections
                        .iter()
                        .any(|section| section.href == destination.href)
                    {
                        return Err(format!(
                            "Supplemental audio review anchor {} is not an EPUB reading-order document.",
                            destination.href
                        ));
                    }
                    continue;
                }
                if destination.image_href.is_some() {
                    return Err(
                        "Graphic/image audio assignments require the dedicated image rendering path, which is not implemented yet."
                            .into(),
                    );
                }
                let line_index = destination
                    .line_index
                    .ok_or("Text audio assignment requires an EPUB text-block line index.")?;
                let key = (destination.href.clone(), line_index);
                let block = block_by_key.get(&key).ok_or_else(|| {
                    format!(
                        "Audio review destination {} line {} is not a readable EPUB text block.",
                        destination.href, line_index
                    )
                })?;
                let block_position = positions.get(&key).copied().ok_or_else(|| {
                    "Audio review destination could not be resolved in EPUB reading order."
                        .to_string()
                })?;
                validate_assignment_window(
                    alignment,
                    item.alignment_index,
                    block_position,
                    &positions,
                )?;

                let end_offset = block.text.chars().count();
                if end_offset == 0 {
                    return Err("Audio review destination text block is empty.".into());
                }
                let segment = &mut effective.segments[item.alignment_index];
                segment.status = AlignmentStatus::Matched;
                segment.match_percent = None;
                segment.book_start = Some(CorpusPosition {
                    href: destination.href.clone(),
                    line_index,
                    char_offset: 0,
                });
                segment.book_end = Some(CorpusPosition {
                    href: destination.href.clone(),
                    line_index,
                    char_offset: end_offset,
                });
            }
        }
    }

    validate_effective_monotonicity(&effective, &positions)?;
    effective.matched_segments = effective
        .segments
        .iter()
        .filter(|segment| segment.status == AlignmentStatus::Matched)
        .count();
    effective.match_percent = if effective.total_segments == 0 {
        100.0
    } else {
        effective.matched_segments as f64 * 100.0 / effective.total_segments as f64
    };
    Ok(effective)
}

fn corpus_blocks(corpus: &EpubCorpus) -> Vec<CorpusBlock> {
    let mut blocks = Vec::new();
    for section in &corpus.sections {
        for (line_index, line) in section.text.lines().enumerate() {
            let text = line.trim();
            if text.is_empty() {
                continue;
            }
            blocks.push(CorpusBlock {
                href: section.href.clone(),
                line_index,
                text: text.to_string(),
            });
        }
    }
    blocks
}

fn corpus_position_map(blocks: &[CorpusBlock]) -> HashMap<(String, usize), usize> {
    blocks
        .iter()
        .enumerate()
        .map(|(position, block)| ((block.href.clone(), block.line_index), position))
        .collect()
}

fn nearest_previous_position(
    alignment: &AlignmentDocument,
    alignment_index: usize,
    positions: &HashMap<(String, usize), usize>,
) -> Result<Option<usize>, String> {
    for segment in alignment.segments[..alignment_index].iter().rev() {
        if segment.status != AlignmentStatus::Matched {
            continue;
        }
        let position = segment
            .book_end
            .as_ref()
            .ok_or("Matched alignment segment is missing its book end position.")?;
        return resolve_position(position, positions).map(Some);
    }
    Ok(None)
}

fn nearest_next_position(
    alignment: &AlignmentDocument,
    alignment_index: usize,
    positions: &HashMap<(String, usize), usize>,
) -> Result<Option<usize>, String> {
    for segment in alignment.segments[alignment_index + 1..].iter() {
        if segment.status != AlignmentStatus::Matched {
            continue;
        }
        let position = segment
            .book_start
            .as_ref()
            .ok_or("Matched alignment segment is missing its book start position.")?;
        return resolve_position(position, positions).map(Some);
    }
    Ok(None)
}

fn resolve_position(
    position: &CorpusPosition,
    positions: &HashMap<(String, usize), usize>,
) -> Result<usize, String> {
    positions
        .get(&(position.href.clone(), position.line_index))
        .copied()
        .ok_or_else(|| {
            format!(
                "Alignment position {} line {} is not present in the EPUB corpus.",
                position.href, position.line_index
            )
        })
}

fn validate_assignment_window(
    alignment: &AlignmentDocument,
    alignment_index: usize,
    destination_position: usize,
    positions: &HashMap<(String, usize), usize>,
) -> Result<(), String> {
    if let Some(previous) = nearest_previous_position(alignment, alignment_index, positions)? {
        if destination_position < previous {
            return Err("Audio review assignment would move backward before the previous matched EPUB block.".into());
        }
    }
    if let Some(next) = nearest_next_position(alignment, alignment_index, positions)? {
        if destination_position > next {
            return Err(
                "Audio review assignment would move forward past the next matched EPUB block."
                    .into(),
            );
        }
    }
    Ok(())
}

fn validate_effective_monotonicity(
    alignment: &AlignmentDocument,
    positions: &HashMap<(String, usize), usize>,
) -> Result<(), String> {
    let mut previous = None::<usize>;
    for segment in &alignment.segments {
        if segment.status != AlignmentStatus::Matched {
            continue;
        }
        let start = segment
            .book_start
            .as_ref()
            .ok_or("Matched alignment segment is missing its book start position.")?;
        let current = resolve_position(start, positions)?;
        if previous.is_some_and(|previous| current < previous) {
            return Err("Reviewed alignment is not monotonic in EPUB reading order.".into());
        }
        previous = Some(current);
    }
    Ok(())
}

fn lexical_overlap_milli(left: &str, right: &str) -> u16 {
    let left = normalized_words(left);
    let right = normalized_words(right);
    if left.is_empty() || right.is_empty() {
        return 0;
    }
    let intersection = left.intersection(&right).count();
    let union = left.union(&right).count();
    ((intersection * 1000) / union.max(1)) as u16
}

fn normalized_words(text: &str) -> BTreeSet<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter_map(|word| {
            let normalized = word.trim().to_lowercase();
            (!normalized.is_empty()).then_some(normalized)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AlignmentSegment, AudioReviewDecisionSource, AudioReviewDestination, AudioReviewItem,
        EpubSection,
    };

    fn corpus() -> EpubCorpus {
        EpubCorpus {
            package_path: "OPS/package.opf".into(),
            sections: vec![
                EpubSection {
                    href: "OPS/ch1.xhtml".into(),
                    text: "Alpha opening\nBravo middle\nCharlie ending".into(),
                },
                EpubSection {
                    href: "OPS/ch2.xhtml".into(),
                    text: "Delta next\nEcho close".into(),
                },
            ],
        }
    }

    fn segment(
        start: u64,
        end: u64,
        text: &str,
        status: AlignmentStatus,
        href: Option<&str>,
        line: usize,
    ) -> AlignmentSegment {
        let position = href.map(|href| CorpusPosition {
            href: href.into(),
            line_index: line,
            char_offset: 0,
        });
        AlignmentSegment {
            audio_start_ms: start,
            audio_end_ms: end,
            transcript_text: text.into(),
            status,
            match_percent: (status == AlignmentStatus::Matched).then_some(100.0),
            book_start: position.clone(),
            book_end: position,
        }
    }

    fn alignment() -> AlignmentDocument {
        AlignmentDocument {
            algorithm: "test".into(),
            language: Some("en".into()),
            total_segments: 3,
            matched_segments: 2,
            match_percent: 66.666,
            segments: vec![
                segment(
                    0,
                    1000,
                    "alpha",
                    AlignmentStatus::Matched,
                    Some("OPS/ch1.xhtml"),
                    0,
                ),
                segment(
                    1000,
                    2000,
                    "bravo middle",
                    AlignmentStatus::Unmatched,
                    None,
                    0,
                ),
                segment(
                    2000,
                    3000,
                    "delta",
                    AlignmentStatus::Matched,
                    Some("OPS/ch2.xhtml"),
                    0,
                ),
            ],
        }
    }

    fn report(decision: AudioReviewDecision) -> AudioReviewReport {
        AudioReviewReport {
            total_segments: 3,
            matched_segments: 2,
            match_percent: 66.666,
            unmatched: vec![AudioReviewItem {
                id: "review-test".into(),
                alignment_index: 1,
                audio_start_ms: 1000,
                audio_end_ms: 2000,
                transcript_text: "bravo middle".into(),
                suggestion: None,
                edge: None,
                silence: None,
                decision,
            }],
            accepted_unmatched_exclusion: false,
        }
    }

    #[test]
    fn candidates_stay_inside_neighboring_matches_and_prefer_overlap() {
        let candidates = review_text_candidates(&alignment(), &corpus(), 1, 3).unwrap();
        assert!(candidates
            .iter()
            .all(|candidate| candidate.href == "OPS/ch1.xhtml" || candidate.line_index == 0));
        assert!(candidates.iter().any(|candidate| {
            candidate.href == "OPS/ch1.xhtml"
                && candidate.line_index == 1
                && candidate.score_milli > 0
        }));
    }

    #[test]
    fn assigned_text_block_becomes_effectively_matched() {
        let decision = AudioReviewDecision::Assigned {
            destination: AudioReviewDestination {
                href: "OPS/ch1.xhtml".into(),
                line_index: Some(1),
                image_href: None,
                supplemental: None,
            },
            classification: None,
            source: AudioReviewDecisionSource::Manual,
        };
        let effective =
            apply_audio_review_to_alignment(&alignment(), &corpus(), &report(decision)).unwrap();
        let segment = &effective.segments[1];
        assert_eq!(segment.status, AlignmentStatus::Matched);
        assert_eq!(segment.book_start.as_ref().unwrap().line_index, 1);
        assert_eq!(effective.matched_segments, 3);
        assert_eq!(effective.match_percent, 100.0);
    }

    #[test]
    fn assignment_outside_monotonic_window_is_rejected() {
        let decision = AudioReviewDecision::Assigned {
            destination: AudioReviewDestination {
                href: "OPS/ch2.xhtml".into(),
                line_index: Some(1),
                image_href: None,
                supplemental: None,
            },
            classification: None,
            source: AudioReviewDecisionSource::Manual,
        };
        let error = apply_audio_review_to_alignment(&alignment(), &corpus(), &report(decision))
            .unwrap_err();
        assert!(error.contains("next matched"));
    }

    #[test]
    fn image_assignment_fails_explicitly_until_renderer_exists() {
        let decision = AudioReviewDecision::Assigned {
            destination: AudioReviewDestination {
                href: "OPS/ch1.xhtml".into(),
                line_index: None,
                image_href: Some("OPS/image.png".into()),
                supplemental: None,
            },
            classification: None,
            source: AudioReviewDecisionSource::Manual,
        };
        let error = apply_audio_review_to_alignment(&alignment(), &corpus(), &report(decision))
            .unwrap_err();
        assert!(error.contains("image rendering path"));
    }

    #[test]
    fn pending_decision_cannot_materialize() {
        let error = apply_audio_review_to_alignment(
            &alignment(),
            &corpus(),
            &report(AudioReviewDecision::Pending),
        )
        .unwrap_err();
        assert!(error.contains("pending"));
    }
}
