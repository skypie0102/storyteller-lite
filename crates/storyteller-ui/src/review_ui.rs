use crate::{worker_bridge, AppWindow, ReviewCandidateRow};
use slint::{ComponentHandle, VecModel};
use std::{
    cell::RefCell,
    env,
    path::PathBuf,
    process::{Child, Command, Stdio},
    rc::Rc,
};
use storyteller_core::{
    apply_audio_review_decision, assign_manual_graphic_readout, prepared_job_sources,
    read_epub_corpus, review_image_candidates, review_text_candidates, AlignmentDocument,
    AlignmentStatus, AudioReviewClassification, AudioReviewDecision, AudioReviewDecisionSource,
    AudioReviewDestination, AudioReviewEdge, AudioReviewItem, AudioReviewSupplementalPlacement,
    CancellationToken, Job, JobId, JobQueue, JobStatus, PipelineStage,
    DEFAULT_REVIEW_CANDIDATE_LIMIT, DEFAULT_REVIEW_IMAGE_DOCUMENT_LIMIT,
    DEFAULT_REVIEW_IMAGE_LIMIT,
};

const UI_TEXT_CANDIDATE_LIMIT: usize = 4;
const UI_IMAGE_CANDIDATE_LIMIT: usize = 2;
const SEEK_STEP_MS: i64 = 5_000;

pub(crate) struct ReviewUiController {
    selected_index: usize,
    seek_ms: u64,
    candidates: Rc<VecModel<ReviewCandidateRow>>,
    candidate_key: Option<(JobId, usize, String)>,
    preview: Option<Child>,
}

impl ReviewUiController {
    fn new() -> Self {
        Self {
            selected_index: 0,
            seek_ms: 0,
            candidates: Rc::new(VecModel::default()),
            candidate_key: None,
            preview: None,
        }
    }

    fn stop_preview(&mut self) {
        if let Some(mut child) = self.preview.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for ReviewUiController {
    fn drop(&mut self) {
        self.stop_preview();
    }
}

pub(crate) fn install_review_ui(
    ui: &AppWindow,
    queue: Rc<RefCell<JobQueue>>,
) -> Rc<RefCell<ReviewUiController>> {
    let controller = Rc::new(RefCell::new(ReviewUiController::new()));
    ui.set_review_candidates(controller.borrow().candidates.clone().into());

    {
        let controller = Rc::clone(&controller);
        let queue = Rc::clone(&queue);
        let ui_weak = ui.as_weak();
        ui.on_review_previous(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let mut controller = controller.borrow_mut();
            if controller.selected_index > 0 {
                controller.selected_index -= 1;
                controller.seek_ms = 0;
                controller.stop_preview();
            }
            refresh_for_ui(&ui, &queue.borrow(), &mut controller);
        });
    }

    {
        let controller = Rc::clone(&controller);
        let queue = Rc::clone(&queue);
        let ui_weak = ui.as_weak();
        ui.on_review_next(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let mut controller = controller.borrow_mut();
            if let Some(job) = review_job(&queue.borrow()) {
                if let Ok(report) = worker_bridge::load_audio_review_report(job) {
                    if controller.selected_index + 1 < report.unmatched.len() {
                        controller.selected_index += 1;
                        controller.seek_ms = 0;
                        controller.stop_preview();
                    }
                }
            }
            refresh_for_ui(&ui, &queue.borrow(), &mut controller);
        });
    }

    {
        let controller = Rc::clone(&controller);
        let queue = Rc::clone(&queue);
        let ui_weak = ui.as_weak();
        ui.on_review_seek_relative(move |direction| {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let mut controller = controller.borrow_mut();
            if let Some(job) = review_job(&queue.borrow()) {
                if let Ok(report) = worker_bridge::load_audio_review_report(job) {
                    if let Some(item) = report.unmatched.get(controller.selected_index) {
                        let duration = item.audio_end_ms.saturating_sub(item.audio_start_ms);
                        let delta = i64::from(direction).saturating_mul(SEEK_STEP_MS);
                        let next = (controller.seek_ms as i64).saturating_add(delta).max(0) as u64;
                        controller.seek_ms = next.min(duration.saturating_sub(1));
                    }
                }
            }
            controller.stop_preview();
            refresh_for_ui(&ui, &queue.borrow(), &mut controller);
        });
    }

    {
        let controller = Rc::clone(&controller);
        let queue = Rc::clone(&queue);
        let ui_weak = ui.as_weak();
        ui.on_review_play(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let mut controller = controller.borrow_mut();
            controller.stop_preview();
            let result = review_job(&queue.borrow())
                .ok_or_else(|| "No book is waiting for audio review.".to_string())
                .and_then(|job| play_selected(job, &mut controller));
            match result {
                Ok(()) => ui.set_review_preview_status_text("Playing selected audio…".into()),
                Err(error) => ui.set_review_preview_status_text(error.into()),
            }
        });
    }

    {
        let controller = Rc::clone(&controller);
        let ui_weak = ui.as_weak();
        ui.on_review_stop(move || {
            controller.borrow_mut().stop_preview();
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_review_preview_status_text("Preview stopped.".into());
            }
        });
    }

    {
        let controller = Rc::clone(&controller);
        let queue = Rc::clone(&queue);
        let ui_weak = ui.as_weak();
        ui.on_review_assign(move |href, line_index| {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let mut controller = controller.borrow_mut();
            controller.stop_preview();
            let is_graphic = line_index < 0;
            let result = review_job(&queue.borrow())
                .ok_or_else(|| "No book is waiting for audio review.".to_string())
                .and_then(|job| {
                    let report = worker_bridge::load_audio_review_report(job)?;
                    let item = report
                        .unmatched
                        .get(controller.selected_index)
                        .ok_or_else(|| "Selected review segment no longer exists.".to_string())?;
                    if line_index >= 0 {
                        let line_index = usize::try_from(line_index)
                            .map_err(|_| "EPUB text-block index is invalid.".to_string())?;
                        return apply_audio_review_decision(
                            &worker_bridge::audio_review_path(job),
                            &worker_bridge::audio_review_draft_path(job),
                            &item.id,
                            AudioReviewDecision::Assigned {
                                destination: AudioReviewDestination {
                                    href: href.to_string(),
                                    line_index: Some(line_index),
                                    image_href: None,
                                    supplemental: None,
                                },
                                classification: None,
                                source: AudioReviewDecisionSource::Manual,
                            },
                        );
                    }

                    let image_index = image_candidate_index(line_index)?;
                    let image_candidates = load_image_candidates(job, item.alignment_index)?;
                    let candidate = image_candidates
                        .get(image_index)
                        .ok_or_else(|| "Selected EPUB image candidate is stale.".to_string())?;
                    if candidate.document_href != href.as_str() {
                        return Err("Selected EPUB image candidate changed; choose it again.".into());
                    }
                    let workspace = worker_bridge::job_workspace(job);
                    let prepared = prepared_job_sources(job, &workspace)?;
                    let alignment_path = workspace
                        .stage_dir(PipelineStage::Align)
                        .join("alignment.json");
                    let corpus_path = workspace
                        .stage_dir(PipelineStage::Analyze)
                        .join("book-corpus.json");
                    assign_manual_graphic_readout(
                        prepared.epub(),
                        &alignment_path,
                        &corpus_path,
                        &worker_bridge::audio_review_path(job),
                        &worker_bridge::audio_review_draft_path(job),
                        &item.id,
                        &candidate.document_href,
                        &candidate.image_href,
                        &CancellationToken::default(),
                    )
                });
            match result {
                Ok(()) => {
                    ui.set_status_text(
                        if is_graphic {
                            "Image assigned as a Graphic Readout for this audio segment."
                        } else {
                            "Text block assigned to audio segment."
                        }
                        .into(),
                    );
                    select_next_pending(&queue.borrow(), &mut controller);
                }
                Err(error) => ui.set_status_text(error.into()),
            }
            refresh_for_ui(&ui, &queue.borrow(), &mut controller);
        });
    }

    {
        let controller = Rc::clone(&controller);
        let queue = Rc::clone(&queue);
        let ui_weak = ui.as_weak();
        ui.on_review_preserve_edge(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let mut controller = controller.borrow_mut();
            controller.stop_preview();
            let result = review_job(&queue.borrow())
                .ok_or_else(|| "No book is waiting for audio review.".to_string())
                .and_then(|job| {
                    let report = worker_bridge::load_audio_review_report(job)?;
                    let item = report
                        .unmatched
                        .get(controller.selected_index)
                        .ok_or_else(|| "Selected review segment no longer exists.".to_string())?;
                    let (anchor_href, placement, classification) =
                        edge_page_destination(job, item)?;
                    apply_audio_review_decision(
                        &worker_bridge::audio_review_path(job),
                        &worker_bridge::audio_review_draft_path(job),
                        &item.id,
                        AudioReviewDecision::Assigned {
                            destination: AudioReviewDestination {
                                href: anchor_href,
                                line_index: None,
                                image_href: None,
                                supplemental: Some(placement),
                            },
                            classification: Some(classification),
                            source: AudioReviewDecisionSource::Manual,
                        },
                    )
                });
            match result {
                Ok(()) => {
                    ui.set_status_text(
                        "Edge narration will be preserved on a supplemental read-aloud page."
                            .into(),
                    );
                    select_next_pending(&queue.borrow(), &mut controller);
                }
                Err(error) => ui.set_status_text(error.into()),
            }
            refresh_for_ui(&ui, &queue.borrow(), &mut controller);
        });
    }

    {
        let controller = Rc::clone(&controller);
        let queue = Rc::clone(&queue);
        let ui_weak = ui.as_weak();
        ui.on_review_exclude(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let mut controller = controller.borrow_mut();
            controller.stop_preview();
            let result = review_job(&queue.borrow())
                .ok_or_else(|| "No book is waiting for audio review.".to_string())
                .and_then(|job| {
                    let report = worker_bridge::load_audio_review_report(job)?;
                    let item = report
                        .unmatched
                        .get(controller.selected_index)
                        .ok_or_else(|| "Selected review segment no longer exists.".to_string())?;
                    apply_audio_review_decision(
                        &worker_bridge::audio_review_path(job),
                        &worker_bridge::audio_review_draft_path(job),
                        &item.id,
                        AudioReviewDecision::Excluded {
                            reason: "User excluded this unmatched audio segment during review."
                                .into(),
                            source: AudioReviewDecisionSource::Manual,
                        },
                    )
                });
            match result {
                Ok(()) => {
                    ui.set_status_text("Audio segment excluded from synchronization.".into());
                    select_next_pending(&queue.borrow(), &mut controller);
                }
                Err(error) => ui.set_status_text(error.into()),
            }
            refresh_for_ui(&ui, &queue.borrow(), &mut controller);
        });
    }

    {
        let controller = Rc::clone(&controller);
        let queue = Rc::clone(&queue);
        let ui_weak = ui.as_weak();
        ui.on_finish_review(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let mut controller = controller.borrow_mut();
            controller.stop_preview();
            let result = {
                let queue_ref = queue.borrow();
                let job = review_job(&queue_ref)
                    .ok_or_else(|| "No book is waiting for audio review.".to_string());
                job.and_then(|job| {
                    let report = worker_bridge::load_audio_review_report(job)?;
                    if !report.is_complete() {
                        return Err(format!(
                            "Resolve all {} pending review segment(s) before continuing.",
                            report.pending_count()
                        ));
                    }
                    Ok(job.id)
                })
            };
            match result.and_then(|job_id| queue.borrow_mut().resume_after_review(job_id)) {
                Ok(()) => ui.set_status_text("Audio review complete; continuing.".into()),
                Err(error) => ui.set_status_text(error.into()),
            }
            refresh_for_ui(&ui, &queue.borrow(), &mut controller);
        });
    }

    controller
}

pub(crate) fn refresh_review_ui(
    ui_weak: &slint::Weak<AppWindow>,
    queue: &Rc<RefCell<JobQueue>>,
    controller: &Rc<RefCell<ReviewUiController>>,
) {
    let Some(ui) = ui_weak.upgrade() else {
        return;
    };
    refresh_for_ui(&ui, &queue.borrow(), &mut controller.borrow_mut());
}

fn refresh_for_ui(ui: &AppWindow, queue: &JobQueue, controller: &mut ReviewUiController) {
    let Some(job) = review_job(queue) else {
        controller.stop_preview();
        controller.selected_index = 0;
        controller.seek_ms = 0;
        controller.candidate_key = None;
        controller.candidates.set_vec(Vec::new());
        clear_review_properties(ui);
        return;
    };
    let report = match worker_bridge::load_audio_review_report(job) {
        Ok(report) => report,
        Err(error) => {
            controller.candidate_key = None;
            controller.candidates.set_vec(Vec::new());
            ui.set_review_allocator_status_text(
                format!("Review data could not be loaded: {error}").into(),
            );
            return;
        }
    };
    if report.unmatched.is_empty() {
        controller.candidate_key = None;
        controller.candidates.set_vec(Vec::new());
        ui.set_review_item_position_text("No unmatched segments".into());
        ui.set_review_allocator_status_text("Nothing requires manual review.".into());
        ui.set_review_complete(true);
        return;
    }

    controller.selected_index = controller.selected_index.min(report.unmatched.len() - 1);
    let item = &report.unmatched[controller.selected_index];
    let duration = item.audio_end_ms.saturating_sub(item.audio_start_ms);
    controller.seek_ms = controller.seek_ms.min(duration.saturating_sub(1));
    ui.set_review_item_position_text(
        format!(
            "Segment {} of {}",
            controller.selected_index + 1,
            report.unmatched.len()
        )
        .into(),
    );
    ui.set_review_item_time_text(
        format!(
            "{}–{}",
            format_millis(item.audio_start_ms),
            format_millis(item.audio_end_ms)
        )
        .into(),
    );
    ui.set_review_item_transcript_text(item.transcript_text.clone().into());
    ui.set_review_item_decision_text(review_context_text(item).into());
    ui.set_review_seek_text(
        format!(
            "Seek {} / {}",
            format_millis(controller.seek_ms),
            format_millis(duration)
        )
        .into(),
    );
    ui.set_review_can_previous(controller.selected_index > 0);
    ui.set_review_can_next(controller.selected_index + 1 < report.unmatched.len());
    ui.set_review_complete(report.is_complete());
    let preserve_label = match item.edge {
        Some(AudioReviewEdge::Introduction) => "Preserve as Introduction",
        Some(AudioReviewEdge::Credits) => "Preserve as Credits",
        None => "",
    };
    ui.set_review_can_preserve_edge(item.edge.is_some());
    ui.set_review_preserve_edge_text(preserve_label.into());
    ui.set_review_allocator_status_text(if report.pending_count() == 0 {
        "All unmatched segments have durable decisions. Continue when ready.".into()
    } else {
        format!(
            "{} segment(s) still need a decision.",
            report.pending_count()
        )
        .into()
    });

    let candidate_key = (job.id, item.alignment_index, item.id.clone());
    if controller.candidate_key.as_ref() != Some(&candidate_key) {
        match load_candidate_rows(job, item) {
            Ok(candidates) => controller.candidates.set_vec(candidates),
            Err(error) => {
                controller.candidates.set_vec(Vec::new());
                ui.set_review_allocator_status_text(
                    format!("Could not load EPUB candidates: {error}").into(),
                );
            }
        }
        controller.candidate_key = Some(candidate_key);
    }
}

fn clear_review_properties(ui: &AppWindow) {
    ui.set_review_item_position_text("".into());
    ui.set_review_item_time_text("".into());
    ui.set_review_item_transcript_text("".into());
    ui.set_review_item_decision_text("".into());
    ui.set_review_seek_text("".into());
    ui.set_review_allocator_status_text("".into());
    ui.set_review_preview_status_text("".into());
    ui.set_review_can_previous(false);
    ui.set_review_can_next(false);
    ui.set_review_can_preserve_edge(false);
    ui.set_review_preserve_edge_text("".into());
    ui.set_review_complete(false);
}

fn review_job(queue: &JobQueue) -> Option<&Job> {
    queue
        .active_job()
        .filter(|job| job.status == JobStatus::NeedsReview)
}

fn select_next_pending(queue: &JobQueue, controller: &mut ReviewUiController) {
    let Some(job) = review_job(queue) else {
        return;
    };
    let Ok(report) = worker_bridge::load_audio_review_report(job) else {
        return;
    };
    if let Some((index, _)) = report
        .unmatched
        .iter()
        .enumerate()
        .find(|(_, item)| item.decision.is_pending())
    {
        controller.selected_index = index;
        controller.seek_ms = 0;
    }
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
    let workspace = worker_bridge::job_workspace(job);
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

fn load_candidate_rows(job: &Job, item: &AudioReviewItem) -> Result<Vec<ReviewCandidateRow>, String> {
    let mut rows = load_text_candidates(job, item.alignment_index)?
        .into_iter()
        .map(|candidate| ReviewCandidateRow {
            href: candidate.href.into(),
            line_index: i32::try_from(candidate.line_index).unwrap_or(i32::MAX),
            text: candidate.text.into(),
            score: format!("{}%", u32::from(candidate.score_milli) / 10).into(),
        })
        .collect::<Vec<_>>();

    if item.edge.is_none() {
        if let Ok(image_candidates) = load_image_candidates(job, item.alignment_index) {
            for (index, candidate) in image_candidates.into_iter().enumerate() {
                let line_index = i32::try_from(index)
                    .ok()
                    .and_then(|index| index.checked_add(1))
                    .and_then(|index| index.checked_neg())
                    .ok_or_else(|| "Too many EPUB image candidates for the review UI.".to_string())?;
                rows.push(ReviewCandidateRow {
                    href: candidate.document_href.clone().into(),
                    line_index,
                    text: image_candidate_text(&candidate).into(),
                    score: "image".into(),
                });
            }
        }
    }
    Ok(rows)
}

fn load_text_candidates(
    job: &Job,
    alignment_index: usize,
) -> Result<Vec<storyteller_core::AudioReviewTextCandidate>, String> {
    let workspace = worker_bridge::job_workspace(job);
    let alignment_path = workspace
        .stage_dir(PipelineStage::Align)
        .join("alignment.json");
    let corpus_path = workspace
        .stage_dir(PipelineStage::Analyze)
        .join("book-corpus.json");
    let alignment_data = std::fs::read(&alignment_path).map_err(|error| {
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
    let corpus = read_epub_corpus(&corpus_path)?;
    review_text_candidates(
        &alignment,
        &corpus,
        alignment_index,
        UI_TEXT_CANDIDATE_LIMIT.min(DEFAULT_REVIEW_CANDIDATE_LIMIT),
    )
}

fn load_image_candidates(
    job: &Job,
    alignment_index: usize,
) -> Result<Vec<storyteller_core::AudioReviewImageCandidate>, String> {
    let workspace = worker_bridge::job_workspace(job);
    let prepared = prepared_job_sources(job, &workspace)?;
    let alignment_path = workspace
        .stage_dir(PipelineStage::Align)
        .join("alignment.json");
    let corpus_path = workspace
        .stage_dir(PipelineStage::Analyze)
        .join("book-corpus.json");
    let alignment_data = std::fs::read(&alignment_path).map_err(|error| {
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
    let corpus = read_epub_corpus(&corpus_path)?;
    review_image_candidates(
        prepared.epub(),
        &alignment,
        &corpus,
        alignment_index,
        DEFAULT_REVIEW_IMAGE_DOCUMENT_LIMIT,
        UI_IMAGE_CANDIDATE_LIMIT.min(DEFAULT_REVIEW_IMAGE_LIMIT),
        &CancellationToken::default(),
    )
}

fn image_candidate_index(line_index: i32) -> Result<usize, String> {
    let index = line_index
        .checked_neg()
        .and_then(|value| value.checked_sub(1))
        .ok_or_else(|| "EPUB image candidate index is invalid.".to_string())?;
    usize::try_from(index).map_err(|_| "EPUB image candidate index is invalid.".to_string())
}

fn image_candidate_text(candidate: &storyteller_core::AudioReviewImageCandidate) -> String {
    let evidence = candidate
        .embedded_text
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let label = if evidence.is_empty() {
        candidate
            .image_href
            .rsplit('/')
            .next()
            .unwrap_or(candidate.image_href.as_str())
            .to_string()
    } else {
        evidence
    };
    format!("Graphic Readout · {label}")
}

fn play_selected(job: &Job, controller: &mut ReviewUiController) -> Result<(), String> {
    let report = worker_bridge::load_audio_review_report(job)?;
    let item = report
        .unmatched
        .get(controller.selected_index)
        .ok_or_else(|| "Selected review segment no longer exists.".to_string())?;
    let workspace = worker_bridge::job_workspace(job);
    let prepared = prepared_job_sources(job, &workspace)?;
    let play_start = item.audio_start_ms.saturating_add(controller.seek_ms);
    let remaining = item.audio_end_ms.saturating_sub(play_start);
    if remaining == 0 {
        return Err("The preview seek position is at the end of this segment.".into());
    }
    let ffplay = resolve_ffplay().ok_or_else(|| {
        "Audio preview requires ffplay. Install an ffmpeg build that includes ffplay; manual allocation still works without preview.".to_string()
    })?;
    let child = Command::new(ffplay)
        .arg("-nodisp")
        .arg("-autoexit")
        .arg("-loglevel")
        .arg("error")
        .arg("-ss")
        .arg(format_seconds(play_start))
        .arg("-t")
        .arg(format_seconds(remaining))
        .arg(prepared.audiobook())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Could not start ffplay audio preview: {error}"))?;
    controller.preview = Some(child);
    Ok(())
}

fn resolve_ffplay() -> Option<PathBuf> {
    if let Some(path) = env::var_os("STORYTELLER_FFPLAY").map(PathBuf::from) {
        if path.is_file() {
            return Some(path);
        }
    }
    if let Some(ffmpeg) = env::var_os("STORYTELLER_FFMPEG").map(PathBuf::from) {
        if let Some(parent) = ffmpeg.parent() {
            let sibling = parent.join(ffplay_file_name());
            if sibling.is_file() {
                return Some(sibling);
            }
        }
    }
    if let Some(path) = env::var_os("PATH") {
        for directory in env::split_paths(&path) {
            let candidate = directory.join(ffplay_file_name());
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn ffplay_file_name() -> &'static str {
    if cfg!(windows) {
        "ffplay.exe"
    } else {
        "ffplay"
    }
}

fn review_context_text(item: &AudioReviewItem) -> String {
    let decision = decision_text(&item.decision);
    let edge = match item.edge {
        Some(AudioReviewEdge::Introduction) => {
            Some("Smart evidence: leading edge / Introduction candidate")
        }
        Some(AudioReviewEdge::Credits) => Some("Smart evidence: trailing edge / Credits candidate"),
        None => None,
    };
    let silence = item.silence.map(|evidence| {
        format!(
            "Silence evidence: {}% silent at {} dB (minimum gap {:.2}s)",
            evidence.silence_percent(),
            evidence.threshold_db,
            evidence.minimum_silence_ms as f64 / 1000.0
        )
    });
    [Some(decision), edge.map(str::to_string), silence]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" — ")
}

fn decision_text(decision: &AudioReviewDecision) -> String {
    match decision {
        AudioReviewDecision::Pending => {
            "Pending — choose an EPUB text/image candidate or exclude this segment.".into()
        }
        AudioReviewDecision::Assigned {
            destination,
            classification,
            ..
        } => {
            if let Some(image_href) = destination.image_href.as_deref() {
                format!(
                    "Assigned — Graphic Readout {} in {}",
                    image_href, destination.href
                )
            } else if destination.supplemental.is_some() {
                format!(
                    "Assigned — {:?} supplemental page anchored at {}",
                    classification, destination.href
                )
            } else {
                match destination.line_index {
                    Some(line) => format!("Assigned — {} line {}", destination.href, line + 1),
                    None => format!("Assigned — {}", destination.href),
                }
            }
        }
        AudioReviewDecision::Excluded { reason, .. } => format!("Excluded — {reason}"),
    }
}

fn format_seconds(milliseconds: u64) -> String {
    format!("{}.{:03}", milliseconds / 1000, milliseconds % 1000)
}

fn format_millis(milliseconds: u64) -> String {
    let total_seconds = milliseconds / 1000;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}
