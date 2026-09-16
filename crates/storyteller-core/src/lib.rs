mod alignment;
mod audio_encode;
mod audio_review;
mod cancellation;
mod command;
#[allow(clippy::too_many_arguments)]
mod epub_build;
mod epub_corpus;
mod epub_graphic;
#[allow(clippy::too_many_arguments, clippy::same_item_push, dead_code)]
mod epub_overlay;
mod epub_validate;
mod job;
mod job_recovery;
mod progress;
mod queue;
mod resume;
mod review_assignment;
#[allow(clippy::too_many_arguments)]
mod review_graphic;
mod review_graphic_manual;
mod review_image;
mod review_image_match;
mod review_materialize;
mod review_ocr;
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
pub use audio_encode::{
    encode_audiobook, read_encoded_audio_descriptor, AudioEncodeProgress, EncodedAudio,
    EncodedAudioDescriptor,
};
pub use audio_review::{
    accept_unmatched_audio_exclusion, accept_unmatched_audio_exclusion_with_draft,
    apply_audio_review_decision, create_audio_review_report, create_audio_review_report_with_draft,
    read_audio_review_report, set_audio_review_silence_evidence, AudioReviewClassification,
    AudioReviewDecision, AudioReviewDecisionSource, AudioReviewDestination, AudioReviewEdge,
    AudioReviewItem, AudioReviewPolicy, AudioReviewReport, AudioReviewSilenceEvidence,
    AudioReviewSuggestion, AudioReviewSummary, AudioReviewSupplementalPlacement,
};
pub use cancellation::CancellationToken;
pub use command::{run_cancellable_command, CommandOutput, CommandRunError, CommandStream};
pub use epub_build::{build_readaloud_epub, EpubBuildSummary};
pub use epub_corpus::{
    extract_epub_corpus, read_epub_corpus, EpubCorpus, EpubCorpusSummary, EpubSection,
};
pub use epub_validate::{
    publish_validated_epub, validate_readaloud_epub, write_validation_report, EpubValidationSummary,
};
pub use job::{
    AudioBitrate, AudioCodec, AudioEncoding, Job, JobId, JobInputs, JobOutcome, JobSettings,
    JobStatus, MAX_WHISPER_WORKERS, MIN_WHISPER_WORKERS,
};
pub use job_recovery::{read_queue_recovery, write_queue_recovery, QueueRecovery};
pub use progress::{LiveMetrics, PipelineProgress, PipelineStage, StageProgress, StageStatus};
pub use queue::{JobQueue, QueueMove, QueueState};
pub use resume::{InvalidResumeStage, ResumeContext, ResumePlan, ValidatedResumePlan};
pub use review_assignment::{
    apply_audio_review_to_alignment, review_text_candidates, AudioReviewTextCandidate,
    DEFAULT_REVIEW_CANDIDATE_LIMIT,
};
pub use review_graphic::apply_smart_graphic_readouts;
pub use review_graphic_manual::assign_manual_graphic_readout;
pub use review_image::{
    review_image_candidates, AudioReviewImageCandidate, DEFAULT_REVIEW_IMAGE_DOCUMENT_LIMIT,
    DEFAULT_REVIEW_IMAGE_LIMIT,
};
pub use review_image_match::{
    review_image_matches, AudioReviewImageMatchCandidate, AudioReviewImageMatchResult,
};
pub use review_materialize::materialize_reviewed_alignment;
pub use review_ocr::{
    review_image_text_evidence, AudioReviewImageEvidenceSource, AudioReviewImageTextEvidence,
};
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
pub use whisper_transcript::{
    merge_chunk_transcripts, plan_transcription_chunks, read_whisper_transcript,
    read_whisper_transcript_chunk, validate_chunk_plan, write_whisper_transcript,
    TranscriptSegment, TranscriptionChunk, WhisperTranscript, CHAPTER_BOUNDARY_TOLERANCE_MS,
    DEFAULT_MAX_TRANSCRIPTION_CHUNK_MS,
};
pub use worker::{
    spawn_pipeline_worker, spawn_pipeline_worker_with_preflight, PipelineEnvironment,
    PipelineWorkerHandle, WorkerResult,
};
pub use workspace::JobWorkspace;
