use crate::{
    CancellationToken, Job, JobOutcome, JobStatus, JobWorkspace, LiveMetrics, PipelineStage,
    ResourceRequest, ResumeContext, RuntimeCoordinator, StageStatus,
};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineRunState {
    Completed,
    Cancelled,
    Failed(String),
    WaitingForResources(PipelineStage),
}

#[derive(Debug, Clone)]
pub struct StagePlan {
    activity: String,
    resources: ResourceRequest,
    planned_at_millis: u64,
}

impl StagePlan {
    pub fn run(
        activity: impl Into<String>,
        resources: ResourceRequest,
        planned_at_millis: u64,
    ) -> Self {
        Self {
            activity: activity.into(),
            resources,
            planned_at_millis,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageRunOutput {
    artifacts: Vec<PathBuf>,
    elapsed_seconds: u64,
    completed_at_millis: u64,
}

impl StageRunOutput {
    pub fn new(artifacts: Vec<PathBuf>, elapsed_seconds: u64, completed_at_millis: u64) -> Self {
        Self {
            artifacts,
            elapsed_seconds,
            completed_at_millis,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageRunErrorKind {
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageRunError {
    kind: StageRunErrorKind,
    message: String,
    at_millis: u64,
}

impl StageRunError {
    pub fn failed(message: impl Into<String>, at_millis: u64) -> Self {
        Self {
            kind: StageRunErrorKind::Failed,
            message: message.into(),
            at_millis,
        }
    }

    pub fn cancelled(message: impl Into<String>, at_millis: u64) -> Self {
        Self {
            kind: StageRunErrorKind::Cancelled,
            message: message.into(),
            at_millis,
        }
    }

    pub fn kind(&self) -> StageRunErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

pub trait PipelineObserver {
    fn observe(&mut self, job: &Job);
}

impl<F> PipelineObserver for F
where
    F: FnMut(&Job),
{
    fn observe(&mut self, job: &Job) {
        self(job);
    }
}

pub trait PipelineBackend: Send + 'static {
    fn plan_stage(&mut self, job: &Job, stage: PipelineStage) -> Result<StagePlan, String>;

    fn run_stage(
        &mut self,
        context: &mut StageRunContext<'_>,
    ) -> Result<StageRunOutput, StageRunError>;
}

pub struct StageRunContext<'a> {
    job: &'a mut Job,
    workspace: &'a JobWorkspace,
    stage: PipelineStage,
    cancellation: &'a CancellationToken,
    observer: &'a mut dyn PipelineObserver,
}

impl<'a> StageRunContext<'a> {
    pub fn stage(&self) -> PipelineStage {
        self.stage
    }

    pub fn job(&self) -> &Job {
        self.job
    }

    pub fn workspace(&self) -> &JobWorkspace {
        self.workspace
    }

    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    pub fn set_activity(
        &mut self,
        activity: impl Into<String>,
        _at_millis: u64,
    ) -> Result<(), String> {
        self.job.progress.set_activity(activity)?;
        self.observer.observe(self.job);
        Ok(())
    }

    pub fn set_stage_percent(&mut self, percent: u8, _at_millis: u64) -> Result<(), String> {
        self.job.progress.set_current_stage_percent(percent)?;
        self.observer.observe(self.job);
        Ok(())
    }

    pub fn set_metrics(&mut self, metrics: LiveMetrics, _at_millis: u64) {
        self.job.progress.set_metrics(metrics);
        self.observer.observe(self.job);
    }
}

pub fn run_pipeline<B: PipelineBackend>(
    job: &mut Job,
    workspace: &JobWorkspace,
    runtime: &mut RuntimeCoordinator,
    resume_context: &ResumeContext,
    cancellation: &CancellationToken,
    backend: &mut B,
    observer: &mut dyn PipelineObserver,
) -> Result<PipelineRunState, String> {
    if job.status != JobStatus::Running {
        return Err("Pipeline runner requires a running job.".into());
    }
    resume_context.validate()?;

    for stage in PipelineStage::ALL {
        let status = job.progress.stages()[stage.index()].status;
        if matches!(
            status,
            StageStatus::Completed | StageStatus::Cached | StageStatus::Skipped
        ) {
            continue;
        }
        if cancellation.is_requested() {
            job.progress.set_activity("Processing cancelled")?;
            job.finish(JobOutcome::Cancelled, elapsed_seconds(job))?;
            observer.observe(job);
            return Ok(PipelineRunState::Cancelled);
        }

        let plan = match backend.plan_stage(job, stage) {
            Ok(plan) => plan,
            Err(error) => {
                job.progress.set_activity("Pipeline stage unavailable")?;
                job.finish(JobOutcome::Failed(error.clone()), elapsed_seconds(job))?;
                observer.observe(job);
                return Ok(PipelineRunState::Failed(error));
            }
        };

        if runtime.acquire(job.id, plan.resources).is_err() {
            job.progress
                .set_activity(format!("Waiting for resources: {}", stage.label()))?;
            observer.observe(job);
            return Ok(PipelineRunState::WaitingForResources(stage));
        }

        job.progress.start_stage(stage, plan.activity)?;
        observer.observe(job);
        let mut context = StageRunContext {
            job,
            workspace,
            stage,
            cancellation,
            observer,
        };

        match backend.run_stage(&mut context) {
            Ok(output) => {
                let _ = output.completed_at_millis.max(plan.planned_at_millis);
                context
                    .job
                    .progress
                    .complete_stage(stage, output.elapsed_seconds)?;
                context
                    .workspace
                    .capture_stage_artifacts(stage, &output.artifacts)?;
                context
                    .job
                    .checkpoint_completed_stage(stage, resume_context)?;
                context.observer.observe(context.job);
            }
            Err(error)
                if error.kind == StageRunErrorKind::Cancelled || cancellation.is_requested() =>
            {
                let _ = error.at_millis;
                context.job.progress.reset_stage(stage);
                context.job.progress.set_activity("Processing cancelled")?;
                context
                    .job
                    .finish(JobOutcome::Cancelled, elapsed_seconds(context.job))?;
                context.observer.observe(context.job);
                return Ok(PipelineRunState::Cancelled);
            }
            Err(error) => {
                let _ = error.at_millis;
                context.job.progress.mark_failed(stage)?;
                context
                    .job
                    .progress
                    .set_activity(format!("{} failed", stage.label()))?;
                context.job.finish(
                    JobOutcome::Failed(error.message.clone()),
                    elapsed_seconds(context.job),
                )?;
                context.observer.observe(context.job);
                return Ok(PipelineRunState::Failed(error.message));
            }
        }
    }

    job.progress.set_activity("Finished")?;
    job.finish(JobOutcome::Completed, elapsed_seconds(job))?;
    observer.observe(job);
    Ok(PipelineRunState::Completed)
}

fn elapsed_seconds(job: &Job) -> u64 {
    job.progress
        .stages()
        .iter()
        .filter_map(|stage| stage.elapsed_seconds)
        .sum()
}
