mod appearance;
mod review_ui;
mod worker_bridge;

use rfd::FileDialog;
use slint::{ComponentHandle, Model, TimerMode, VecModel};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};
use storyteller_application::{ApplicationCommand, ApplicationController};
use storyteller_core::{
    AudioBitrate, AudioCodec, AudioEncoding, AudioReviewPolicy, Job, JobId, JobInputs, JobQueue,
    JobSettings, JobStatus, QueueMove, QueueState, StageStatus, TranscriptionBackend,
};
use worker_bridge::ViewBridge;

slint::include_modules!();

pub(crate) type SharedApplication = Rc<RefCell<ApplicationController>>;

#[derive(Debug, Default)]
struct PendingSources {
    epub: Option<PathBuf>,
    audiobook: Option<PathBuf>,
    output_directory: Option<PathBuf>,
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    appearance::install(
        &ui,
        storyteller_application::recovery_app_root().join("appearance.json"),
    );
    let pending = Rc::new(RefCell::new(PendingSources::default()));
    let app = Rc::new(RefCell::new(ApplicationController::new()));
    let queue_rows = Rc::new(VecModel::<QueueRow>::default());
    let stage_rows = Rc::new(VecModel::<StageRow>::default());
    let detail_stage_rows = Rc::new(VecModel::<StageDetailRow>::default());
    let view_bridge = Rc::new(RefCell::new(ViewBridge::default()));
    let review_ui_controller = review_ui::install_review_ui(&ui, Rc::clone(&app));
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
                update_output_preview(&ui, &pending.borrow());
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
        let pending = Rc::clone(&pending);
        let ui_weak = ui.as_weak();
        ui.on_browse_output(move || {
            let initial = pending.borrow().output_directory.clone().or_else(|| {
                pending
                    .borrow()
                    .epub
                    .as_ref()
                    .and_then(|path| path.parent().map(Path::to_path_buf))
            });
            let mut dialog = FileDialog::new().set_title("Choose output folder");
            if let Some(initial) = initial {
                dialog = dialog.set_directory(initial);
            }
            if let Some(directory) = dialog.pick_folder() {
                pending.borrow_mut().output_directory = Some(directory);
                if let Some(ui) = ui_weak.upgrade() {
                    update_output_preview(&ui, &pending.borrow());
                }
            }
        });
    }
    {
        let pending = Rc::clone(&pending);
        let app = Rc::clone(&app);
        let ui_weak = ui.as_weak();
        ui.on_queue_book(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let result = (|| {
                let backend = selected_transcription_backend(ui.get_transcription_backend_selection())?;
                if !ui.get_runtime_ready() || ui.get_runtime_busy() {
                    return Err("Finish local setup in Settings before starting a book.".into());
                }
                if !app.borrow().snapshot().runtime.status.as_ref().is_some_and(|status| status.ready_for(backend)) {
                    return Err("Finish setup for the selected transcription engine in Settings.".into());
                }
                let sources = pending.borrow();
                let epub_path = sources.epub.clone().ok_or("Choose an EPUB first")?;
                let audiobook_path = sources
                    .audiobook
                    .clone()
                    .ok_or("Choose an audiobook first")?;
                let title = book_title(&epub_path);
                let inputs = JobInputs {
                    output_path: selected_output_path(&sources, &epub_path, &title),
                    title,
                    epub_path,
                    audiobook_path,
                };
                if inputs.output_path.exists() {
                    return Err(format!("An output already exists at {}. Choose another output folder or move that file before starting.", inputs.output_path.display()));
                }
                let settings = JobSettings {
                    transcription_backend: backend,
                    transcription_model: backend.model_name().into(),
                    audio: audio_encoding(
                        ui.get_codec_text().as_str(),
                        ui.get_bitrate_text().as_str(),
                    )?,
                    transcription_workers: if backend == TranscriptionBackend::WhisperCuda { 1 } else { app.borrow().snapshot().runtime.status.as_ref()
                        .ok_or("Wait for the local setup and system scan to finish.")?
                        .workers.resolve_selection(ui.get_transcription_worker_selection())? },
                    audio_review_policy: parse_audio_review_policy(
                        ui.get_unmatched_audio_policy_text().as_str(),
                    )?,
                    ..JobSettings::default()
                };
                Ok::<_, String>(ApplicationCommand::Enqueue { inputs, settings })
            })();
            match result {
                Ok(command) => {
                    if dispatch_command(&ui, &app, command) {
                        ui.set_epub_source_name("".into());
                        ui.set_audio_source_name("".into());
                        *pending.borrow_mut() = PendingSources::default();
                        update_output_preview(&ui, &pending.borrow());
                        ui.set_workspace_page(1);
                    }
                }
                Err(error) => ui.set_status_text(error.into()),
            }
        });
    }

    macro_rules! bind_command {
        ($callback:ident, $command:expr) => {{
            let app = Rc::clone(&app);
            let ui_weak = ui.as_weak();
            ui.$callback(move || {
                if let Some(ui) = ui_weak.upgrade() {
                    dispatch_command(&ui, &app, $command);
                }
            });
        }};
    }
    bind_command!(on_pause_after_book, ApplicationCommand::PauseAfterCurrent);
    {
        let app = Rc::clone(&app);
        let weak = ui.as_weak();
        ui.on_cancel_current(move || {
            if let Some(ui) = weak.upgrade() {
                dispatch_job_command(
                    &ui,
                    &app,
                    ui.get_active_job_id().as_str(),
                    ApplicationCommand::CancelJob,
                );
            }
        });
    }
    bind_command!(on_resume_queue, ApplicationCommand::ResumeQueue);
    {
        let app = Rc::clone(&app);
        let weak = ui.as_weak();
        ui.on_open_output(move |id| {
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let result = parse_job_id(id.as_str()).and_then(|id| {
                let app = app.borrow();
                let snapshot = app.snapshot();
                let job = snapshot.queue.job(id)?;
                if job.status != JobStatus::Completed {
                    return Err("This book has not finished yet.".into());
                }
                open_output_folder(&job.inputs.output_path)
            });
            if let Err(error) = result {
                ui.set_status_text(error.into());
            }
        });
    }
    {
        let app = Rc::clone(&app);
        let ui_weak = ui.as_weak();
        ui.on_continue_after_review(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let id = app.borrow().snapshot().review.job_id;
            if let Some(id) = id {
                dispatch_command(&ui, &app, ApplicationCommand::ContinueWithoutUnmatched(id));
            } else {
                ui.set_status_text("No book is waiting for audio review".into());
            }
        });
    }
    {
        let app = Rc::clone(&app);
        let ui_weak = ui.as_weak();
        ui.on_move_waiting(move |id, up| {
            if let Some(ui) = ui_weak.upgrade() {
                dispatch_job_command(&ui, &app, id.as_str(), |id| {
                    ApplicationCommand::MoveWaiting {
                        id,
                        direction: if up { QueueMove::Up } else { QueueMove::Down },
                    }
                });
            }
        });
    }
    {
        let app = Rc::clone(&app);
        let ui_weak = ui.as_weak();
        ui.on_remove_job(move |id| {
            if let Some(ui) = ui_weak.upgrade() {
                dispatch_job_command(&ui, &app, id.as_str(), ApplicationCommand::Remove);
            }
        });
    }
    {
        let app = Rc::clone(&app);
        let ui_weak = ui.as_weak();
        ui.on_retry_from_start(move |id| {
            if let Some(ui) = ui_weak.upgrade() {
                dispatch_job_command(&ui, &app, id.as_str(), ApplicationCommand::RetryFromScratch);
            }
        });
    }

    let poll_timer = slint::Timer::default();
    {
        let app = Rc::clone(&app);
        let view_bridge = Rc::clone(&view_bridge);
        let queue_rows = Rc::clone(&queue_rows);
        let stage_rows = Rc::clone(&stage_rows);
        let detail_stage_rows = Rc::clone(&detail_stage_rows);
        let review_ui_controller = Rc::clone(&review_ui_controller);
        let ui_weak = ui.as_weak();
        poll_timer.start(TimerMode::Repeated, Duration::from_millis(100), move || {
            view_bridge.borrow_mut().poll(
                &ui_weak,
                &app,
                &queue_rows,
                &stage_rows,
                &detail_stage_rows,
            );
            review_ui::refresh_review_ui(&ui_weak, &app, &review_ui_controller);
        });
    }
    if !app.borrow().snapshot().queue.jobs().is_empty() {
        ui.set_workspace_page(1);
    }
    dispatch_command(&ui, &app, ApplicationCommand::ScanRuntime);
    view_bridge
        .borrow_mut()
        .render(&ui, &app, &queue_rows, &stage_rows, &detail_stage_rows);
    review_ui::refresh_review_ui(&ui.as_weak(), &app, &review_ui_controller);
    let result = ui.run();
    poll_timer.stop();
    app.borrow_mut().shutdown();
    result
}

pub(crate) fn dispatch_command(
    ui: &AppWindow,
    app: &SharedApplication,
    command: ApplicationCommand,
) -> bool {
    match app.borrow_mut().dispatch(command) {
        Ok(()) => true,
        Err(error) => {
            ui.set_status_text(error.into());
            false
        }
    }
}

fn dispatch_job_command(
    ui: &AppWindow,
    app: &SharedApplication,
    id: &str,
    command: impl FnOnce(JobId) -> ApplicationCommand,
) {
    match parse_job_id(id) {
        Ok(id) => {
            dispatch_command(ui, app, command(id));
        }
        Err(error) => ui.set_status_text(error.into()),
    }
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

fn selected_output_path(sources: &PendingSources, epub_path: &Path, title: &str) -> PathBuf {
    let default = output_path(epub_path, title);
    sources.output_directory.as_ref().map_or_else(
        || default.clone(),
        |directory| directory.join(default.file_name().unwrap()),
    )
}
fn update_output_preview(ui: &AppWindow, sources: &PendingSources) {
    if let Some(epub) = &sources.epub {
        let output = selected_output_path(sources, epub, &book_title(epub));
        ui.set_output_name(display_name(&output).into());
        ui.set_output_directory(
            output
                .parent()
                .map_or_else(String::new, |path| path.display().to_string())
                .into(),
        );
    } else {
        ui.set_output_name("Choose an EPUB to see the output filename".into());
        ui.set_output_directory("Saved beside your source EPUB".into());
    }
}
fn open_output_folder(output: &Path) -> Result<(), String> {
    if !output.is_file() {
        return Err(format!(
            "The output has moved or is missing: {}",
            output.display()
        ));
    }
    let directory = output.parent().ok_or("The output folder is unavailable.")?;
    let program = if cfg!(windows) {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(directory)
        .spawn()
        .map_err(|error| format!("Could not open the output folder: {error}"))?;
    Ok(())
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
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

pub(crate) fn selected_transcription_backend(
    selection: i32,
) -> Result<TranscriptionBackend, String> {
    match selection {
        0 => Ok(TranscriptionBackend::Whistle),
        1 => Ok(TranscriptionBackend::WhisperCuda),
        _ => Err("Choose a supported transcription engine in Settings.".into()),
    }
}

pub(crate) fn refresh_main_view(
    ui: &AppWindow,
    queue: &JobQueue,
    queue_rows: &VecModel<QueueRow>,
    stage_rows: &VecModel<StageRow>,
    detail_stage_rows: &VecModel<StageDetailRow>,
) {
    update_model(queue_rows, build_queue_rows(queue));
    ui.set_queue_paused(queue.state() == QueueState::Paused);

    let Some(job) = queue.active_job() else {
        ui.set_active_job_id("".into());
        ui.set_cancel_confirmation(false);
        ui.set_exclude_confirmation(false);
        update_model(stage_rows, Vec::new());
        update_model(detail_stage_rows, Vec::new());
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

    let id = job.id.to_string();
    if ui.get_active_job_id().as_str() != id {
        ui.set_cancel_confirmation(false);
        ui.set_exclude_confirmation(false);
        ui.set_active_job_id(id.into());
    }

    update_model(stage_rows, build_stage_rows(job));
    update_model(detail_stage_rows, build_stage_detail_rows(job));
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
    if needs_review && !ui.get_needs_review() {
        ui.set_workspace_page(1);
    }
    ui.set_needs_review(needs_review);
    if !needs_review {
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

fn update_model<T: Clone + PartialEq + 'static>(model: &VecModel<T>, rows: Vec<T>) {
    if model.row_count() != rows.len() {
        model.set_vec(rows);
        return;
    }
    for (index, row) in rows.into_iter().enumerate() {
        if model.row_data(index).as_ref() != Some(&row) {
            model.set_row_data(index, row);
        }
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
            detail: format!(
                "{} · {} worker{}",
                match job.settings.transcription_backend {
                    TranscriptionBackend::Whistle => "Whistle · CPU",
                    TranscriptionBackend::WhisperCuda => "Whisper Turbo · NVIDIA GPU",
                },
                job.settings.transcription_workers,
                if job.settings.transcription_workers == 1 {
                    ""
                } else {
                    "s"
                }
            )
            .into(),
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
    use storyteller_core::{JobOutcome, LiveMetrics, PipelineStage};

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
    fn custom_output_folder_keeps_the_filename_and_source_paths() {
        let epub = PathBuf::from("books/Example.epub");
        let sources = PendingSources {
            epub: Some(epub.clone()),
            audiobook: Some("audio/Example.m4b".into()),
            output_directory: Some("finished".into()),
        };
        assert_eq!(
            selected_output_path(&sources, &epub, "Example"),
            PathBuf::from("finished/Example (readaloud).epub")
        );
        assert_eq!(sources.epub, Some(epub));
        assert_eq!(sources.audiobook, Some(PathBuf::from("audio/Example.m4b")));
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
            backend: Some("Whistle / native CPU".into()),
            model: Some("Whistle".into()),
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
            "Backend Whistle / native CPU  •  Model Whistle  •  Match 98.4%"
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
