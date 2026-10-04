use std::path::PathBuf;
use storyteller_core::{
    apply_audio_review_decision, assign_manual_graphic_readout, prepared_job_sources,
    read_audio_review_report, AlignmentDocument, AlignmentStatus, AudioReviewClassification,
    AudioReviewDecision, AudioReviewDecisionSource, AudioReviewDestination, AudioReviewEdge,
    AudioReviewItem, AudioReviewReport, AudioReviewSupplementalPlacement, CancellationToken, Job,
    PipelineStage, StageArtifacts,
};

pub fn audio_review_path(job: &Job) -> PathBuf {
    crate::job_workspace(job)
        .stage_dir(PipelineStage::ReviewAudio)
        .join("review.json")
}

pub fn audio_review_draft_path(job: &Job) -> PathBuf {
    crate::job_workspace(job).root().join("review-draft.json")
}

pub fn load_audio_review_report(job: &Job) -> Result<AudioReviewReport, String> {
    read_audio_review_report(&audio_review_path(job))
}

pub(crate) fn seal_review(job: &Job) -> Result<(), String> {
    crate::job_workspace(job).capture_stage_artifacts(
        PipelineStage::ReviewAudio,
        &StageArtifacts::ReviewAudio {
            report: PathBuf::from("review.json"),
        },
        &CancellationToken::default(),
    )
}

pub(crate) fn save_decision(
    job: &Job,
    item_id: &str,
    decision: AudioReviewDecision,
) -> Result<(), String> {
    apply_audio_review_decision(
        &audio_review_path(job),
        &audio_review_draft_path(job),
        item_id,
        decision,
    )
}

pub(crate) fn assign_graphic(
    job: &Job,
    item_id: &str,
    document_href: &str,
    image_href: &str,
) -> Result<(), String> {
    let workspace = crate::job_workspace(job);
    let prepared = prepared_job_sources(job, &workspace)?;
    assign_manual_graphic_readout(
        prepared.epub(),
        &workspace
            .stage_dir(PipelineStage::Align)
            .join("alignment.json"),
        &workspace
            .stage_dir(PipelineStage::Analyze)
            .join("book-corpus.json"),
        &audio_review_path(job),
        &audio_review_draft_path(job),
        item_id,
        document_href,
        image_href,
        &CancellationToken::default(),
    )
}

pub(crate) fn preserve_edge(job: &Job, item_id: &str) -> Result<(), String> {
    let report = load_audio_review_report(job)?;
    let item = report
        .unmatched
        .iter()
        .find(|item| item.id == item_id)
        .ok_or_else(|| "Selected review segment no longer exists.".to_string())?;
    let (href, placement, classification) = edge_page_destination(job, item)?;
    save_decision(
        job,
        item_id,
        AudioReviewDecision::Assigned {
            destination: AudioReviewDestination {
                href,
                line_index: None,
                image_href: None,
                supplemental: Some(placement),
            },
            classification: Some(classification),
            source: AudioReviewDecisionSource::Manual,
        },
    )
}

fn edge_page_destination(
    job: &Job,
    item: &AudioReviewItem,
) -> Result<
    (
        String,
        AudioReviewSupplementalPlacement,
        AudioReviewClassification,
    ),
    String,
> {
    let workspace = crate::job_workspace(job);
    let alignment_path = workspace
        .stage_dir(PipelineStage::Align)
        .join("alignment.json");
    let data = std::fs::read(&alignment_path).map_err(|error| {
        format!(
            "Could not read alignment map {}: {error}",
            alignment_path.display()
        )
    })?;
    let alignment: AlignmentDocument = serde_json::from_slice(&data).map_err(|error| {
        format!(
            "Could not parse alignment map {}: {error}",
            alignment_path.display()
        )
    })?;
    match item.edge {
        Some(AudioReviewEdge::Introduction) => {
            let href = alignment
                .segments
                .iter()
                .find(|segment| segment.status == AlignmentStatus::Matched)
                .and_then(|segment| segment.book_start.as_ref())
                .map(|position| position.href.clone())
                .ok_or_else(|| "Introduction has no matched EPUB anchor.".to_string())?;
            Ok((
                href,
                AudioReviewSupplementalPlacement::BeforeAnchor,
                AudioReviewClassification::Introduction,
            ))
        }
        Some(AudioReviewEdge::Credits) => {
            let href = alignment
                .segments
                .iter()
                .rev()
                .find(|segment| segment.status == AlignmentStatus::Matched)
                .and_then(|segment| segment.book_end.as_ref())
                .map(|position| position.href.clone())
                .ok_or_else(|| "Credits have no matched EPUB anchor.".to_string())?;
            Ok((
                href,
                AudioReviewSupplementalPlacement::AfterAnchor,
                AudioReviewClassification::Credits,
            ))
        }
        None => Err("Selected segment is not a leading or trailing edge candidate.".into()),
    }
}
