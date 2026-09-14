use crate::{
    apply_audio_review_to_alignment, read_audio_review_report, read_epub_corpus, AlignmentDocument,
    AudioReviewDecision,
};
use std::{fs, path::Path};

pub fn materialize_reviewed_alignment(
    alignment_path: &Path,
    corpus_path: &Path,
    review_path: &Path,
    destination: &Path,
) -> Result<AlignmentDocument, String> {
    let data = fs::read(alignment_path).map_err(|error| {
        format!(
            "Could not read source alignment {}: {error}",
            alignment_path.display()
        )
    })?;
    let alignment: AlignmentDocument = serde_json::from_slice(&data).map_err(|error| {
        format!(
            "Could not parse source alignment {}: {error}",
            alignment_path.display()
        )
    })?;
    let corpus = read_epub_corpus(corpus_path)?;
    let review = read_audio_review_report(review_path)?;
    let mut alignment_review = review.clone();
    for item in &mut alignment_review.unmatched {
        let AudioReviewDecision::Assigned {
            destination,
            source,
            ..
        } = &item.decision
        else {
            continue;
        };
        if destination.image_href.is_some() {
            item.decision = AudioReviewDecision::Excluded {
                reason: "Graphic Readout is materialized directly on its EPUB image target.".into(),
                source: *source,
            };
        }
    }
    let effective = apply_audio_review_to_alignment(&alignment, &corpus, &alignment_review)?;

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Could not create reviewed alignment directory {}: {error}",
                parent.display()
            )
        })?;
    }
    let json = serde_json::to_vec_pretty(&effective)
        .map_err(|error| format!("Could not serialize reviewed alignment: {error}"))?;
    fs::write(destination, json).map_err(|error| {
        format!(
            "Could not write reviewed alignment {}: {error}",
            destination.display()
        )
    })?;
    Ok(effective)
}
