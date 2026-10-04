//! The desktop-independent application layer: pipeline execution and native runtimes.
//!
//! Slint sends commands and renders snapshots. It does not implement audio analysis,
//! dependency installation, checkpoint identity, or EPUB publication.

mod app_paths;
mod chunked_transcription;
mod controller;
mod pipeline_backend;
mod recovery_state;
mod review_preview;
mod review_service;
mod review_session;
mod runtime_setup;
mod worker_recommendation;

pub use app_paths::{persistent_app_root, recovery_app_root};
pub use chunked_transcription::{
    transcribe_audiobook_in_chunks, ChunkedTranscriptionConfig, ChunkedTranscriptionProgress,
    ChunkedTranscriptionSummary,
};
pub use controller::{ApplicationCommand, ApplicationController, ApplicationSnapshot, RuntimeView};
pub use pipeline_backend::{job_workspace, spawn_job_worker};
pub use review_service::{audio_review_draft_path, audio_review_path, load_audio_review_report};
pub use review_session::{ReviewAction, ReviewCandidate, ReviewView};
pub use runtime_setup::{
    configure_runtime_environment, detect_runtime, install_missing_dependencies, RuntimeStatus,
};
pub use worker_recommendation::WorkerRecommendation;
