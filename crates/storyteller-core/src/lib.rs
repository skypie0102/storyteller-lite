mod alignment;
mod audio_review;
mod cancellation;
mod command;
mod epub_corpus;
mod job;
mod progress;
mod queue;
mod resume;
mod runner;
mod scheduler;
mod source_fingerprint;
mod source_prepare;
mod whisper_transcript;
mod worker;
mod workspace;

pub use alignment::{
    align_transcript_to_corpus, AlignmentDocument, AlignmentProgress, AlignmentSegment,
    AlignmentStatus, AlignmentSummary, CorpusPosition,
};
pub use audio_review::{
    accept_unmatched_audio_exclusion, create_audio_review_report, read_audio_review_report,
    AudioReviewItem, AudioReviewReport, AudioReviewSummary,
};
pub use cancellation::CancellationToken;
pub use command::{
    run_cancellable_command, CommandOutput, CommandRunError, CommandStream,
};
pub use epub_corpus::{
    extract_epub_corpus, read_epub_corpus, EpubCorpus, EpubCorpusSummary, EpubSection,
};
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
pub use source_prepare::{
    copy_file_cancellable, prepare_job_sources, prepared_job_sources, PreparedSources,
};
pub use whisper_transcript::{read_whisper_transcript, TranscriptSegment, WhisperTranscript};
pub use worker::{
    spawn_pipeline_worker, spawn_pipeline_worker_with_preflight, PipelineEnvironment,
    PipelineWorkerHandle, WorkerResult,
};
pub use workspace::JobWorkspace;
