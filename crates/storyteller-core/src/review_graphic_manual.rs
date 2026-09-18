use crate::{
    apply_audio_review_decision, read_audio_review_report, read_epub_corpus,
    review_image_candidates, AlignmentDocument, AudioReviewClassification, AudioReviewDecision,
    AudioReviewDecisionSource, AudioReviewDestination, AudioReviewImageCandidate,
    AudioReviewReport, CancellationToken, DEFAULT_REVIEW_IMAGE_DOCUMENT_LIMIT,
    DEFAULT_REVIEW_IMAGE_LIMIT,
};
use std::{fs, path::Path};

/// Records a manual Graphic Readout assignment only after rediscovering the selected image inside
/// the same bounded EPUB window used by Smart review.
///
/// The UI-facing document/image pair is never trusted as a free-form destination. This function
/// reloads the current review item, reopens the alignment/corpus inputs, reruns bounded image
/// discovery, rejects edge narration and duplicate image ownership, and only then persists a
/// Manual Graphic Readout decision.
#[allow(clippy::too_many_arguments)]
pub fn assign_manual_graphic_readout(
    epub_path: &Path,
    alignment_path: &Path,
    corpus_path: &Path,
    report_path: &Path,
    draft_path: &Path,
    item_id: &str,
    document_href: &str,
    image_href: &str,
    cancellation: &CancellationToken,
) -> Result<(), String> {
    if cancellation.is_requested() {
        return Err("Manual Graphic Readout assignment was cancelled.".into());
    }

    let report = read_audio_review_report(report_path)?;
    let alignment_index = report
        .unmatched
        .iter()
        .find(|item| item.id == item_id)
        .ok_or_else(|| format!("Audio review item {item_id} was not found."))?
        .alignment_index;

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
    let candidates = review_image_candidates(
        epub_path,
        &alignment,
        &corpus,
        alignment_index,
        DEFAULT_REVIEW_IMAGE_DOCUMENT_LIMIT,
        DEFAULT_REVIEW_IMAGE_LIMIT,
        cancellation,
    )?;
    let candidate =
        validate_manual_graphic_target(&report, item_id, &candidates, document_href, image_href)?;

    apply_audio_review_decision(
        report_path,
        draft_path,
        item_id,
        AudioReviewDecision::Assigned {
            destination: AudioReviewDestination {
                href: candidate.document_href,
                line_index: None,
                image_href: Some(candidate.image_href),
                supplemental: None,
            },
            classification: Some(AudioReviewClassification::GraphicReadout),
            source: AudioReviewDecisionSource::Manual,
        },
    )
}

fn validate_manual_graphic_target(
    report: &AudioReviewReport,
    item_id: &str,
    candidates: &[AudioReviewImageCandidate],
    document_href: &str,
    image_href: &str,
) -> Result<AudioReviewImageCandidate, String> {
    let item = report
        .unmatched
        .iter()
        .find(|item| item.id == item_id)
        .ok_or_else(|| format!("Audio review item {item_id} was not found."))?;
    if item.edge.is_some() {
        return Err(
            "Leading/trailing edge narration cannot be manually reassigned as a Graphic Readout."
                .into(),
        );
    }

    let already_used = report.unmatched.iter().any(|other| {
        if other.id == item_id {
            return false;
        }
        matches!(
            &other.decision,
            AudioReviewDecision::Assigned { destination, .. }
                if destination.href == document_href
                    && destination.image_href.as_deref() == Some(image_href)
        )
    });
    if already_used {
        return Err(format!(
            "Graphic Readout image {image_href} in {document_href} is already assigned to another audio segment."
        ));
    }

    candidates
        .iter()
        .find(|candidate| {
            candidate.document_href == document_href && candidate.image_href == image_href
        })
        .cloned()
        .ok_or_else(|| {
            format!(
                "Graphic Readout target {image_href} in {document_href} is not a current bounded EPUB image candidate."
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AudioReviewEdge, AudioReviewItem};

    fn candidate(document_href: &str, image_href: &str) -> AudioReviewImageCandidate {
        AudioReviewImageCandidate {
            document_href: document_href.into(),
            image_href: image_href.into(),
            media_type: "image/png".into(),
            byte_size: 123,
            document_spine_index: 2,
            image_ordinal: 0,
            embedded_text: vec!["Family tree showing Alice Robert Clara Daniel".into()],
        }
    }

    fn item(id: &str) -> AudioReviewItem {
        AudioReviewItem {
            id: id.into(),
            alignment_index: 4,
            audio_start_ms: 1_000,
            audio_end_ms: 2_000,
            transcript_text: "Family tree showing Alice Robert Clara Daniel".into(),
            suggestion: None,
            edge: None,
            silence: None,
            decision: AudioReviewDecision::Pending,
        }
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
    fn current_bounded_target_is_accepted() {
        let report = report(vec![item("manual")]);
        let source = candidate("OPS/Text/diagram.xhtml", "OPS/Images/diagram.png");
        let selected = validate_manual_graphic_target(
            &report,
            "manual",
            std::slice::from_ref(&source),
            &source.document_href,
            &source.image_href,
        )
        .unwrap();

        assert_eq!(selected, source);
    }

    #[test]
    fn arbitrary_target_is_rejected() {
        let report = report(vec![item("manual")]);
        let error = validate_manual_graphic_target(
            &report,
            "manual",
            &[candidate(
                "OPS/Text/diagram.xhtml",
                "OPS/Images/diagram.png",
            )],
            "OPS/Text/diagram.xhtml",
            "OPS/Images/not-a-candidate.png",
        )
        .unwrap_err();

        assert!(error.contains("not a current bounded EPUB image candidate"));
    }

    #[test]
    fn edge_item_cannot_be_reclassified_as_graphic_readout() {
        let mut edge = item("edge");
        edge.edge = Some(AudioReviewEdge::Introduction);
        let report = report(vec![edge]);
        let source = candidate("OPS/Text/diagram.xhtml", "OPS/Images/diagram.png");
        let error = validate_manual_graphic_target(
            &report,
            "edge",
            std::slice::from_ref(&source),
            &source.document_href,
            &source.image_href,
        )
        .unwrap_err();

        assert!(error.contains("edge narration"));
    }

    #[test]
    fn image_target_cannot_be_allocated_twice() {
        let source = candidate("OPS/Text/diagram.xhtml", "OPS/Images/diagram.png");
        let mut existing = item("existing");
        existing.decision = AudioReviewDecision::Assigned {
            destination: AudioReviewDestination {
                href: source.document_href.clone(),
                line_index: None,
                image_href: Some(source.image_href.clone()),
                supplemental: None,
            },
            classification: Some(AudioReviewClassification::GraphicReadout),
            source: AudioReviewDecisionSource::Automatic,
        };
        let report = report(vec![existing, item("manual")]);
        let error = validate_manual_graphic_target(
            &report,
            "manual",
            std::slice::from_ref(&source),
            &source.document_href,
            &source.image_href,
        )
        .unwrap_err();

        assert!(error.contains("already assigned"));
    }
}
