mod cancellation;
mod job;
mod progress;
mod queue;
mod resume;
mod runner;
mod scheduler;
mod source_fingerprint;
mod source_prepare;
mod worker;
mod workspace;

pub use cancellation::CancellationToken;
pub use job::{
    AudioBitrate, AudioCodec, AudioEncoding, Job, JobId, JobInputs, JobOutcome, JobSettings,
    JobStatus,
};
pub use progress::{LiveMetrics, PipelineProgress, PipelineStage, StageProgress, StageStatus};
pub use queue::{JobQueue, QueueMove, QueueState};
pub use resume::{InvalidResumeStage, ResumeContext, ResumePlan, ValidatedResumePlan};
pub use runner::{
    run_pipeline, PipelineBackend, PipelineObserver, PipelineRunState, StagePlan, StageRunContext,
    StageRunError, StageRunErrorKind, StageRunOutput,
};
pub use scheduler::{HardwareProfile, ResourceRequest, ResourceScheduler, RuntimeCoordinator};
pub use source_fingerprint::{
    fingerprint_job_sources, fingerprint_source_file, SourceFingerprints,
};
pub use source_prepare::{prepare_job_sources, PreparedSources};
pub use worker::{
    spawn_pipeline_worker, spawn_pipeline_worker_with_preflight, PipelineEnvironment,
    PipelineWorkerHandle, WorkerResult,
};
pub use workspace::JobWorkspace;
