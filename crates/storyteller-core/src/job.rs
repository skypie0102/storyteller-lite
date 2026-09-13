use crate::{
    PipelineProgress, PipelineStage, ResumeContext, ResumePlan, StageStatus, ValidatedResumePlan,
};
use std::path::PathBuf;
use uuid::Uuid;

pub type JobId = Uuid;

pub const MIN_WHISPER_WORKERS: usize = 1;
pub const MAX_WHISPER_WORKERS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioCodec {
    Copy,
    Opus,
    Aac,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioBitrate {
    Kbps32,
    Kbps64,
    Kbps96,
}

impl AudioBitrate {
    pub const fn kbps(self) -> u16 {
        match self {
            Self::Kbps32 => 32,
            Self::Kbps64 => 64,
            Self::Kbps96 => 96,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AudioEncoding {
    pub codec: AudioCodec,
    pub bitrate: Option<AudioBitrate>,
}

impl AudioEncoding {
    pub fn new(codec: AudioCodec, bitrate: Option<AudioBitrate>) -> Result<Self, String> {
        match (codec, bitrate) {
            (AudioCodec::Copy, None) => Ok(Self { codec, bitrate }),
            (AudioCodec::Copy, Some(_)) => Err("Copy audio cannot specify a bitrate.".into()),
            (_, Some(_)) => Ok(Self { codec, bitrate }),
            (_, None) => Err("Encoded audio requires a bitrate.".into()),
        }
    }

    pub const fn copy() -> Self {
        Self {
            codec: AudioCodec::Copy,
            bitrate: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JobSettings {
    pub audio: AudioEncoding,
    pub language: Option<String>,
    pub whisper_model: String,
    /// Maximum number of independent transcription chunks that may run concurrently.
    /// This is intentionally separate from whisper.cpp's internal processor count.
    pub whisper_workers: usize,
}

impl Default for JobSettings {
    fn default() -> Self {
        Self {
            audio: AudioEncoding {
                codec: AudioCodec::Opus,
                bitrate: Some(AudioBitrate::Kbps64),
            },
            language: None,
            whisper_model: "large-v3-turbo".into(),
            whisper_workers: MIN_WHISPER_WORKERS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobInputs {
    pub title: String,
    pub epub_path: PathBuf,
    pub audiobook_path: PathBuf,
    pub output_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Waiting,
    Running,
    NeedsReview,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    pub const fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::NeedsReview)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobOutcome {
    Completed,
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StageCheckpoint {
    pub stage: PipelineStage,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub id: JobId,
    pub inputs: JobInputs,
    pub settings: JobSettings,
    pub status: JobStatus,
    pub progress: PipelineProgress,
    pub last_error: Option<String>,
    pub runtime_seconds: u64,
    pub(crate) checkpoints: Vec<StageCheckpoint>,
}

impl Job {
    pub fn new(inputs: JobInputs, settings: JobSettings) -> Result<Self, String> {
        if inputs.title.trim().is_empty() {
            return Err("Book title cannot be blank.".into());
        }
        if inputs.epub_path.as_os_str().is_empty() || inputs.audiobook_path.as_os_str().is_empty() {
            return Err("EPUB and audiobook sources are required.".into());
        }
        if inputs.output_path.as_os_str().is_empty() {
            return Err("Output path is required.".into());
        }
        if inputs.output_path == inputs.epub_path {
            return Err("Output path must not replace the source EPUB.".into());
        }
        if !(MIN_WHISPER_WORKERS..=MAX_WHISPER_WORKERS).contains(&settings.whisper_workers) {
            return Err(format!(
                "Whisper workers must be between {MIN_WHISPER_WORKERS} and {MAX_WHISPER_WORKERS}."
            ));
        }
        Ok(Self {
            id: Uuid::new_v4(),
            inputs,
            settings,
            status: JobStatus::Waiting,
            progress: PipelineProgress::default(),
            last_error: None,
            runtime_seconds: 0,
            checkpoints: Vec::new(),
        })
    }

    pub fn start(&mut self) -> Result<(), String> {
        if self.status != JobStatus::Waiting {
            return Err("Only waiting jobs can start.".into());
        }
        self.status = JobStatus::Running;
        Ok(())
    }

    pub fn require_review(&mut self) -> Result<(), String> {
        if self.status != JobStatus::Running {
            return Err("Only a running job can request review.".into());
        }
        self.status = JobStatus::NeedsReview;
        Ok(())
    }

    pub fn resume_after_review(&mut self) -> Result<(), String> {
        if self.status != JobStatus::NeedsReview {
            return Err("Job is not waiting for review.".into());
        }
        self.status = JobStatus::Running;
        Ok(())
    }

    pub fn finish(&mut self, outcome: JobOutcome, runtime_seconds: u64) -> Result<(), String> {
        if !self.status.is_active() {
            return Err("Only an active job can finish.".into());
        }
        self.runtime_seconds = runtime_seconds;
        match outcome {
            JobOutcome::Completed => {
                self.status = JobStatus::Completed;
                self.last_error = None;
            }
            JobOutcome::Failed(error) => {
                self.status = JobStatus::Failed;
                self.last_error = Some(error);
            }
            JobOutcome::Cancelled => {
                self.status = JobStatus::Cancelled;
                self.last_error = None;
            }
        }
        Ok(())
    }

    pub fn retry(&mut self) -> Result<(), String> {
        if !self.status.is_terminal() {
            return Err("Only a finished job can be retried.".into());
        }
        self.status = JobStatus::Waiting;
        self.last_error = None;
        if let Some(stage) = self
            .progress
            .stages()
            .iter()
            .find(|stage| stage.status == StageStatus::Failed)
            .map(|stage| stage.stage)
        {
            self.progress.reset_from(stage);
        }
        Ok(())
    }

    pub fn retry_from_scratch(&mut self) -> Result<(), String> {
        if !matches!(self.status, JobStatus::Failed | JobStatus::Cancelled) {
            return Err("Only failed or cancelled jobs can restart from the beginning.".into());
        }
        self.status = JobStatus::Waiting;
        self.progress = PipelineProgress::default();
        self.last_error = None;
        self.runtime_seconds = 0;
        self.checkpoints.clear();
        Ok(())
    }

    pub fn checkpoint_completed_stage(
        &mut self,
        stage: PipelineStage,
        context: &ResumeContext,
    ) -> Result<(), String> {
        let status = self.progress.stages()[stage.index()].status;
        if !matches!(status, StageStatus::Completed | StageStatus::Cached) {
            return Err(format!(
                "Cannot checkpoint incomplete stage {}.",
                stage.label()
            ));
        }
        let checkpoint = StageCheckpoint {
            stage,
            fingerprint: context.stage_fingerprint(stage),
        };
        self.checkpoints.retain(|saved| saved.stage != stage);
        self.checkpoints.push(checkpoint);
        self.checkpoints.sort_by_key(|saved| saved.stage.index());
        Ok(())
    }

    pub fn invalidate_stale_cache(&mut self, context: &ResumeContext) -> Result<(), String> {
        if let Some(stale) = self
            .checkpoints
            .iter()
            .find(|saved| saved.fingerprint != context.stage_fingerprint(saved.stage))
            .map(|saved| saved.stage)
        {
            self.progress.reset_from(stale);
            self.checkpoints
                .retain(|saved| saved.stage.index() < stale.index());
        }
        Ok(())
    }

    pub fn resume_plan(&self, context: &ResumeContext) -> Result<ResumePlan, String> {
        let mut reusable = Vec::new();
        for stage in PipelineStage::ALL {
            let Some(saved) = self.checkpoints.iter().find(|saved| saved.stage == stage) else {
                break;
            };
            if saved.fingerprint != context.stage_fingerprint(stage) {
                break;
            }
            reusable.push(stage);
        }
        Ok(ResumePlan::new(reusable))
    }

    pub fn apply_validated_resume_plan(
        &mut self,
        plan: &ValidatedResumePlan,
    ) -> Result<(), String> {
        self.progress.reset_from(PipelineStage::Prepare);
        for stage in plan.reusable() {
            self.progress.mark_cached(*stage, None)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> JobInputs {
        JobInputs {
            title: "Test".into(),
            epub_path: "test.epub".into(),
            audiobook_path: "test.m4b".into(),
            output_path: "test (readaloud).epub".into(),
        }
    }

    #[test]
    fn whisper_worker_count_is_bounded_for_queued_jobs() {
        let mut settings = JobSettings::default();
        settings.whisper_workers = MAX_WHISPER_WORKERS;
        assert!(Job::new(inputs(), settings).is_ok());

        let mut invalid = JobSettings::default();
        invalid.whisper_workers = MAX_WHISPER_WORKERS + 1;
        assert!(Job::new(inputs(), invalid).is_err());
    }
}
