use crate::{
    job::JobStatus, Job, JobId, JobOutcome, JobWorkspace, ResumeContext, ValidatedResumePlan,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QueueState {
    #[default]
    Running,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueMove {
    Up,
    Down,
}

impl QueueMove {
    const fn delta(self) -> isize {
        match self {
            Self::Up => -1,
            Self::Down => 1,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct JobQueue {
    jobs: Vec<Job>,
    state: QueueState,
    pause_after_current: bool,
}

impl JobQueue {
    pub fn jobs(&self) -> &[Job] {
        &self.jobs
    }

    pub fn state(&self) -> QueueState {
        self.state
    }

    pub fn pause_after_current_requested(&self) -> bool {
        self.pause_after_current
    }

    pub fn enqueue(&mut self, job: Job) -> JobId {
        let id = job.id;
        self.jobs.push(job);
        id
    }

    pub fn active_job(&self) -> Option<&Job> {
        self.jobs
            .iter()
            .find(|job| matches!(job.status, JobStatus::Running | JobStatus::NeedsReview))
    }

    pub fn start_next(&mut self) -> Result<Option<JobId>, String> {
        if self.state == QueueState::Paused || self.active_job().is_some() {
            return Ok(None);
        }
        let Some(job) = self
            .jobs
            .iter_mut()
            .find(|job| job.status == JobStatus::Waiting)
        else {
            return Ok(None);
        };
        job.start()?;
        Ok(Some(job.id))
    }

    pub fn request_pause_after_current(&mut self) {
        if self.active_job().is_some() {
            self.pause_after_current = true;
        } else {
            self.state = QueueState::Paused;
        }
    }

    pub fn resume(&mut self) {
        self.pause_after_current = false;
        self.state = QueueState::Running;
    }

    pub fn require_review(&mut self, id: JobId) -> Result<(), String> {
        self.job_mut(id)?.require_review()
    }

    pub fn resume_after_review(&mut self, id: JobId) -> Result<(), String> {
        self.job_mut(id)?.resume_after_review()
    }

    pub fn finish(
        &mut self,
        id: JobId,
        outcome: JobOutcome,
        runtime_seconds: u64,
    ) -> Result<(), String> {
        self.job_mut(id)?.finish(outcome, runtime_seconds)?;
        self.apply_pause_after_terminal_transition();
        Ok(())
    }

    pub fn reconcile_worker_snapshot(&mut self, snapshot: Job) -> Result<(), String> {
        let index = self
            .jobs
            .iter()
            .position(|job| job.id == snapshot.id)
            .ok_or("Queue job was not found.")?;
        let previous = &self.jobs[index];
        if previous.inputs != snapshot.inputs || previous.settings != snapshot.settings {
            return Err("Worker snapshot changed immutable job inputs or settings.".into());
        }
        if previous.status.is_terminal() && !snapshot.status.is_terminal() {
            return Err("Worker snapshot cannot reopen a terminal job.".into());
        }
        if previous.status.is_active() && snapshot.status == JobStatus::Waiting {
            return Err("Worker snapshot cannot move an active job back to waiting.".into());
        }
        let became_terminal = previous.status.is_active() && snapshot.status.is_terminal();
        self.jobs[index] = snapshot;
        if became_terminal {
            self.apply_pause_after_terminal_transition();
        }
        Ok(())
    }

    fn apply_pause_after_terminal_transition(&mut self) {
        if self.pause_after_current {
            self.pause_after_current = false;
            self.state = QueueState::Paused;
        }
    }

    pub fn retry(&mut self, id: JobId) -> Result<(), String> {
        self.job_mut(id)?.retry()
    }

    pub fn retry_from_scratch(&mut self, id: JobId) -> Result<(), String> {
        self.job_mut(id)?.retry_from_scratch()
    }

    pub fn retry_with_resume(
        &mut self,
        id: JobId,
        context: &ResumeContext,
        workspace: &JobWorkspace,
    ) -> Result<ValidatedResumePlan, String> {
        let job = self.job_mut(id)?;
        let mut candidate = job.clone();
        candidate.retry()?;
        candidate.invalidate_stale_cache(context)?;
        let fingerprint_plan = candidate.resume_plan(context)?;
        let validated = workspace.validate_resume_plan(&fingerprint_plan);
        candidate.apply_validated_resume_plan(&validated)?;

        if let Some(invalid) = validated.invalid() {
            workspace.reset_from(invalid.stage)?;
        }

        *job = candidate;
        Ok(validated)
    }

    pub fn remove(&mut self, id: JobId) -> Result<Job, String> {
        let index = self
            .jobs
            .iter()
            .position(|job| job.id == id)
            .ok_or("Queue job was not found.")?;
        if self.jobs[index].status.is_active() {
            return Err("Cannot remove the active job; cancel it first.".into());
        }
        Ok(self.jobs.remove(index))
    }

    pub fn move_waiting(&mut self, id: JobId, direction: QueueMove) -> Result<(), String> {
        let index = self
            .jobs
            .iter()
            .position(|job| job.id == id)
            .ok_or("Queue job was not found.")?;
        if self.jobs[index].status != JobStatus::Waiting {
            return Err("Only waiting jobs can be reordered.".into());
        }

        let target = index as isize + direction.delta();
        if target < 0 || target >= self.jobs.len() as isize {
            return Err("Queue job cannot move farther in that direction.".into());
        }
        let target = target as usize;
        if self.jobs[target].status != JobStatus::Waiting {
            return Err(
                "Waiting jobs cannot be moved across the active or recent sections.".into(),
            );
        }
        self.jobs.swap(index, target);
        Ok(())
    }

    pub fn job(&self, id: JobId) -> Result<&Job, String> {
        self.jobs
            .iter()
            .find(|job| job.id == id)
            .ok_or_else(|| "Queue job was not found.".into())
    }

    pub fn job_mut(&mut self, id: JobId) -> Result<&mut Job, String> {
        self.jobs
            .iter_mut()
            .find(|job| job.id == id)
            .ok_or_else(|| "Queue job was not found.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JobInputs, JobSettings, PipelineStage};

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
    fn queue_auto_starts_the_first_waiting_job() {
        let mut queue = JobQueue::default();
        let first = queue.enqueue(sample_job("first"));
        queue.enqueue(sample_job("second"));
        assert_eq!(queue.start_next().unwrap(), Some(first));
        assert_eq!(queue.active_job().map(|job| job.id), Some(first));
        assert!(queue.start_next().unwrap().is_none());
    }

    #[test]
    fn nonterminal_worker_snapshot_keeps_pause_request_pending() {
        let mut queue = JobQueue::default();
        let id = queue.enqueue(sample_job("first"));
        queue.start_next().unwrap();
        queue.request_pause_after_current();
        let mut snapshot = queue.job(id).unwrap().clone();
        snapshot
            .progress
            .start_stage(PipelineStage::Prepare, "Preparing")
            .unwrap();
        snapshot.progress.set_current_stage_percent(50).unwrap();
        queue.reconcile_worker_snapshot(snapshot).unwrap();
        assert!(queue.pause_after_current_requested());
        assert_eq!(queue.state(), QueueState::Running);
    }

    #[test]
    fn terminal_worker_snapshot_honors_pause_after_current_once() {
        let mut queue = JobQueue::default();
        let id = queue.enqueue(sample_job("first"));
        queue.start_next().unwrap();
        queue.request_pause_after_current();
        let mut snapshot = queue.job(id).unwrap().clone();
        snapshot.finish(JobOutcome::Cancelled, 3).unwrap();
        queue.reconcile_worker_snapshot(snapshot.clone()).unwrap();
        assert_eq!(queue.state(), QueueState::Paused);
        assert!(!queue.pause_after_current_requested());
        queue.reconcile_worker_snapshot(snapshot).unwrap();
        assert_eq!(queue.state(), QueueState::Paused);
    }

    #[test]
    fn terminal_worker_snapshot_without_pause_keeps_queue_running() {
        let mut queue = JobQueue::default();
        let id = queue.enqueue(sample_job("first"));
        queue.start_next().unwrap();
        let mut snapshot = queue.job(id).unwrap().clone();
        snapshot.finish(JobOutcome::Completed, 3).unwrap();
        queue.reconcile_worker_snapshot(snapshot).unwrap();
        assert_eq!(queue.state(), QueueState::Running);
    }

    #[test]
    fn retry_from_scratch_clears_failed_stage_state() {
        let mut queue = JobQueue::default();
        let id = queue.enqueue(sample_job("retry"));
        queue.start_next().unwrap();
        {
            let job = queue.job_mut(id).unwrap();
            job.progress
                .start_stage(PipelineStage::Prepare, "Preparing")
                .unwrap();
            job.progress.set_current_stage_percent(100).unwrap();
            job.progress
                .complete_stage(PipelineStage::Prepare, 2)
                .unwrap();
            job.progress
                .start_stage(PipelineStage::Analyze, "Analyzing")
                .unwrap();
            job.progress.mark_failed(PipelineStage::Analyze).unwrap();
        }
        queue
            .finish(id, JobOutcome::Failed("analysis failed".into()), 7)
            .unwrap();

        queue.retry_from_scratch(id).unwrap();
        let job = queue.job(id).unwrap();
        assert_eq!(job.status, JobStatus::Waiting);
        assert_eq!(job.runtime_seconds, 0);
        assert!(job.last_error.is_none());
        assert!(job
            .progress
            .stages()
            .iter()
            .all(|stage| stage.status == crate::StageStatus::Pending));
    }

    #[test]
    fn completed_job_cannot_restart_from_scratch() {
        let mut queue = JobQueue::default();
        let id = queue.enqueue(sample_job("completed"));
        queue.start_next().unwrap();
        queue.finish(id, JobOutcome::Completed, 1).unwrap();
        assert!(queue.retry_from_scratch(id).is_err());
    }
}
