#[path = "pipeline_backend.rs"]
mod pipeline_backend;
#[path = "runtime_setup.rs"]
mod runtime_setup;

use crate::{refresh_main_view, AppWindow, QueueRow, StageDetailRow, StageRow};
use runtime_setup::{
    configure_runtime_environment, detect_runtime, install_missing_dependencies, RuntimeStatus,
};
use slint::VecModel;
use std::{
    cell::RefCell,
    rc::Rc,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread::{self, JoinHandle},
};
use storyteller_core::{
    accept_unmatched_audio_exclusion, read_audio_review_report, AudioReviewReport, Job, JobOutcome,
    JobQueue, JobStatus, PipelineRunState, PipelineStage, PipelineWorkerHandle,
};

enum RuntimeInstallEvent {
    Progress(String),
    Finished(Result<RuntimeStatus, String>),
}

struct RuntimeInstallState {
    receiver: Receiver<RuntimeInstallEvent>,
    handle: Option<JoinHandle<()>>,
}

#[derive(Default)]
pub(crate) struct WorkerBridge {
    worker: Option<PipelineWorkerHandle>,
    runtime_install: Option<RuntimeInstallState>,
    runtime_settings_was_open: bool,
}

impl WorkerBridge {
    pub(crate) fn request_cancellation(&self) -> bool {
        let Some(worker) = self.worker.as_ref() else {
            return false;
        };
        worker.request_cancellation();
        true
    }

    pub(crate) fn poll(
        &mut self,
        ui_weak: &slint::Weak<AppWindow>,
        queue: &Rc<RefCell<JobQueue>>,
        queue_rows: &Rc<VecModel<QueueRow>>,
        stage_rows: &Rc<VecModel<StageRow>>,
        detail_stage_rows: &Rc<VecModel<StageDetailRow>>,
    ) {
        self.poll_runtime_setup(ui_weak);

        if self.worker.is_none() {
            if let Some(status) = self.start_active_worker(queue) {
                refresh_if_open(
                    ui_weak,
                    queue,
                    queue_rows,
                    stage_rows,
                    detail_stage_rows,
                    Some(status),
                );
            }
        }

        let Some(worker) = self.worker.as_ref() else {
            return;
        };

        let snapshots = worker.drain_updates();
        let finished = worker.is_finished();
        let changed = !snapshots.is_empty();
        if changed {
            let mut queue = queue.borrow_mut();
            for snapshot in snapshots {
                if queue.reconcile_worker_snapshot(snapshot).is_err() {
                    break;
                }
            }
        }

        if !finished {
            if changed {
                refresh_if_open(
                    ui_weak,
                    queue,
                    queue_rows,
                    stage_rows,
                    detail_stage_rows,
                    None,
                );
            }
            return;
        }

        let Some(worker) = self.worker.take() else {
            return;
        };
        let terminal_status = match worker.join() {
            Ok(result) => {
                let final_job = result.job;
                if queue
                    .borrow_mut()
                    .reconcile_worker_snapshot(final_job)
                    .is_err()
                {
                    Some("Processing finished, but the queue item was no longer available.".into())
                } else {
                    match result.run_result {
                        Ok(PipelineRunState::Completed) => Some("Finished".into()),
                        Ok(PipelineRunState::Cancelled) => Some("Cancelled".into()),
                        Ok(PipelineRunState::Failed(error)) => Some(format!("Failed: {error}")),
                        Ok(PipelineRunState::WaitingForResources(stage)) => {
                            Some(format!("Waiting for resources: {}", stage.label()))
                        }
                        Ok(PipelineRunState::NeedsReview(stage)) => {
                            Some(format!("Needs review: {}", stage.label()))
                        }
                        Err(error) => Some(format!("Processing stopped: {error}")),
                    }
                }
            }
            Err(error) => {
                let active_job_id = { queue.borrow().active_job().map(|job| job.id) };
                if let Some(job_id) = active_job_id {
                    let _ = queue
                        .borrow_mut()
                        .finish(job_id, JobOutcome::Failed(error.clone()), 0);
                }
                Some(format!("Failed: {error}"))
            }
        };

        let status_override = match self.start_next_worker(queue) {
            Ok(true) => None,
            Ok(false) => terminal_status,
            Err(status) => Some(status),
        };

        refresh_if_open(
            ui_weak,
            queue,
            queue_rows,
            stage_rows,
            detail_stage_rows,
            status_override,
        );
    }

    fn poll_runtime_setup(&mut self, ui_weak: &slint::Weak<AppWindow>) {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };

        let settings_open = ui.get_settings_open();
        if settings_open && !self.runtime_settings_was_open {
            ui.set_runtime_refresh_requested(true);
        }
        self.runtime_settings_was_open = settings_open;

        if ui.get_runtime_refresh_requested() && self.runtime_install.is_none() {
            ui.set_runtime_refresh_requested(false);
            let status = detect_runtime();
            apply_runtime_status(&ui, &status);
            ui.set_runtime_install_status_text("Scan complete.".into());
        }

        if ui.get_runtime_install_requested() && self.runtime_install.is_none() {
            ui.set_runtime_install_requested(false);
            let status = detect_runtime();
            apply_runtime_status(&ui, &status);
            if status.ready() {
                ui.set_runtime_install_status_text(
                    "All runtime dependencies are already ready.".into(),
                );
            } else {
                ui.set_runtime_busy(true);
                ui.set_runtime_install_status_text("Preparing dependency download…".into());
                let (sender, receiver) = mpsc::channel();
                let handle = thread::spawn(move || {
                    let result = install_missing_dependencies(&status, |message| {
                        let _ = sender.send(RuntimeInstallEvent::Progress(message));
                    });
                    let result = result.map(|()| detect_runtime());
                    let _ = sender.send(RuntimeInstallEvent::Finished(result));
                });
                self.runtime_install = Some(RuntimeInstallState {
                    receiver,
                    handle: Some(handle),
                });
            }
        }

        let mut finished = None;
        if let Some(install) = self.runtime_install.as_mut() {
            loop {
                match install.receiver.try_recv() {
                    Ok(RuntimeInstallEvent::Progress(message)) => {
                        ui.set_runtime_install_status_text(message.into());
                    }
                    Ok(RuntimeInstallEvent::Finished(result)) => {
                        finished = Some(result);
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        finished = Some(Err(
                            "The dependency installer stopped without returning a result.".into(),
                        ));
                        break;
                    }
                }
            }
        }

        let Some(result) = finished else {
            return;
        };
        if let Some(mut install) = self.runtime_install.take() {
            if let Some(handle) = install.handle.take() {
                let _ = handle.join();
            }
        }
        ui.set_runtime_busy(false);
        match result {
            Ok(status) => {
                apply_runtime_status(&ui, &status);
                if status.ready() {
                    ui.set_runtime_install_status_text(
                        "Dependencies installed and verified successfully.".into(),
                    );
                } else {
                    ui.set_runtime_install_status_text(
                        "Installation completed, but one or more dependencies are still missing. Re-scan or install them manually.".into(),
                    );
                }
            }
            Err(error) => {
                let status = detect_runtime();
                apply_runtime_status(&ui, &status);
                ui.set_runtime_install_status_text(format!("Install failed: {error}").into());
            }
        }
    }

    fn start_next_worker(&mut self, queue: &Rc<RefCell<JobQueue>>) -> Result<bool, String> {
        let next_job = { queue.borrow_mut().start_next()? };
        if next_job.is_none() {
            return Ok(false);
        }

        if let Some(status) = self.start_active_worker(queue) {
            return Err(status);
        }
        Ok(true)
    }

    fn start_active_worker(&mut self, queue: &Rc<RefCell<JobQueue>>) -> Option<String> {
        let job = queue.borrow().active_job()?.clone();
        if job.status != JobStatus::Running {
            return None;
        }
        configure_runtime_environment();
        let job_id = job.id;
        match pipeline_backend::spawn_job_worker(job) {
            Ok(worker) => {
                self.worker = Some(worker);
                None
            }
            Err(error) => {
                let message = format!("Could not start processing worker: {error}");
                mark_worker_start_failed(queue, job_id, &message);
                Some(format!("Failed: {message}"))
            }
        }
    }
}

fn apply_runtime_status(ui: &AppWindow, status: &RuntimeStatus) {
    ui.set_runtime_summary_text(status.summary().into());
    ui.set_runtime_ffmpeg_text(status.ffmpeg_text().into());
    ui.set_runtime_whisper_text(status.whisper_text().into());
    ui.set_runtime_model_text(status.model_text().into());
    ui.set_runtime_ready(status.ready());
}

fn audio_review_path(job: &Job) -> std::path::PathBuf {
    pipeline_backend::job_workspace(job)
        .stage_dir(PipelineStage::ReviewAudio)
        .join("review.json")
}

pub(crate) fn load_audio_review_report(job: &Job) -> Result<AudioReviewReport, String> {
    read_audio_review_report(&audio_review_path(job))
}

pub(crate) fn accept_audio_review_exclusion(job: &Job) -> Result<(), String> {
    accept_unmatched_audio_exclusion(&audio_review_path(job))
}

fn mark_worker_start_failed(
    queue: &Rc<RefCell<JobQueue>>,
    job_id: storyteller_core::JobId,
    message: &str,
) {
    let _ = queue
        .borrow_mut()
        .finish(job_id, JobOutcome::Failed(message.to_string()), 0);
}

fn refresh_if_open(
    ui_weak: &slint::Weak<AppWindow>,
    queue: &Rc<RefCell<JobQueue>>,
    queue_rows: &Rc<VecModel<QueueRow>>,
    stage_rows: &Rc<VecModel<StageRow>>,
    detail_stage_rows: &Rc<VecModel<StageDetailRow>>,
    status_override: Option<String>,
) {
    let Some(ui) = ui_weak.upgrade() else {
        return;
    };
    refresh_main_view(
        &ui,
        &queue.borrow(),
        queue_rows.as_ref(),
        stage_rows.as_ref(),
        detail_stage_rows.as_ref(),
    );
    if let Some(status) = status_override {
        ui.set_status_text(status.into());
    }
}