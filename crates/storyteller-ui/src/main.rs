mod app_paths;
mod recovery_smoke;
mod review_ui;
mod runtime_import;
mod worker_bridge;

use rfd::FileDialog;
use slint::{ComponentHandle, TimerMode, VecModel};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};
use storyteller_core::{
    AudioBitrate, AudioCodec, AudioEncoding, AudioReviewPolicy, Job, JobId, JobInputs, JobOutcome,
    JobQueue, JobSettings, JobStatus, QueueMove, QueueState, StageStatus,
};
use worker_bridge::{accept_audio_review_exclusion, load_audio_review_report, WorkerBridge};

slint::include_modules!();

#[derive(Debug, Default)]
struct PendingSources {
    epub: Option<PathBuf>,
    audiobook: Option<PathBuf>,
}

fn main() -> Result<(), slint::PlatformError> {
    if std::env::args_os().any(|argument| argument == "--recovery-smoke") {
        if let Err(error) = recovery_smoke::run() {
            eprintln!("Packaged recovery smoke failed: {error}");
            std::process::exit(2);
        }
        return Ok(());
    }

    let ui = AppWindow::new()?;
    let pending = Rc::new(RefCell::new(PendingSources::default()));
    let queue = Rc::new(RefCell::new(JobQueue::default()));
    let queue_rows = Rc::new(VecModel::<QueueRow>::default());
    let stage_rows = Rc::new(VecModel::<StageRow>::default());
    let detail_stage_rows = Rc::new(VecModel::<StageDetailRow>::default());
    let worker_bridge = Rc::new(RefCell::new(WorkerBridge::default()));
    let review_ui_controller = review_ui::install_review_ui(&ui, Rc::clone(&queue));
    ui.set_queue_rows(queue_rows.clone().into());
    ui.set_active_stages(stage_rows.clone().into());
    ui.set_active_stage_details(detail_stage_rows.clone().into());

    {
        let pending = Rc::clone(&pending);
        let ui_weak = ui.as_weak();
        ui.on_browse_epub(move || {
            let Some(path) = FileDialog::new()
                .set_title("Choose source EPUB")
                .add_filter("EPUB", &["epub"])
                .pick_file()
            else {
                return;
            };
            let label = display_name(&path);
            pending.borrow_mut().epub = Some(path);
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_epub_source_name(label.into());
                ui.set_status_text("EPUB selected".into());
            }
        });
    }

    {
        let pending = Rc::clone(&pending);
        let ui_weak = ui.as_weak();
        ui.on_browse_audio(move || {
            let Some(path) = FileDialog::new()
                .set_title("Choose audiobook")
                .add_filter(
                    "Audiobook audio",
                    &["m4b", "m4a", "mp3", "opus", "ogg", "aac", "flac", "wav"],
                )
                .pick_file()
            else {
                return;
            };
            let label = display_name(&path);
            pending.borrow_mut().audiobook = Some(path);
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_audio_source_name(label.into());
                ui.set_status_text("Audiobook selected".into());
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        ui.on_import_whisper_archive(move || {
            let Some(path) = FileDialog::new()
                .set_title("Import whisper.cpp archive")
                .add_filter("whisper.cpp archive", &["zip", "tgz", "gz"])
                .pick_file()
            else {
                return;
            };
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            ui.set_runtime_busy(true);
            ui.set_runtime_install_status_text(
                format!("Importing and verifying {}…", display_name(&path)).into(),
            );
            match runtime_import::import_whisper_archive(&path) {
                Ok(cli) => {
                    std::env::set_var("STORYTELLER_WHISPER", &cli);
                    ui.set_runtime_install_status_text(
                        format!("Imported and verified whisper.cpp at {}", cli.display()).into(),
                    );
                    ui.set_runtime_refresh_requested(true);
                }
                Err(error) => {
                    ui.set_runtime_install_status_text(format!("Import failed: {error}").into());
                }
            }
            ui.set_runtime_busy(false);
        });
    }

    {
        let pending = Rc::clone(&pending);
        let queue = Rc::clone(&queue);
        let queue_rows = Rc::clone(&queue_rows);
        let stage_rows = Rc::clone(&stage_rows);
        let detail_stage_rows = Rc::clone(&detail_stage_rows);
        let ui_weak = ui.as_weak();
        ui.on_queue_book(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };

            let (epub_path, audiobook_path) = {
                let pending = pending.borrow();
                let Some(epub_path) = pending.epub.clone() else {
                    ui.set_status_text("Choose an EPUB first".into());
                    return;
                };
                let Some(audiobook_path) = pending.audiobook.clone() else {
                    ui.set_status_text("Choose an audiobook first".into());
                    return;
                };
                (epub_path, audiobook_path)
            };

            let codec = ui.get_codec_text().to_string();
            let bitrate = ui.get_bitrate_text().to_string();
            let audio = match audio_encoding(&codec, &bitrate) {
                Ok(audio) => audio,
                Err(error) => {
                    ui.set_status_text(error.into());
                    return;
                }
            };
            let workers_text = ui.get_whisper_workers_text().to_string();
            let whisper_workers = match parse_whisper_workers(&workers_text) {
                Ok(workers) => workers,
                Err(error) => {
                    ui.set_status_text(error.into());
                    return;
                }
            };
            let audio_review_policy =
                match parse_audio_review_policy(ui.get_unmatched_audio_policy_text().as_str()) {
                    Ok(policy) => policy,
                    Err(error) => {
                        ui.set_status_text(error.into());
                        return;
                    }
                };
            let title = book_title(&epub_path);
            let output_path = output_path(&epub_path, &title);
            let settings = JobSettings {
                audio,
                audio_review_policy,
                whisper_workers,
                ..JobSettings::default()
            };
            let job = match Job::new(
                JobInputs {
                    title: title.clone(),
                    epub_path,
                    audiobook_path,
                    output_path,
                },
                settings,
            ) {
                Ok(job) => job,
                Err(error) => {
                    ui.set_status_text(error.into());
                    return;
                }
            };

            let start_result = {
                let mut queue = queue.borrow_mut();
                queue.enqueue(job);
                queue.start_next()
            };
            if let Err(error) = start_result {
                ui.set_status_text(error.into());
                return;
            }

            refresh_main_view(
                &ui,
                &queue.borrow(),
                &queue_rows,
                &stage_rows,
                &detail_stage_rows,
            );
            ui.set_epub_source_name("".into());
            ui.set_audio_source_name("".into());
            *pending.borrow_mut() = PendingSources::default();
        });
    }

    {
        let queue = Rc::clone(&queue);
        let queue_rows = Rc::clone(&queue_rows);
        let stage_rows = Rc::clone(&stage_rows);
        let detail_stage_rows = Rc::clone(&detail_stage_rows);
        let ui_weak = ui.as_weak();
        ui.on_pause_after_book(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            queue.borrow_mut().request_pause_after_current();
            refresh_main_view(
                &ui,
                &queue.borrow(),
                &queue_rows,
                &stage_rows,
                &detail_stage_rows,
            );
        });
    }

    {
        let worker_bridge = Rc::clone(&worker_bridge);
        let queue = Rc::clone(&queue);
        let queue_rows = Rc::clone(&queue_rows);
        let stage_rows = Rc::clone(&stage_rows);
        let detail_stage_rows = Rc::clone(&detail_stage_rows);
        let ui_weak = ui.as_weak();
        ui.on_cancel_current(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            if worker_bridge.borrow().request_cancellation() {
                ui.set_status_text("Cancellation requested".into());
                return;
            }

            let review_job = {
                queue
                    .borrow()
                    .active_job()
                    .filter(|job| job.status == JobStatus::NeedsReview)
                    .map(|job| (job.id, elapsed_stage_seconds(job)))
            };
            let Some((job_id, runtime_seconds)) = review_job else {
                ui.set_status_text("Active worker is not ready to cancel".into());
                return;
            };
            let result = {
                let mut queue = queue.borrow_mut();
                queue
                    .finish(job_id, JobOutcome::Cancelled, runtime_seconds)
                    .and_then(|()| queue.start_next().map(|_| ()))
            };
            if let Err(error) = result {
                ui.set_status_text(error.into());
                return;
            }
            refresh_main_view(
                &ui,
                &queue.borrow(),
                &queue_rows,
                &stage_rows,
                &detail_stage_rows,
            );
            ui.set_status_text("Cancelled".into());
        });
    }

    {
        let queue = Rc::clone(&queue);
        let queue_rows = Rc::clone(&queue_rows);
        let stage_rows = Rc::clone(&stage_rows);
        let detail_stage_rows = Rc::clone(&detail_stage_rows);
        let ui_weak = ui.as_weak();
        ui.on_continue_after_review(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let review_job = {
                queue
                    .borrow()
                    .active_job()
                    .filter(|job| job.status == JobStatus::NeedsReview)
                    .cloned()
            };
            let Some(job) = review_job else {
                ui.set_status_text("No book is waiting for audio review".into());
                return;
            };
            if let Err(error) = accept_audio_review_exclusion(&job) {
                ui.set_status_text(error.into());
                return;
            }
            if let Err(error) = queue.borrow_mut().resume_after_review(job.id) {
                ui.set_status_text(error.into());
                return;
            }
            refresh_main_view(
                &ui,
                &queue.borrow(),
                &queue_rows,
                &stage_rows,
                &detail_stage_rows,
            );
            ui.set_status_text("Review accepted; continuing".into());
        });
    }

    {
        let queue = Rc::clone(&queue);
        let queue_rows = Rc::clone(&queue_rows);
        let stage_rows = Rc::clone(&stage_rows);
        let detail_stage_rows = Rc::clone(&detail_stage_rows);
        let ui_weak = ui.as_weak();
        ui.on_resume_queue(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let start_result = {
                let mut queue = queue.borrow_mut();
                queue.resume();
                queue.start_next()
            };
            if let Err(error) = start_result {
                ui.set_status_text(error.into());
                return;
            }
            refresh_main_view(
                &ui,
                &queue.borrow(),
                &queue_rows,
                &stage_rows,
                &detail_stage_rows,
            );
            if queue.borrow().active_job().is_none() {
                ui.set_status_text("Queue resumed".into());
            }
        });
    }

    {
        let queue = Rc::clone(&queue);
        let queue_rows = Rc::clone(&queue_rows);
        let stage_rows = Rc::clone(&stage_rows);
        let detail_stage_rows = Rc::clone(&detail_stage_rows);
        let ui_weak = ui.as_weak();
        ui.on_move_waiting(move |id, up| {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let id = match parse_job_id(id.as_str()) {
                Ok(id) => id,
                Err(error) => {
                    ui.set_status_text(error.into());
                    return;
                }
            };
            let direction = if up { QueueMove::Up } else { QueueMove::Down };
            if let Err(error) = queue.borrow_mut().move_waiting(id, direction) {
                ui.set_status_text(error.into());
                return;
            }
            refresh_main_view(
                &ui,
                &queue.borrow(),
                &queue_rows,
                &stage_rows,
                &detail_stage_rows,
            );
        });
    }

    {
        let queue = Rc::clone(&queue);
        let queue_rows = Rc::clone(&queue_rows);
        let stage_rows = Rc::clone(&stage_rows);
        let detail_stage_rows = Rc::clone(&detail_stage_rows);
        let ui_weak = ui.as_weak();
        ui.on_remove_job(move |id| {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let id = match parse_job_id(id.as_str()) {
                Ok(id) => id,
                Err(error) => {
                    ui.set_status_text(error.into());
                    return;
                }
            };
            if let Err(error) = queue.borrow_mut().remove(id) {
                ui.set_status_text(error.into());
                return;
            }
            refresh_main_view(
                &ui,
                &queue.borrow(),
                &queue_rows,
                &stage_rows,
                &detail_stage_rows,
            );
        });
    }

    {
        let queue = Rc::clone(&queue);
        let queue_rows = Rc::clone(&queue_rows);
        let stage_rows = Rc::clone(&stage_rows);
        let detail_stage_rows = Rc::clone(&detail_stage_rows);
        let ui_weak = ui.as_weak();
        ui.on_retry_from_start(move |id| {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let id = match parse_job_id(id.as_str()) {
                Ok(id) => id,
                Err(error) => {
                    ui.set_status_text(error.into());
                    return;
                }
            };
            let retry_result = {
                let mut queue = queue.borrow_mut();
                queue
                    .retry_from_scratch(id)
                    .and_then(|()| queue.start_next().map(|_| ()))
            };
            if let Err(error) = retry_result {
                ui.set_status_text(error.into());
                return;
            }
            refresh_main_view(
                &ui,
                &queue.borrow(),
                &queue_rows,
                &stage_rows,
                &detail_stage_rows,
            );
        });
    }

    ui.on_open_settings(|| {
        println!("Settings requested");
    });

    let poll_timer = slint::Timer::default();
    {
        let worker_bridge = Rc::clone(&worker_bridge);
        let queue = Rc::clone(&queue);
        let queue_rows = Rc::clone(&queue_rows);
        let stage_rows = Rc::clone(&stage_rows);
        let detail_stage_rows = Rc::clone(&detail_stage_rows);
        let review_ui_controller = Rc::clone(&review_ui_controller);
        let ui_weak = ui.as_weak();
        poll_timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
            worker_bridge.borrow_mut().poll(
                &ui_weak,
                &queue,
                &queue_rows,
                &stage_rows,
                &detail_stage_rows,
            );
            review_ui::refresh_review_ui(&ui_weak, &queue, &review_ui_controller);
        });
    }

    refresh_main_view(
        &ui,
        &queue.borrow(),
        &queue_rows,
        &stage_rows,
        &detail_stage_rows,
    );
    review_ui::refresh_review_ui(&ui.as_weak(), &queue, &review_ui_controller);
    let result = ui.run();
    poll_timer.stop();
    result
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn book_title(epub_path: &Path) -> String {
    epub_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Untitled Book".into())
}

fn output_path(epub_path: &Path, title: &str) -> PathBuf {
    epub_path.with_file_name(format!("{title} (readaloud).epub"))
}

fn parse_job_id(value: &str) -> Result<JobId, String> {
    value
        .parse::<JobId>()
        .map_err(|error| format!("Queue job identifier is invalid: {error}"))
}

fn audio_encoding(codec: &str, bitrate: &str) -> Result<AudioEncoding, String> {
    let codec = match codec {
        "Copy" => return AudioEncoding::new(AudioCodec::Copy, None),
        "Opus" => AudioCodec::Opus,
        "AAC" => AudioCodec::Aac,
        value => return Err(format!("Unsupported audio codec: {value}")),
    };
    let bitrate = match bitrate {
        "32K" => AudioBitrate::Kbps32,
        "64K" => AudioBitrate::Kbps64,
        "96K" => AudioBitrate::Kbps96,
        value => return Err(format!("Unsupported audio bitrate: {value}")),
    };
    AudioEncoding::new(codec, Some(bitrate))
}

fn parse_audio_review_policy(value: &str) -> Result<AudioReviewPolicy, String> {
    match value.trim() {
        "Smart" => Ok(AudioReviewPolicy::Smart),
        "Review all" => Ok(AudioReviewPolicy::ReviewAll),
        value => Err(format!("Unsupported unmatched-audio policy: {value}")),
    }
}

fn parse_whisper_workers(value: &str) -> Result<usize, String> {
    let workers = value
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("Invalid Whisper worker count: {value}"))?;
    if !(1..=4).contains(&workers) {
        return Err("Whisper workers must be between 1 and 4.".into());
    }
    Ok(workers)
}

pub(crate) fn refresh_main_view(
    ui: &AppWindow,
    queue: &JobQueue,
    queue_rows: &VecModel<QueueRow>,
    stage_rows: &VecModel<StageRow>,
    detail_stage_rows: &VecModel<StageDetailRow>,
) {
    queue_rows.set_vec(build_queue_rows(queue));
    ui.set_queue_paused(queue.state() == QueueState::Paused);

    let Some(job) = queue.active_job() else {
        stage_rows.set_vec(Vec::new());
        detail_stage_rows.set_vec(Vec::new());
        ui.set_has_active_job(false);
        ui.set_active_title("".into());
        ui.set_active_overall_progress(0.0);
        ui.set_active_progress_text("0%".into());
        ui.set_active_activity_text("".into());
        ui.set_active_context_text("".into());
        ui.set_active_timing_text("".into());
        ui.set_active_metrics_text("".into());
        ui.set_pause_action_text("Pause after book".into());
        ui.set_needs_review(false);
        ui.set_review_summary_text("".into());
        ui.set_review_preview_text("".into());
        return;
    };

    stage_rows.set_vec(build_stage_rows(job));
    detail_stage_rows.set_vec(build_stage_detail_rows(job));
    ui.set_has_active_job(true);
    ui.set_active_title(job.inputs.title.clone().into());
    ui.set_active_overall_progress(job.progress.overall_percent() as f32 / 100.0);
    ui.set_active_progress_text(format!("{}%", job.progress.overall_percent()).into());
    ui.set_active_activity_text(active_activity(job).into());
    ui.set_active_context_text(active_context(job).into());
    ui.set_active_timing_text(active_timing(job).into());
    ui.set_active_metrics_text(active_metrics(job).into());
    ui.set_pause_action_text(
        if queue.pause_after_current_requested() {
            "Pause requested"
        } else {
            "Pause after book"
        }
        .into(),
    );

    let needs_review = job.status == JobStatus::NeedsReview;
    ui.set_needs_review(needs_review);
    if needs_review {
        let (summary, preview) = review_text(job);
        ui.set_review_summary_text(summary.into());
        ui.set_review_preview_text(preview.into());
    } else {
        ui.set_review_summary_text("".into());
        ui.set_review_preview_text("".into());
    }
    ui.set_status_text(
        match job.status {
            JobStatus::NeedsReview => "Needs Review",
            _ => "Processing",
        }
        .into(),
    );
}

fn review_text(job: &Job) -> (String, String) {
    match load_audio_review_report(job) {
        Ok(report) => {
            let count = report.unmatched.len();
            let summary = format!(
                "{count} unmatched audio segment{} will be excluded from synchronization if you continue. Match {:.1}%.",
                if count == 1 { "" } else { "s" },
                report.match_percent
            );
            let mut previews = report
                .unmatched
                .iter()
                .take(4)
                .map(|item| {
                    format!(
                        "{}–{}  {}",
                        format_millis(item.audio_start_ms),
                        format_millis(item.audio_end_ms),
                        item.transcript_text
                    )
                })
                .collect::<Vec<_>>();
            if count > previews.len() {
                previews.push(format!("…and {} more", count - previews.len()));
            }
            (summary, previews.join("\n"))
        }
        Err(error) => (
            format!("Review data could not be loaded: {error}"),
            String::new(),
        ),
    }
}

fn build_queue_rows(queue: &JobQueue) -> Vec<QueueRow> {
    let waiting = queue
        .jobs()
        .iter()
        .filter(|job| job.status == JobStatus::Waiting)
        .collect::<Vec<_>>();
    let waiting_count = waiting.len();
    let mut rows = waiting
        .into_iter()
        .enumerate()
        .map(|(index, job)| QueueRow {
            id: job.id.to_string().into(),
            position: (index + 1).to_string().into(),
            title: job.inputs.title.clone().into(),
            status: "Waiting".into(),
            detail: "".into(),
            waiting: true,
            retryable: false,
            can_move_up: index > 0,
            can_move_down: index + 1 < waiting_count,
        })
        .collect::<Vec<_>>();

    rows.extend(
        queue
            .jobs()
            .iter()
            .rev()
            .filter(|job| job.status.is_terminal())
            .take(3)
            .map(|job| QueueRow {
                id: job.id.to_string().into(),
                position: "".into(),
                title: job.inputs.title.clone().into(),
                status: terminal_status(job.status).into(),
                detail: terminal_detail(job).into(),
                waiting: false,
                retryable: matches!(job.status, JobStatus::Failed | JobStatus::Cancelled),
                can_move_up: false,
                can_move_down: false,
            }),
    );

    rows
}

fn terminal_status(status: JobStatus) -> &'static str {
    match status {
        JobStatus::Completed => "Completed",
        JobStatus::Failed => "Failed",
        JobStatus::Cancelled => "Cancelled",
        _ => "",
    }
}

fn terminal_detail(job: &Job) -> String {
    if let Some(error) = job
        .last_error
        .as_deref()
        .filter(|error| !error.trim().is_empty())
    {
        return error.to_string();
    }
    if job.runtime_seconds > 0 {
        return format!("Runtime {}", format_duration(job.runtime_seconds));
    }
    String::new()
}

fn build_stage_rows(job: &Job) -> Vec<StageRow> {
    job.progress
        .stages()
        .iter()
        .map(|stage| StageRow {
            label: stage.stage.label().into(),
            state: stage_state(stage.status).into(),
        })
        .collect()
}

fn build_stage_detail_rows(job: &Job) -> Vec<StageDetailRow> {
    job.progress
        .stages()
        .iter()
        .map(|stage| StageDetailRow {
            label: stage.stage.label().into(),
            state: stage_state(stage.status).into(),
            elapsed: stage
                .elapsed_seconds
                .map(format_duration)
                .unwrap_or_default()
                .into(),
            activity: if stage.status == StageStatus::Running {
                job.progress.current_activity().into()
            } else {
                "".into()
            },
        })
        .collect()
}

fn stage_state(status: StageStatus) -> &'static str {
    match status {
        StageStatus::Pending => "Pending",
        StageStatus::Running => "Active",
        StageStatus::Completed => "Done",
        StageStatus::Cached => "Cached",
        StageStatus::Skipped => "Skipped",
        StageStatus::Failed => "Failed",
    }
}

fn active_activity(job: &Job) -> String {
    if job.progress.current_stage().is_none() && job.progress.current_activity() == "Waiting" {
        "Waiting for first pipeline stage".into()
    } else {
        job.progress.current_activity().to_owned()
    }
}

fn active_context(job: &Job) -> String {
    let metrics = job.progress.metrics();
    let mut parts = Vec::new();

    if let (Some(current), Some(total)) = (metrics.current_item, metrics.total_items) {
        if total > 0 {
            parts.push(format!("Item {current} of {total}"));
        }
    }

    if let (Some(processed), Some(total)) =
        (metrics.processed_audio_seconds, metrics.total_audio_seconds)
    {
        if processed.is_finite() && total.is_finite() && total > 0.0 {
            parts.push(format!(
                "Audio {} / {}",
                format_duration(processed.max(0.0) as u64),
                format_duration(total.max(0.0) as u64)
            ));
        }
    }

    parts.join("  •  ")
}

fn active_timing(job: &Job) -> String {
    let metrics = job.progress.metrics();
    let mut parts = Vec::new();
    let elapsed_seconds = elapsed_stage_seconds(job);

    if elapsed_seconds > 0 {
        parts.push(format!("Elapsed {}", format_duration(elapsed_seconds)));
    }
    if let Some(eta_seconds) = metrics.eta_seconds {
        parts.push(format!("ETA ~{}", format_duration(eta_seconds)));
    }
    if let Some(speed) = metrics
        .speed_factor
        .filter(|speed| speed.is_finite() && *speed > 0.0)
    {
        parts.push(format!("Speed {speed:.1}x realtime"));
    }

    parts.join("  •  ")
}

fn active_metrics(job: &Job) -> String {
    let metrics = job.progress.metrics();
    let mut parts = Vec::new();

    if let Some(backend) = metrics
        .backend
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(format!("Backend {backend}"));
    }
    if let Some(model) = metrics
        .model
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        parts.push(format!("Model {model}"));
    }
    if let Some(percent) = metrics
        .match_percent
        .filter(|percent| percent.is_finite() && *percent >= 0.0)
    {
        parts.push(format!("Match {percent:.1}%"));
    }

    parts.join("  •  ")
}

fn elapsed_stage_seconds(job: &Job) -> u64 {
    job.progress
        .stages()
        .iter()
        .filter_map(|stage| stage.elapsed_seconds)
        .sum()
}

fn format_duration(total_seconds: u64) -> String {
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn format_millis(total_millis: u64) -> String {
    let total_seconds = total_millis / 1000;
    let millis = total_millis % 1000;
    format!("{}.{millis:03}", format_duration(total_seconds))
}

#[cfg(test)]
mod tests {
    use super::*;
    use storyteller_core::{LiveMetrics, PipelineStage};

    fn sample_job(title: &str) -> Job {
        Job::new(
            JobInputs {
                title: title.into(),
                epub_path: format!("{title}.epub").into(),
                audiobook_path: format!("{title}.m4b").into(),
                output_path: format!("{title} (readaloud).epub").into(),
            },
            JobSettings::default(),
        )
        .unwrap()
    }

    #[test]
    fn ui_encoding_values_map_to_core_settings() {
        assert_eq!(
            audio_encoding("Copy", "96K").unwrap(),
            AudioEncoding::copy()
        );
        assert_eq!(
            audio_encoding("Opus", "64K").unwrap(),
            AudioEncoding::new(AudioCodec::Opus, Some(AudioBitrate::Kbps64)).unwrap()
        );
        assert!(audio_encoding("MP3", "64K").is_err());
    }

    #[test]
    fn whisper_worker_values_are_bounded() {
        assert_eq!(parse_whisper_workers("1").unwrap(), 1);
        assert_eq!(parse_whisper_workers("4").unwrap(), 4);
        assert!(parse_whisper_workers("0").is_err());
        assert!(parse_whisper_workers("5").is_err());
    }

    #[test]
    fn output_stays_next_to_source_without_replacing_it() {
        let source = Path::new("books/The Example Novel.epub");
        let output = output_path(source, &book_title(source));
        assert_eq!(
            output,
            PathBuf::from("books/The Example Novel (readaloud).epub")
        );
        assert_ne!(source, output);
    }

    #[test]
    fn active_progress_text_uses_only_real_core_metrics() {
        let mut job = sample_job("Metrics");
        job.start().unwrap();
        job.progress
            .start_stage(PipelineStage::Prepare, "Preparing sources")
            .unwrap();
        job.progress.set_current_stage_percent(100).unwrap();
        job.progress
            .complete_stage(PipelineStage::Prepare, 8)
            .unwrap();
        job.progress
            .start_stage(PipelineStage::Analyze, "Transcribing audio")
            .unwrap();
        job.progress.set_metrics(LiveMetrics {
            processed_audio_seconds: Some(60.0),
            total_audio_seconds: Some(3600.0),
            current_item: Some(2),
            total_items: Some(12),
            speed_factor: Some(22.4),
            eta_seconds: Some(410),
            match_percent: Some(98.4),
            backend: Some("CUDA".into()),
            model: Some("large-v3-turbo".into()),
        });

        assert_eq!(
            active_context(&job),
            "Item 2 of 12  •  Audio 1:00 / 1:00:00"
        );
        assert_eq!(
            active_timing(&job),
            "Elapsed 0:08  •  ETA ~6:50  •  Speed 22.4x realtime"
        );
        assert_eq!(
            active_metrics(&job),
            "Backend CUDA  •  Model large-v3-turbo  •  Match 98.4%"
        );
    }

    #[test]
    fn terminal_rows_keep_recent_failure_details_visible() {
        let mut queue = JobQueue::default();
        let id = queue.enqueue(sample_job("Recent"));
        queue.start_next().unwrap();
        queue
            .finish(id, JobOutcome::Failed("boom".into()), 4)
            .unwrap();
        let rows = build_queue_rows(&queue);
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].waiting);
        assert!(rows[0].retryable);
        assert_eq!(rows[0].status.as_str(), "Failed");
        assert_eq!(rows[0].detail.as_str(), "boom");
    }

    #[test]
    fn millisecond_review_ranges_are_not_rounded_to_fake_seconds() {
        assert_eq!(format_millis(65_432), "1:05.432");
    }
}
