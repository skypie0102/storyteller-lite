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
    apply_audio_review_decision, prepared_job_sources, read_epub_corpus, review_text_candidates,
    AlignmentDocument, AudioReviewDecision, AudioReviewDecisionSource, AudioReviewDestination,
    AudioReviewEdge, AudioReviewItem, Job, JobQueue, JobStatus, PipelineStage,
    DEFAULT_REVIEW_CANDIDATE_LIMIT,
};

const UI_CANDIDATE_LIMIT: usize = 4;
const SEEK_STEP_MS: i64 = 5_000;

pub(crate) struct ReviewUiController {
    selected_index: usize,
    seek_ms: u64,
    candidates: Rc<VecModel<ReviewCandidateRow>>,
    preview: Option<Child>,
}

impl ReviewUiController {
    fn new() -> Self {
        Self {
            selected_index: 0,
            seek_ms: 0,
            candidates: Rc::new(VecModel::default()),
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
            let result = review_job(&queue.borrow())
                .ok_or_else(|| "No book is waiting for audio review.".to_string())
                .and_then(|job| {
                    let report = worker_bridge::load_audio_review_report(job)?;
                    let item = report
                        .unmatched
                        .get(controller.selected_index)
                        .ok_or_else(|| "Selected review segment no longer exists.".to_string())?;
                    let line_index = usize::try_from(line_index)
                        .map_err(|_| "EPUB text-block index is invalid.".to_string())?;
                    apply_audio_review_decision(
                        &worker_bridge::audio_review_path(job),
                        &worker_bridge::audio_review_draft_path(job),
                        &item.id,
                        AudioReviewDecision::Assigned {
                            destination: AudioReviewDestination {
                                href: href.to_string(),
                                line_index: Some(line_index),
                                image_href: None,
                            },
                            classification: None,
                            source: AudioReviewDecisionSource::Manual,
                        },
                    )
                });
            match result {
                Ok(()) => {
                    ui.set_status_text("Text block assigned to audio segment.".into());
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
        controller.candidates.set_vec(Vec::new());
        clear_review_properties(ui);
        return;
    };
    let report = match worker_bridge::load_audio_review_report(job) {
        Ok(report) => report,
        Err(error) => {
            controller.candidates.set_vec(Vec::new());
            ui.set_review_allocator_status_text(
                format!("Review data could not be loaded: {error}").into(),
            );
            return;
        }
    };
    if report.unmatched.is_empty() {
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
    ui.set_review_allocator_status_text(if report.pending_count() == 0 {
        "All unmatched segments have durable decisions. Continue when ready.".into()
    } else {
        format!(
            "{} segment(s) still need a decision.",
            report.pending_count()
        )
        .into()
    });

    match load_candidates(job, item.alignment_index) {
        Ok(candidates) => controller.candidates.set_vec(
            candidates
                .into_iter()
                .map(|candidate| ReviewCandidateRow {
                    href: candidate.href.into(),
                    line_index: i32::try_from(candidate.line_index).unwrap_or(i32::MAX),
                    text: candidate.text.into(),
                    score: format!("{}%", u32::from(candidate.score_milli) / 10).into(),
                })
                .collect::<Vec<_>>(),
        ),
        Err(error) => {
            controller.candidates.set_vec(Vec::new());
            ui.set_review_allocator_status_text(
                format!("Could not load text candidates: {error}").into(),
            );
        }
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

fn load_candidates(
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
        UI_CANDIDATE_LIMIT.min(DEFAULT_REVIEW_CANDIDATE_LIMIT),
    )
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
            "Pending — choose a text block or exclude this segment.".into()
        }
        AudioReviewDecision::Assigned { destination, .. } => match destination.line_index {
            Some(line) => format!("Assigned — {} line {}", destination.href, line + 1),
            None => format!("Assigned — {}", destination.href),
        },
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
