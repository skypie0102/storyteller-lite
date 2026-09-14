use crate::{
    apply_audio_review_decision, read_audio_review_report, read_epub_corpus,
    review_image_candidates, review_image_matches, AlignmentDocument, AudioReviewClassification,
    AudioReviewDecision, AudioReviewDecisionSource, AudioReviewDestination,
    AudioReviewImageCandidate, AudioReviewPolicy, CancellationToken,
    DEFAULT_REVIEW_IMAGE_DOCUMENT_LIMIT, DEFAULT_REVIEW_IMAGE_LIMIT,
};
use std::{fs, path::Path};

/// Applies conservative Smart Graphic Readout assignments to currently-pending review items.
///
/// Candidate discovery remains bounded by the neighboring accepted alignment anchors. Embedded
/// image evidence is evaluated first by `review_image_matches`; optional OCR is used only when the
/// caller supplies a Tesseract executable and embedded evidence does not already produce a strong,
/// unambiguous winner. Evidence failures are advisory and leave the item Pending, while explicit
/// cancellation still aborts the pass.
pub fn apply_smart_graphic_readouts(
    epub_path: &Path,
    alignment_path: &Path,
    corpus_path: &Path,
    report_path: &Path,
    draft_path: &Path,
    policy: AudioReviewPolicy,
    tesseract_executable: Option<&Path>,
    cancellation: &CancellationToken,
) -> Result<usize, String> {
    if policy != AudioReviewPolicy::Smart {
        return Ok(0);
    }
    if cancellation.is_requested() {
        return Err("Smart Graphic Readout matching was cancelled.".into());
    }

    let alignment_data = fs::read(alignment_path).map_err(|error| {
        format!(
            "Could not read alignment map {}: {error}",
            alignment_path.display()
        )
    })?;
    let alignment: AlignmentDocument =
        serde_json::from_slice(&alignment_data).map_err(|error| {
            format!(
                "Could not parse alignment map {}: {error}",
                alignment_path.display()
            )
        })?;
    let corpus = read_epub_corpus(corpus_path)?;
    let report = read_audio_review_report(report_path)?;
    let pending = report
        .unmatched
        .iter()
        .filter(|item| item.decision.is_pending() && item.edge.is_none())
        .cloned()
        .collect::<Vec<_>>();

    let mut assigned = 0usize;
    for item in pending {
        if cancellation.is_requested() {
            return Err("Smart Graphic Readout matching was cancelled.".into());
        }
        let candidates = match review_image_candidates(
            epub_path,
            &alignment,
            &corpus,
            item.alignment_index,
            DEFAULT_REVIEW_IMAGE_DOCUMENT_LIMIT,
            DEFAULT_REVIEW_IMAGE_LIMIT,
            cancellation,
        ) {
            Ok(candidates) => candidates,
            Err(error) if cancellation.is_requested() => return Err(error),
            Err(_) => continue,
        };
        if candidates.is_empty() {
            continue;
        }
        let matches = match review_image_matches(
            epub_path,
            &item.transcript_text,
            &candidates,
            tesseract_executable,
            cancellation,
        ) {
            Ok(matches) => matches,
            Err(error) if cancellation.is_requested() => return Err(error),
            Err(_) => continue,
        };
        let Some(recommended) = matches.recommended() else {
            continue;
        };
        if apply_automatic_graphic_readout_decision(
            report_path,
            draft_path,
            policy,
            &item.id,
            &recommended.candidate,
        )? {
            assigned = assigned.saturating_add(1);
        }
    }
    Ok(assigned)
}

fn apply_automatic_graphic_readout_decision(
    report_path: &Path,
    draft_path: &Path,
    policy: AudioReviewPolicy,
    item_id: &str,
    candidate: &AudioReviewImageCandidate,
) -> Result<bool, String> {
    if policy != AudioReviewPolicy::Smart {
        return Ok(false);
    }
    let report = read_audio_review_report(report_path)?;
    let item = report
        .unmatched
        .iter()
        .find(|item| item.id == item_id)
        .ok_or_else(|| format!("Audio review item {item_id} was not found."))?;
    if !item.decision.is_pending() || item.edge.is_some() {
        return Ok(false);
    }
    let already_used = report.unmatched.iter().any(|other| {
        matches!(
            &other.decision,
            AudioReviewDecision::Assigned { destination, .. }
                if destination.href == candidate.document_href
                    && destination.image_href.as_deref() == Some(candidate.image_href.as_str())
        )
    });
    if already_used {
        return Ok(false);
    }

    apply_audio_review_decision(
        report_path,
        draft_path,
        item_id,
        AudioReviewDecision::Assigned {
            destination: AudioReviewDestination {
                href: candidate.document_href.clone(),
                line_index: None,
                image_href: Some(candidate.image_href.clone()),
                supplemental: None,
            },
            classification: Some(AudioReviewClassification::GraphicReadout),
            source: AudioReviewDecisionSource::Automatic,
        },
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AudioReviewEdge, AudioReviewItem, AudioReviewReport};
    use std::path::PathBuf;

    fn candidate() -> AudioReviewImageCandidate {
        AudioReviewImageCandidate {
            document_href: "OPS/Text/diagram.xhtml".into(),
            image_href: "OPS/Images/diagram.png".into(),
            media_type: "image/png".into(),
            byte_size: 123,
            document_spine_index: 2,
            image_ordinal: 0,
            embedded_text: vec!["Alice Robert Clara Daniel family tree".into()],
        }
    }

    fn item(id: &str) -> AudioReviewItem {
        AudioReviewItem {
            id: id.into(),
            alignment_index: 1,
            audio_start_ms: 1000,
            audio_end_ms: 2000,
            transcript_text: "Alice Robert Clara Daniel family tree".into(),
            suggestion: None,
            edge: None,
            silence: None,
            decision: AudioReviewDecision::Pending,
        }
    }

    fn temp_paths(label: &str, report: &AudioReviewReport) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "storyteller-graphic-review-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let report_path = root.join("review.json");
        let draft_path = root.join("review-draft.json");
        fs::write(&report_path, serde_json::to_vec_pretty(report).unwrap()).unwrap();
        (report_path, draft_path)
    }

    fn report(items: Vec<AudioReviewItem>) -> AudioReviewReport {
        AudioReviewReport {
            total_segments: items.len(),
            matched_segments: 0,
            match_percent: 0.0,
            unmatched: items,
            accepted_unmatched_exclusion: false,
        }
    }

    #[test]
    fn smart_assignment_records_graphic_destination_and_provenance() {
        let source = report(vec![item("graphic-one")]);
        let (report_path, draft_path) = temp_paths("smart", &source);

        assert!(apply_automatic_graphic_readout_decision(
            &report_path,
            &draft_path,
            AudioReviewPolicy::Smart,
            "graphic-one",
            &candidate(),
        )
        .unwrap());

        let updated = read_audio_review_report(&report_path).unwrap();
        assert!(matches!(
            &updated.unmatched[0].decision,
            AudioReviewDecision::Assigned {
                destination: AudioReviewDestination {
                    href,
                    image_href: Some(image_href),
                    line_index: None,
                    supplemental: None,
                },
                classification: Some(AudioReviewClassification::GraphicReadout),
                source: AudioReviewDecisionSource::Automatic,
            } if href == "OPS/Text/diagram.xhtml" && image_href == "OPS/Images/diagram.png"
        ));
        assert!(draft_path.is_file());
    }

    #[test]
    fn review_all_never_applies_automatic_graphic_assignment() {
        let source = report(vec![item("graphic-one")]);
        let (report_path, draft_path) = temp_paths("review-all", &source);

        assert!(!apply_automatic_graphic_readout_decision(
            &report_path,
            &draft_path,
            AudioReviewPolicy::ReviewAll,
            "graphic-one",
            &candidate(),
        )
        .unwrap());
        assert!(read_audio_review_report(&report_path).unwrap().unmatched[0]
            .decision
            .is_pending());
        assert!(!draft_path.exists());
    }

    #[test]
    fn existing_manual_decision_wins_over_smart_graphic_assignment() {
        let mut existing = item("graphic-one");
        existing.decision = AudioReviewDecision::Excluded {
            reason: "Reviewed manually.".into(),
            source: AudioReviewDecisionSource::Manual,
        };
        let source = report(vec![existing]);
        let (report_path, draft_path) = temp_paths("manual", &source);

        assert!(!apply_automatic_graphic_readout_decision(
            &report_path,
            &draft_path,
            AudioReviewPolicy::Smart,
            "graphic-one",
            &candidate(),
        )
        .unwrap());
        assert!(matches!(
            read_audio_review_report(&report_path).unwrap().unmatched[0].decision,
            AudioReviewDecision::Excluded {
                source: AudioReviewDecisionSource::Manual,
                ..
            }
        ));
    }

    #[test]
    fn edge_item_is_not_reclassified_as_graphic_readout() {
        let mut edge = item("edge-one");
        edge.edge = Some(AudioReviewEdge::Introduction);
        let source = report(vec![edge]);
        let (report_path, draft_path) = temp_paths("edge", &source);

        assert!(!apply_automatic_graphic_readout_decision(
            &report_path,
            &draft_path,
            AudioReviewPolicy::Smart,
            "edge-one",
            &candidate(),
        )
        .unwrap());
        assert!(read_audio_review_report(&report_path).unwrap().unmatched[0]
            .decision
            .is_pending());
    }

    #[test]
    fn automatic_graphic_target_is_not_allocated_twice() {
        let mut first = item("graphic-one");
        first.decision = AudioReviewDecision::Assigned {
            destination: AudioReviewDestination {
                href: "OPS/Text/diagram.xhtml".into(),
                line_index: None,
                image_href: Some("OPS/Images/diagram.png".into()),
                supplemental: None,
            },
            classification: Some(AudioReviewClassification::GraphicReadout),
            source: AudioReviewDecisionSource::Automatic,
        };
        let mut second = item("graphic-two");
        second.alignment_index = 2;
        second.audio_start_ms = 2000;
        second.audio_end_ms = 3000;
        let source = report(vec![first, second]);
        let (report_path, draft_path) = temp_paths("duplicate", &source);

        assert!(!apply_automatic_graphic_readout_decision(
            &report_path,
            &draft_path,
            AudioReviewPolicy::Smart,
            "graphic-two",
            &candidate(),
        )
        .unwrap());
        let updated = read_audio_review_report(&report_path).unwrap();
        assert!(updated.unmatched[1].decision.is_pending());
    }
}
