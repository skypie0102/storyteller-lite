use crate::{JobSettings, PipelineStage};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeContext {
    pub epub_source: String,
    pub audiobook_source: String,
    pub whisper_backend: String,
    pub alignment_backend: String,
    pub audio_backend: String,
    pub ocr_backend: String,
    pub epub_backend: String,
    pub effective_language: String,
    pub effective_whisper_model: String,
    pub settings: JobSettings,
}

impl ResumeContext {
    pub fn validate(&self) -> Result<(), String> {
        for (label, value) in [
            ("EPUB source fingerprint", self.epub_source.as_str()),
            (
                "Audiobook source fingerprint",
                self.audiobook_source.as_str(),
            ),
            ("Whisper backend", self.whisper_backend.as_str()),
            ("Alignment backend", self.alignment_backend.as_str()),
            ("Audio backend", self.audio_backend.as_str()),
            ("OCR backend", self.ocr_backend.as_str()),
            ("EPUB backend", self.epub_backend.as_str()),
            ("Effective language", self.effective_language.as_str()),
            (
                "Effective Whisper model",
                self.effective_whisper_model.as_str(),
            ),
        ] {
            if value.trim().is_empty() {
                return Err(format!("{label} cannot be blank."));
            }
        }
        Ok(())
    }

    pub(crate) fn stage_fingerprint(&self, stage: PipelineStage) -> String {
        let mut hasher = Sha256::new();
        hasher.update(format!("stage:{}\n", stage.label()));
        hasher.update(format!("epub:{}\n", self.epub_source));
        hasher.update(format!("audio:{}\n", self.audiobook_source));
        hasher.update(format!("settings:{:?}\n", self.settings));
        match stage {
            PipelineStage::Prepare => {}
            PipelineStage::Analyze => {
                hasher.update(self.whisper_backend.as_bytes());
                hasher.update(self.effective_language.as_bytes());
                hasher.update(self.effective_whisper_model.as_bytes());
            }
            PipelineStage::Align | PipelineStage::ReviewAudio => {
                hasher.update(self.whisper_backend.as_bytes());
                hasher.update(self.alignment_backend.as_bytes());
                hasher.update(self.effective_language.as_bytes());
                hasher.update(self.effective_whisper_model.as_bytes());
            }
            PipelineStage::Encode => {
                hasher.update(self.alignment_backend.as_bytes());
                hasher.update(self.audio_backend.as_bytes());
            }
            PipelineStage::BuildEpub | PipelineStage::Validate => {
                hasher.update(self.alignment_backend.as_bytes());
                hasher.update(self.audio_backend.as_bytes());
                hasher.update(self.ocr_backend.as_bytes());
                hasher.update(self.epub_backend.as_bytes());
            }
        }
        format!("sha256:{:x}", hasher.finalize())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumePlan {
    reusable: Vec<PipelineStage>,
}

impl ResumePlan {
    pub fn new(reusable: Vec<PipelineStage>) -> Self {
        Self { reusable }
    }

    pub fn reusable(&self) -> &[PipelineStage] {
        &self.reusable
    }

    pub fn next_stage(&self) -> Option<PipelineStage> {
        PipelineStage::ALL.get(self.reusable.len()).copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidResumeStage {
    pub stage: PipelineStage,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedResumePlan {
    reusable: Vec<PipelineStage>,
    invalid: Option<InvalidResumeStage>,
}

impl ValidatedResumePlan {
    pub fn new(reusable: Vec<PipelineStage>, invalid: Option<InvalidResumeStage>) -> Self {
        Self { reusable, invalid }
    }

    pub fn reusable(&self) -> &[PipelineStage] {
        &self.reusable
    }

    pub fn invalid(&self) -> Option<&InvalidResumeStage> {
        self.invalid.as_ref()
    }

    pub fn next_stage(&self) -> Option<PipelineStage> {
        PipelineStage::ALL.get(self.reusable.len()).copied()
    }
}
