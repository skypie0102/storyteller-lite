//! The desktop-independent application layer: pipeline execution and native runtimes.
//!
//! Slint sends commands and renders snapshots. It does not implement audio analysis,
//! dependency installation, checkpoint identity, or EPUB publication.

mod app_paths;
mod chunked_transcription;
mod pipeline_backend;
mod runtime_setup;

pub use app_paths::{persistent_app_root, recovery_app_root};
pub use chunked_transcription::{
    transcribe_audiobook_in_chunks, ChunkedTranscriptionConfig, ChunkedTranscriptionProgress,
    ChunkedTranscriptionSummary,
};
pub use pipeline_backend::{job_workspace, spawn_job_worker};
pub use runtime_setup::{
    configure_runtime_environment, detect_runtime, install_missing_dependencies, RuntimeStatus,
};
