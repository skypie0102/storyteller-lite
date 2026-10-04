//! Owns desktop-independent commands, workers, recovery, and read-only snapshots.
use crate::{recovery_state, review_service, RuntimeStatus};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use storyteller_core::{
    accept_unmatched_audio_exclusion_with_draft, read_queue_recovery, write_queue_recovery,
    AudioReviewDecision, Job, JobId, JobInputs, JobOutcome, JobQueue, JobSettings, JobStatus,
    PipelineRunState, PipelineWorkerHandle, QueueMove,
};

pub enum ApplicationCommand {
    Enqueue {
        inputs: JobInputs,
        settings: JobSettings,
    },
    PauseAfterCurrent,
    ResumeQueue,
    CancelActive,
    RetryFromScratch(JobId),
    Remove(JobId),
    MoveWaiting {
        id: JobId,
        direction: QueueMove,
    },
    FinishReview(JobId),
    ContinueWithoutUnmatched(JobId),
    SaveReviewDecision {
        id: JobId,
        item_id: String,
        decision: AudioReviewDecision,
    },
    AssignGraphic {
        id: JobId,
        item_id: String,
        document_href: String,
        image_href: String,
    },
    PreserveEdge {
        id: JobId,
        item_id: String,
    },
    ScanRuntime,
    InstallMissingRuntime,
}

#[derive(Default)]
pub struct RuntimeView {
    pub status: Option<RuntimeStatus>,
    pub busy: bool,
    pub message: String,
}

/// Borrowed views avoid cloning the entire queue for each desktop refresh.
pub struct ApplicationSnapshot<'a> {
    pub queue: &'a JobQueue,
    pub queue_revision: u64,
    pub runtime: &'a RuntimeView,
    pub runtime_revision: u64,
    pub notice: Option<&'a str>,
}

enum RuntimeEvent {
    Progress(String),
    Finished(Result<RuntimeStatus, String>),
}

struct RuntimeTask {
    receiver: Receiver<RuntimeEvent>,
    handle: JoinHandle<()>,
    install: bool,
    result: Option<Result<RuntimeStatus, String>>,
}

pub struct ApplicationController {
    queue: JobQueue,
    queue_revision: u64,
    notice: Option<String>,
    worker: Option<PipelineWorkerHandle>,
    worker_job_id: Option<JobId>,
    pending_cancel: Option<JobId>,
    worker_launcher: fn(Job) -> Result<PipelineWorkerHandle, String>,
    runtime: RuntimeView,
    runtime_revision: u64,
    runtime_task: Option<RuntimeTask>,
    recovery_path: PathBuf,
    recovery_blocked: bool,
    recovery_dirty: bool,
    last_recovery_save: Instant,
}

impl Default for ApplicationController {
    fn default() -> Self {
        Self::new()
    }
}

impl ApplicationController {
    pub fn new() -> Self {
        Self::with_recovery_path(recovery_state::recovery_path())
    }

    pub fn with_recovery_path(path: PathBuf) -> Self {
        let mut app = Self {
            queue: JobQueue::default(),
            queue_revision: 1,
            notice: None,
            worker: None,
            worker_job_id: None,
            pending_cancel: None,
            worker_launcher: launch_job,
            runtime: RuntimeView::default(),
            runtime_revision: 1,
            runtime_task: None,
            recovery_path: path,
            recovery_blocked: false,
            recovery_dirty: false,
            last_recovery_save: Instant::now(),
        };
        // Load before accepting any commands; a timer must not overwrite newly queued work.
        match read_queue_recovery(&app.recovery_path) {
            Ok(recovered) if recovered.recovered_jobs > 0 => {
                let count = recovered.recovered_jobs;
                app.queue = recovered.queue;
                app.notice = Some(format!(
                    "Recovered {count} interrupted or queued book{}. Queue is paused; choose Resume queue to continue.",
                    if count == 1 { "" } else { "s" },
                ));
                app.recovery_dirty = true;
                app.persist_recovery(true);
            }
            Ok(_) => {}
            Err(error) => {
                app.notice = Some(match recovery_state::quarantine_queue(&app.recovery_path) {
                    Ok(Some(path)) => format!("Recovery state could not be loaded: {error} The unreadable snapshot was preserved at {}.", path.display()),
                    Ok(None) => format!("Recovery state could not be loaded: {error}"),
                    Err(preserve_error) => {
                        app.recovery_blocked = true;
                        format!("Recovery state could not be loaded: {error} The unreadable snapshot could not be preserved, so automatic recovery saving is disabled for this session: {preserve_error}")
                    }
                });
            }
        }
        app
    }

    pub fn snapshot(&self) -> ApplicationSnapshot<'_> {
        ApplicationSnapshot {
            queue: &self.queue,
            queue_revision: self.queue_revision,
            runtime: &self.runtime,
            runtime_revision: self.runtime_revision,
            notice: self.notice.as_deref(),
        }
    }

    pub fn dispatch(&mut self, command: ApplicationCommand) -> Result<(), String> {
        self.poll_worker();
        let notice = match command {
            ApplicationCommand::ScanRuntime => return self.start_runtime_task(false),
            ApplicationCommand::InstallMissingRuntime => return self.start_runtime_task(true),
            ApplicationCommand::Enqueue { inputs, settings } => {
                self.queue.enqueue(Job::new(inputs, settings)?);
                self.start_next_if_idle()?;
                None
            }
            ApplicationCommand::PauseAfterCurrent => {
                self.queue.request_pause_after_current();
                Some(
                    if self.queue.active_job().is_some() {
                        "Queue will pause after this book."
                    } else {
                        "Queue paused."
                    }
                    .into(),
                )
            }
            ApplicationCommand::ResumeQueue => {
                self.queue.resume();
                self.start_next_if_idle()?;
                None
            }
            ApplicationCommand::CancelActive => {
                if let Some(worker) = &self.worker {
                    worker.request_cancellation();
                    self.pending_cancel = self.worker_job_id;
                    Some("Cancellation requested".into())
                } else {
                    let job = self.queue.active_job().ok_or("No active book to cancel.")?;
                    let id = job.id;
                    let seconds = elapsed_seconds(job);
                    self.queue.finish(id, JobOutcome::Cancelled, seconds)?;
                    self.start_next_if_idle()?;
                    Some("Cancelled".into())
                }
            }
            ApplicationCommand::RetryFromScratch(id) => {
                self.ensure_worker_released(id)?;
                self.queue.retry_from_scratch(id)?;
                self.start_next_if_idle()?;
                None
            }
            ApplicationCommand::Remove(id) => {
                self.ensure_worker_released(id)?;
                self.queue.remove(id)?;
                None
            }
            ApplicationCommand::MoveWaiting { id, direction } => {
                self.queue.move_waiting(id, direction)?;
                None
            }
            ApplicationCommand::FinishReview(id) => {
                let job = self.review_job(id)?;
                let report = review_service::load_audio_review_report(job)?;
                if !report.is_complete() {
                    return Err(format!(
                        "Resolve all {} pending review segment(s) before continuing.",
                        report.pending_count()
                    ));
                }
                self.queue.resume_after_review(id)?;
                Some("Audio review complete; continuing.".into())
            }
            ApplicationCommand::ContinueWithoutUnmatched(id) => {
                let job = self.review_job(id)?;
                accept_unmatched_audio_exclusion_with_draft(
                    &review_service::audio_review_path(job),
                    &review_service::audio_review_draft_path(job),
                )?;
                self.queue.resume_after_review(id)?;
                Some("Review accepted; continuing.".into())
            }
            ApplicationCommand::SaveReviewDecision {
                id,
                item_id,
                decision,
            } => {
                review_service::save_decision(self.review_job(id)?, &item_id, decision)?;
                Some("Audio review decision saved.".into())
            }
            ApplicationCommand::AssignGraphic {
                id,
                item_id,
                document_href,
                image_href,
            } => {
                review_service::assign_graphic(
                    self.review_job(id)?,
                    &item_id,
                    &document_href,
                    &image_href,
                )?;
                Some("Image assigned as a Graphic Readout for this audio segment.".into())
            }
            ApplicationCommand::PreserveEdge { id, item_id } => {
                review_service::preserve_edge(self.review_job(id)?, &item_id)?;
                Some("Edge narration will be preserved on a supplemental read-aloud page.".into())
            }
        };
        self.notice = notice;
        self.touch_queue();
        self.persist_recovery(true);
        Ok(())
    }

    /// The host supplies a short timer; idle polls never rewrite recovery or redraw views.
    pub fn poll(&mut self) {
        self.poll_runtime_task();
        self.poll_worker();
        if self.worker.is_none() {
            match self.queue.start_next() {
                Ok(Some(_)) => {
                    self.notice = None;
                    self.touch_queue();
                }
                Ok(None) => {}
                Err(error) => {
                    self.notice = Some(error);
                    self.touch_queue();
                }
            }
            if let Some(job) = self
                .queue
                .active_job()
                .filter(|job| job.status == JobStatus::Running)
                .cloned()
            {
                let id = job.id;
                match (self.worker_launcher)(job) {
                    Ok(worker) => {
                        self.worker = Some(worker);
                        self.worker_job_id = Some(id);
                    }
                    Err(error) => {
                        let message = format!("Could not start processing worker: {error}");
                        let _ = self
                            .queue
                            .finish(id, JobOutcome::Failed(message.clone()), 0);
                        self.notice = Some(format!("Failed: {message}"));
                        self.touch_queue();
                        self.persist_recovery(true);
                        // The next poll advances, avoiding a long synchronous failure loop.
                    }
                }
            }
        }
        self.persist_recovery(false);
    }

    fn review_job(&self, id: JobId) -> Result<&Job, String> {
        self.ensure_worker_released(id)?;
        self.queue
            .active_job()
            .filter(|job| job.id == id && job.status == JobStatus::NeedsReview)
            .ok_or_else(|| "No matching book is waiting for audio review.".into())
    }

    fn ensure_worker_released(&self, id: JobId) -> Result<(), String> {
        if self.worker_job_id == Some(id) {
            Err("This book is still finalizing its worker. Try again when it finishes.".into())
        } else {
            Ok(())
        }
    }

    fn start_next_if_idle(&mut self) -> Result<(), String> {
        // A final snapshot may arrive before the thread releases its resources.
        // Keep new jobs Waiting until the owned worker has actually been joined.
        if self.worker.is_none() {
            self.queue.start_next()?;
        }
        Ok(())
    }

    fn poll_worker(&mut self) {
        let Some(worker) = &self.worker else {
            return;
        };
        let snapshot = worker.drain_updates().into_iter().last();
        let finished = worker.is_finished();
        if let Some(snapshot) = snapshot {
            let different = self
                .queue
                .job(snapshot.id)
                .is_ok_and(|old| old != &snapshot);
            match self.queue.reconcile_worker_snapshot(snapshot) {
                Ok(()) if different => {
                    self.notice = None;
                    self.touch_queue();
                }
                Ok(()) => {}
                Err(error) => {
                    if let Some(worker) = &self.worker {
                        worker.request_cancellation();
                    }
                    self.notice = Some(format!("Worker update was rejected: {error}"));
                    self.touch_queue();
                }
            }
        }
        if !finished {
            return;
        }
        let Some(worker) = self.worker.take() else {
            return;
        };
        let id = self.worker_job_id.take();
        self.notice = Some(match worker.join() {
            Ok(result) => match self.queue.reconcile_worker_snapshot(result.job) {
                Ok(()) => match result.run_result {
                    Ok(PipelineRunState::Completed) => "Finished".into(),
                    Ok(PipelineRunState::Cancelled) => "Cancelled".into(),
                    Ok(PipelineRunState::Failed(error)) => format!("Failed: {error}"),
                    Ok(PipelineRunState::NeedsReview(stage)) => {
                        format!("Needs review: {}", stage.label())
                    }
                    Ok(PipelineRunState::WaitingForResources(stage)) => {
                        format!("Waiting for resources: {}", stage.label())
                    }
                    Err(error) => {
                        if let Some(id) = id {
                            let _ = self.queue.finish(id, JobOutcome::Failed(error.clone()), 0);
                        }
                        format!("Processing stopped: {error}")
                    }
                },
                Err(error) => {
                    if let Some(id) = id {
                        let _ = self.queue.finish(id, JobOutcome::Failed(error.clone()), 0);
                    }
                    format!("Final worker update was rejected: {error}")
                }
            },
            Err(error) => {
                if let Some(id) = id {
                    let _ = self.queue.finish(id, JobOutcome::Failed(error.clone()), 0);
                }
                format!("Failed: {error}")
            }
        });
        // A cancellation request can race with a worker yielding human review.
        // Keep the user's command through that final handoff instead of losing it.
        if let Some(cancelled_id) = self.pending_cancel.take() {
            if self
                .queue
                .job(cancelled_id)
                .is_ok_and(|job| job.status.is_active())
            {
                let seconds = self
                    .queue
                    .job(cancelled_id)
                    .map(elapsed_seconds)
                    .unwrap_or(0);
                if self
                    .queue
                    .finish(cancelled_id, JobOutcome::Cancelled, seconds)
                    .is_ok()
                {
                    self.notice = Some("Cancelled".into());
                }
            }
        }
        self.touch_queue();
        self.persist_recovery(true);
    }

    fn touch_queue(&mut self) {
        self.queue_revision = self.queue_revision.wrapping_add(1);
        self.recovery_dirty = true;
    }

    fn persist_recovery(&mut self, force: bool) {
        if self.recovery_blocked
            || !self.recovery_dirty
            || (!force && self.last_recovery_save.elapsed() < Duration::from_secs(1))
        {
            return;
        }
        self.last_recovery_save = Instant::now();
        match write_queue_recovery(&self.recovery_path, &self.queue) {
            Ok(_) => self.recovery_dirty = false,
            Err(error) => eprintln!("Queue recovery could not be saved: {error}"),
        }
    }

    fn start_runtime_task(&mut self, install: bool) -> Result<(), String> {
        if self.runtime_task.is_some() {
            return Err("Dependency setup is already running.".into());
        }
        let (sender, receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            let status = crate::detect_runtime();
            let result = if install && !status.ready() {
                crate::install_missing_dependencies(&status, |message| {
                    let _ = sender.send(RuntimeEvent::Progress(message));
                })
                .map(|()| crate::detect_runtime())
            } else {
                Ok(status)
            };
            let _ = sender.send(RuntimeEvent::Finished(result));
        });
        self.runtime_task = Some(RuntimeTask {
            receiver,
            handle,
            install,
            result: None,
        });
        self.runtime.busy = true;
        self.runtime.message = if install {
            "Preparing dependency download…"
        } else {
            "Scanning dependencies…"
        }
        .into();
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
        Ok(())
    }

    fn poll_runtime_task(&mut self) {
        let Some(task) = &mut self.runtime_task else {
            return;
        };
        loop {
            match task.receiver.try_recv() {
                Ok(RuntimeEvent::Progress(message)) => {
                    self.runtime.message = message;
                    self.runtime_revision = self.runtime_revision.wrapping_add(1);
                }
                Ok(RuntimeEvent::Finished(result)) => {
                    task.result = Some(result);
                    break;
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        if !task.handle.is_finished() {
            return;
        }
        // A thread can finish between an empty receive and this completion check.
        // Drain again after completion before treating a missing result as failure.
        for event in task.receiver.try_iter() {
            if let RuntimeEvent::Finished(result) = event {
                task.result = Some(result);
            }
        }
        let Some(task) = self.runtime_task.take() else {
            return;
        };
        let joined = task.handle.join();
        self.runtime.busy = false;
        match task.result {
            Some(Ok(status)) if joined.is_ok() => {
                self.runtime.message = if !task.install { "Scan complete." }
                    else if status.ready() { "Dependencies installed and verified successfully." }
                    else { "Installation completed, but dependencies are still missing. Re-scan or install them manually." }.into();
                self.runtime.status = Some(status);
            }
            Some(Err(error)) => self.runtime.message = format!("Dependency setup failed: {error}"),
            _ => {
                self.runtime.message = "Dependency setup stopped without returning a result.".into()
            }
        }
        self.runtime_revision = self.runtime_revision.wrapping_add(1);
    }

    /// Save interrupted work before cancellation, then join owned processing to clean up children.
    /// The saved Running state recovers as Waiting; closing the app is not a user Cancel command.
    pub fn shutdown(&mut self) {
        self.poll_worker();
        self.persist_recovery(true);
        if let Some(worker) = self.worker.take() {
            worker.request_cancellation();
            let _ = worker.join();
        }
        self.worker_job_id = None;
        self.pending_cancel = None;
    }
}

impl Drop for ApplicationController {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn launch_job(job: Job) -> Result<PipelineWorkerHandle, String> {
    storyteller_core::validate_whistle_language(
        job.settings
            .language
            .as_deref()
            .unwrap_or(storyteller_core::WHISTLE_LANGUAGE),
    )?;
    crate::configure_runtime_environment();
    crate::spawn_job_worker(job)
}

fn elapsed_seconds(job: &Job) -> u64 {
    job.progress
        .stages()
        .iter()
        .filter_map(|stage| stage.elapsed_seconds)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, time::SystemTime};
    use storyteller_core::{
        spawn_pipeline_worker, AudioReviewDecisionSource, AudioReviewItem, AudioReviewReport,
        HardwareProfile, JobWorkspace, PipelineBackend, PipelineStage, QueueState, ResourceRequest,
        ResourceScheduler, ResumeContext, RuntimeCoordinator, StagePlan, StageRunContext,
        StageRunError, StageRunOutput,
    };

    struct TestRoot(PathBuf);
    impl TestRoot {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "storyteller-controller-{}-{stamp}",
                std::process::id()
            ));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn app(&self) -> ApplicationController {
            let mut app = ApplicationController::with_recovery_path(self.0.join("queue.json"));
            app.worker_launcher = reject_worker;
            app
        }
    }
    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn inputs(title: &str) -> JobInputs {
        JobInputs {
            title: title.into(),
            epub_path: format!("{title}.epub").into(),
            audiobook_path: format!("{title}.m4b").into(),
            output_path: format!("{title}-out.epub").into(),
        }
    }
    fn enqueue(app: &mut ApplicationController, title: &str) -> JobId {
        app.dispatch(ApplicationCommand::Enqueue {
            inputs: inputs(title),
            settings: JobSettings::default(),
        })
        .unwrap();
        app.snapshot().queue.jobs().last().unwrap().id
    }
    fn reject_worker(_job: Job) -> Result<PipelineWorkerHandle, String> {
        Err("test startup failure".into())
    }

    #[test]
    fn idle_polls_do_not_change_revisions_or_write_recovery() {
        let root = TestRoot::new();
        let mut app = root.app();
        let revisions = (
            app.snapshot().queue_revision,
            app.snapshot().runtime_revision,
        );
        for _ in 0..30 {
            app.poll();
        }
        assert_eq!(
            (
                app.snapshot().queue_revision,
                app.snapshot().runtime_revision
            ),
            revisions
        );
        assert!(!root.0.join("queue.json").exists());
    }

    #[test]
    fn startup_failure_advances_to_the_next_book_on_the_next_poll() {
        let root = TestRoot::new();
        let mut app = root.app();
        let first = enqueue(&mut app, "first");
        let second = enqueue(&mut app, "second");
        app.poll();
        assert_eq!(
            app.snapshot().queue.job(first).unwrap().status,
            JobStatus::Failed
        );
        assert_eq!(
            app.snapshot().queue.job(second).unwrap().status,
            JobStatus::Waiting
        );
        app.poll();
        assert_eq!(
            app.snapshot().queue.job(second).unwrap().status,
            JobStatus::Failed
        );
        assert!(app.snapshot().queue.active_job().is_none());
    }

    #[test]
    fn pause_after_current_survives_startup_failure() {
        let root = TestRoot::new();
        let mut app = root.app();
        enqueue(&mut app, "first");
        let second = enqueue(&mut app, "second");
        app.dispatch(ApplicationCommand::PauseAfterCurrent).unwrap();
        app.poll();
        app.poll();
        assert_eq!(app.snapshot().queue.state(), QueueState::Paused);
        assert_eq!(
            app.snapshot().queue.job(second).unwrap().status,
            JobStatus::Waiting
        );
        app.dispatch(ApplicationCommand::ResumeQueue).unwrap();
        assert_eq!(
            app.snapshot().queue.job(second).unwrap().status,
            JobStatus::Running
        );
    }

    #[test]
    fn cancel_before_worker_start_and_retry_preserve_queue_rules() {
        let root = TestRoot::new();
        let mut app = root.app();
        let first = enqueue(&mut app, "first");
        let second = enqueue(&mut app, "second");
        app.dispatch(ApplicationCommand::PauseAfterCurrent).unwrap();
        app.dispatch(ApplicationCommand::CancelActive).unwrap();
        assert_eq!(
            app.snapshot().queue.job(first).unwrap().status,
            JobStatus::Cancelled
        );
        assert_eq!(
            app.snapshot().queue.job(second).unwrap().status,
            JobStatus::Waiting
        );
        assert_eq!(app.snapshot().queue.state(), QueueState::Paused);
        app.dispatch(ApplicationCommand::RetryFromScratch(first))
            .unwrap();
        assert_eq!(
            app.snapshot().queue.job(first).unwrap().status,
            JobStatus::Waiting
        );
        assert!(app.dispatch(ApplicationCommand::CancelActive).is_err());
    }

    #[test]
    fn commands_validate_reordering_removal_and_language_before_mutation() {
        let root = TestRoot::new();
        let mut app = root.app();
        app.dispatch(ApplicationCommand::PauseAfterCurrent).unwrap();
        let first = enqueue(&mut app, "first");
        let second = enqueue(&mut app, "second");
        app.dispatch(ApplicationCommand::MoveWaiting {
            id: second,
            direction: QueueMove::Up,
        })
        .unwrap();
        assert_eq!(app.snapshot().queue.jobs()[0].id, second);
        app.dispatch(ApplicationCommand::Remove(first)).unwrap();
        app.dispatch(ApplicationCommand::ResumeQueue).unwrap();
        assert!(app.dispatch(ApplicationCommand::Remove(second)).is_err());
        let length = app.snapshot().queue.jobs().len();
        let settings = JobSettings {
            language: Some("fr".into()),
            ..JobSettings::default()
        };
        assert!(app
            .dispatch(ApplicationCommand::Enqueue {
                inputs: inputs("foreign"),
                settings
            })
            .is_err());
        assert_eq!(app.snapshot().queue.jobs().len(), length);
    }

    #[test]
    fn recovery_loads_before_commands_and_never_starts_automatically() {
        let root = TestRoot::new();
        let first;
        {
            let mut app = root.app();
            first = enqueue(&mut app, "interrupted");
        }
        let mut app = root.app();
        let second = enqueue(&mut app, "new-book");
        app.poll();
        assert_eq!(app.snapshot().queue.state(), QueueState::Paused);
        assert_eq!(app.snapshot().queue.jobs().len(), 2);
        assert_eq!(
            app.snapshot().queue.job(first).unwrap().status,
            JobStatus::Waiting
        );
        assert_eq!(
            app.snapshot().queue.job(second).unwrap().status,
            JobStatus::Waiting
        );
        let disk = read_queue_recovery(&root.0.join("queue.json")).unwrap();
        assert_eq!(disk.recovered_jobs, 2);
    }

    #[test]
    fn malformed_recovery_is_preserved_before_new_commands_save() {
        let root = TestRoot::new();
        fs::write(root.0.join("queue.json"), b"{broken").unwrap();
        let mut app = root.app();
        enqueue(&mut app, "new-book");
        let preserved = fs::read_dir(&root.0)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(".invalid-")
            })
            .unwrap();
        assert_eq!(fs::read(preserved).unwrap(), b"{broken");
        assert_eq!(
            read_queue_recovery(&root.0.join("queue.json"))
                .unwrap()
                .recovered_jobs,
            1
        );
    }

    fn put_review(app: &mut ApplicationController) -> (JobId, PathBuf) {
        let id = enqueue(app, "review");
        app.queue.require_review(id).unwrap();
        let job = app.queue.job(id).unwrap();
        let workspace = crate::job_workspace(job);
        fs::create_dir_all(workspace.stage_dir(PipelineStage::ReviewAudio)).unwrap();
        let report = AudioReviewReport {
            total_segments: 1,
            matched_segments: 0,
            match_percent: 0.0,
            unmatched: vec![AudioReviewItem {
                id: "segment-1".into(),
                alignment_index: 0,
                audio_start_ms: 0,
                audio_end_ms: 1_000,
                transcript_text: "Unmatched narration".into(),
                suggestion: None,
                edge: None,
                silence: None,
                decision: AudioReviewDecision::Pending,
            }],
            accepted_unmatched_exclusion: false,
        };
        fs::write(
            review_service::audio_review_path(job),
            serde_json::to_vec(&report).unwrap(),
        )
        .unwrap();
        (id, workspace.root().to_path_buf())
    }

    #[test]
    fn review_cannot_finish_until_durable_decisions_are_complete() {
        let root = TestRoot::new();
        let mut app = root.app();
        let (id, workspace) = put_review(&mut app);
        assert!(app
            .dispatch(ApplicationCommand::FinishReview(id))
            .unwrap_err()
            .contains("pending"));
        assert_eq!(
            app.snapshot().queue.job(id).unwrap().status,
            JobStatus::NeedsReview
        );
        app.dispatch(ApplicationCommand::SaveReviewDecision {
            id,
            item_id: "segment-1".into(),
            decision: AudioReviewDecision::Excluded {
                reason: "Explicit test decision".into(),
                source: AudioReviewDecisionSource::Manual,
            },
        })
        .unwrap();
        assert!(workspace.join("review-draft.json").is_file());
        app.dispatch(ApplicationCommand::FinishReview(id)).unwrap();
        assert_eq!(
            app.snapshot().queue.job(id).unwrap().status,
            JobStatus::Running
        );
        let _ = fs::remove_dir_all(workspace);
    }

    #[test]
    fn stale_review_commands_are_rejected_without_modifying_other_jobs() {
        let root = TestRoot::new();
        let mut app = root.app();
        let (id, workspace) = put_review(&mut app);
        let wrong = Job::new(inputs("stale"), JobSettings::default())
            .unwrap()
            .id;
        assert!(app
            .dispatch(ApplicationCommand::ContinueWithoutUnmatched(wrong))
            .is_err());
        let report = review_service::load_audio_review_report(app.queue.job(id).unwrap()).unwrap();
        assert_eq!(report.pending_count(), 1);
        app.dispatch(ApplicationCommand::ContinueWithoutUnmatched(id))
            .unwrap();
        assert_eq!(
            app.snapshot().queue.job(id).unwrap().status,
            JobStatus::Running
        );
        assert!(
            review_service::load_audio_review_report(app.queue.job(id).unwrap())
                .unwrap()
                .is_complete()
        );
        let _ = fs::remove_dir_all(workspace);
    }

    struct TestBackend {
        wait_for_cancel: bool,
        require_review: bool,
        exit_gate: Option<PathBuf>,
    }
    impl Drop for TestBackend {
        fn drop(&mut self) {
            if let Some(gate) = &self.exit_gate {
                let limit = Instant::now() + Duration::from_secs(5);
                while !gate.exists() && Instant::now() < limit {
                    thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }
    impl PipelineBackend for TestBackend {
        fn plan_stage(&mut self, _job: &Job, stage: PipelineStage) -> Result<StagePlan, String> {
            Ok(StagePlan::run(
                stage.label(),
                ResourceRequest::io_heavy(1),
                0,
            ))
        }
        fn run_stage(
            &mut self,
            context: &mut StageRunContext<'_>,
        ) -> Result<StageRunOutput, StageRunError> {
            if self.wait_for_cancel {
                while !context.cancellation_token().is_requested() {
                    thread::sleep(Duration::from_millis(1));
                }
                return Err(StageRunError::cancelled("test shutdown", 0));
            }
            let dir = context.workspace().stage_dir(context.stage());
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("artifact.txt"), context.stage().label()).unwrap();
            let output = StageRunOutput::new(vec![PathBuf::from("artifact.txt")], 1, 1);
            Ok(
                if self.require_review && context.stage() == PipelineStage::ReviewAudio {
                    output.requiring_review()
                } else {
                    output
                },
            )
        }
    }
    fn test_worker(
        job: Job,
        wait_for_cancel: bool,
        require_review: bool,
        hold_exit: bool,
    ) -> Result<PipelineWorkerHandle, String> {
        let context = ResumeContext {
            epub_source: "epub:test".into(),
            audiobook_source: "audio:test".into(),
            transcription_backend: "test".into(),
            alignment_backend: "test".into(),
            audio_backend: "test".into(),
            ocr_backend: "test".into(),
            epub_backend: "test".into(),
            effective_language: "en".into(),
            effective_transcription_model: "test".into(),
            settings: job.settings.clone(),
        };
        let scheduler = ResourceScheduler::automatic(&HardwareProfile {
            logical_cpu_threads: 2,
            memory_gib: None,
            gpu_backend: None,
            gpu_vram_mib: None,
        })?;
        let mut runtime = RuntimeCoordinator::new(scheduler);
        runtime.register_job(job.id)?;
        let workspace =
            JobWorkspace::new(std::env::temp_dir().join(format!("controller-worker-{}", job.id)));
        let exit_gate = hold_exit.then(|| workspace.root().join("release-worker"));
        spawn_pipeline_worker(
            job,
            workspace,
            runtime,
            context,
            TestBackend {
                wait_for_cancel,
                require_review,
                exit_gate,
            },
        )
    }
    fn fast_worker(job: Job) -> Result<PipelineWorkerHandle, String> {
        test_worker(job, false, false, false)
    }
    fn cancellable_worker(job: Job) -> Result<PipelineWorkerHandle, String> {
        test_worker(job, true, false, false)
    }
    fn held_completed_worker(job: Job) -> Result<PipelineWorkerHandle, String> {
        test_worker(job, false, false, true)
    }
    fn held_review_worker(job: Job) -> Result<PipelineWorkerHandle, String> {
        test_worker(job, false, true, true)
    }
    fn release_worker(id: JobId) {
        fs::write(
            std::env::temp_dir()
                .join(format!("controller-worker-{id}"))
                .join("release-worker"),
            b"release",
        )
        .unwrap();
    }
    fn clean_worker_workspace(id: JobId) {
        let _ = fs::remove_dir_all(std::env::temp_dir().join(format!("controller-worker-{id}")));
    }
    fn poll_until(app: &mut ApplicationController, predicate: impl Fn(&JobQueue) -> bool) {
        let limit = Instant::now() + Duration::from_secs(5);
        while !predicate(&app.queue) {
            assert!(
                Instant::now() < limit,
                "controller did not reach expected state"
            );
            app.poll();
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn joined_worker_honors_pause_and_resumes_the_next_book() {
        let root = TestRoot::new();
        let mut app = root.app();
        app.worker_launcher = fast_worker;
        let first = enqueue(&mut app, "first");
        let second = enqueue(&mut app, "second");
        app.dispatch(ApplicationCommand::PauseAfterCurrent).unwrap();
        poll_until(&mut app, |queue| queue.state() == QueueState::Paused);
        assert_eq!(app.queue.job(first).unwrap().status, JobStatus::Completed);
        assert_eq!(app.queue.job(second).unwrap().status, JobStatus::Waiting);
        app.dispatch(ApplicationCommand::ResumeQueue).unwrap();
        poll_until(&mut app, |queue| {
            queue
                .jobs()
                .iter()
                .all(|job| job.status == JobStatus::Completed)
        });
        app.shutdown();
        assert!(app.worker.is_none());
        assert!(!root.0.join("queue.json").exists());
        clean_worker_workspace(first);
        clean_worker_workspace(second);
    }

    #[test]
    fn commands_do_not_start_a_book_before_the_previous_worker_is_joined() {
        let root = TestRoot::new();
        let mut app = root.app();
        app.worker_launcher = held_completed_worker;
        let first = enqueue(&mut app, "first");
        poll_until(&mut app, |queue| {
            queue.job(first).unwrap().status == JobStatus::Completed
        });
        assert!(app
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished()));
        let second = enqueue(&mut app, "second");
        app.dispatch(ApplicationCommand::ResumeQueue).unwrap();
        assert_eq!(app.queue.job(second).unwrap().status, JobStatus::Waiting);
        assert!(app.queue.active_job().is_none());
        app.worker_launcher = fast_worker;
        release_worker(first);
        poll_until(&mut app, |queue| {
            queue.job(second).unwrap().status == JobStatus::Completed
        });
        app.shutdown();
        clean_worker_workspace(first);
        clean_worker_workspace(second);
    }

    #[test]
    fn cancellation_survives_a_worker_yielding_review_before_thread_exit() {
        let root = TestRoot::new();
        let mut app = root.app();
        app.worker_launcher = held_review_worker;
        let first = enqueue(&mut app, "first");
        let second = enqueue(&mut app, "second");
        poll_until(&mut app, |queue| {
            queue.job(first).unwrap().status == JobStatus::NeedsReview
        });
        assert!(app
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished()));
        app.dispatch(ApplicationCommand::PauseAfterCurrent).unwrap();
        app.dispatch(ApplicationCommand::CancelActive).unwrap();
        release_worker(first);
        poll_until(&mut app, |queue| {
            queue.job(first).unwrap().status == JobStatus::Cancelled
        });
        assert_eq!(app.queue.state(), QueueState::Paused);
        assert_eq!(app.queue.job(second).unwrap().status, JobStatus::Waiting);
        app.shutdown();
        clean_worker_workspace(first);
    }

    #[test]
    fn shutdown_joins_cancellation_but_recovers_interrupted_work_as_paused() {
        let root = TestRoot::new();
        let mut app = root.app();
        app.worker_launcher = cancellable_worker;
        let id = enqueue(&mut app, "interrupted");
        app.poll();
        assert!(app.worker.is_some());
        app.shutdown();
        assert!(app.worker.is_none());
        let recovered = ApplicationController::with_recovery_path(root.0.join("queue.json"));
        assert_eq!(recovered.snapshot().queue.state(), QueueState::Paused);
        assert_eq!(
            recovered.snapshot().queue.job(id).unwrap().status,
            JobStatus::Waiting
        );
        clean_worker_workspace(id);
    }

    #[test]
    fn runtime_task_finishes_without_blocking_commands_or_losing_its_result() {
        let root = TestRoot::new();
        let mut app = root.app();
        let (sender, receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            sender
                .send(RuntimeEvent::Progress("Checking pinned assets".into()))
                .unwrap();
            sender
                .send(RuntimeEvent::Finished(Ok(RuntimeStatus::default())))
                .unwrap();
        });
        app.runtime_task = Some(RuntimeTask {
            receiver,
            handle,
            install: false,
            result: None,
        });
        app.runtime.busy = true;
        assert!(app.dispatch(ApplicationCommand::ScanRuntime).is_err());
        app.dispatch(ApplicationCommand::PauseAfterCurrent).unwrap();
        let limit = Instant::now() + Duration::from_secs(5);
        while app.runtime.busy {
            assert!(Instant::now() < limit, "runtime task did not complete");
            app.poll();
            thread::sleep(Duration::from_millis(1));
        }
        assert!(app.snapshot().runtime.status.is_some());
        assert_eq!(app.snapshot().runtime.message, "Scan complete.");
        assert_eq!(app.snapshot().queue.state(), QueueState::Paused);
    }
}
