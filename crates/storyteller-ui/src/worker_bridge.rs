#[path = "pipeline_backend.rs"]
mod pipeline_backend;

use crate::{refresh_main_view, AppWindow, QueueRow, StageDetailRow, StageRow};
use slint::VecModel;
use std::{cell::RefCell, rc::Rc};
use storyteller_core::{JobOutcome, JobQueue, PipelineRunState, PipelineWorkerHandle};

#[derive(Default)]
pub(crate) struct WorkerBridge {
    worker: Option<PipelineWorkerHandle>,
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
                        Err(error) => Some(format!("Processing stopped: {error}")),
                    }
                }
            }
            Err(error) => Some(error),
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
