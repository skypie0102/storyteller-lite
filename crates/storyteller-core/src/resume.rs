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
        match stage {
            PipelineStage::Prepare => {}
            PipelineStage::Analyze => {
                hasher.update(self.whisper_backend.as_bytes());
                hasher.update(self.effective_language.as_bytes());
                hasher.update(self.effective_whisper_model.as_bytes());
            }
            PipelineStage::Align => {
                hasher.update(self.whisper_backend.as_bytes());
                hasher.update(self.alignment_backend.as_bytes());
                hasher.update(self.effective_language.as_bytes());
                hasher.update(self.effective_whisper_model.as_bytes());
            }
            PipelineStage::ReviewAudio => {
                hasher.update(self.whisper_backend.as_bytes());
                hasher.update(self.alignment_backend.as_bytes());
                hasher.update(self.ocr_backend.as_bytes());
                hasher.update(self.effective_language.as_bytes());
                hasher.update(self.effective_whisper_model.as_bytes());
                hasher.update(format!(
                    "review-policy:{:?}\n",
                    self.settings.audio_review_policy
                ));
            }
            PipelineStage::Encode => {
                hasher.update(format!("audio-settings:{:?}\n", self.settings.audio));
                hasher.update(self.alignment_backend.as_bytes());
                hasher.update(self.audio_backend.as_bytes());
            }
            PipelineStage::BuildEpub | PipelineStage::Validate => {
                hasher.update(format!("audio-settings:{:?}\n", self.settings.audio));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AudioBitrate, AudioCodec, AudioEncoding, MAX_WHISPER_WORKERS};

    fn context(settings: JobSettings) -> ResumeContext {
        ResumeContext {
            epub_source: "epub:one".into(),
            audiobook_source: "audio:one".into(),
            whisper_backend: "whisper:one".into(),
            alignment_backend: "align:one".into(),
            audio_backend: "ffmpeg:one".into(),
            ocr_backend: "ocr:one".into(),
            epub_backend: "epub-builder:one".into(),
            effective_language: "auto".into(),
            effective_whisper_model: "model:one".into(),
            settings,
        }
    }

    #[test]
    fn worker_count_is_execution_only_for_stage_fingerprints() {
        let first = context(JobSettings::default());
        let changed = JobSettings {
            whisper_workers: MAX_WHISPER_WORKERS,
            ..JobSettings::default()
        };
        let second = context(changed);

        for stage in PipelineStage::ALL {
            assert_eq!(
                first.stage_fingerprint(stage),
                second.stage_fingerprint(stage),
                "worker count unexpectedly changed {} fingerprint",
                stage.label()
            );
        }
    }

    #[test]
    fn review_policy_invalidates_review_audio_but_not_alignment() {
        let first = context(JobSettings::default());
        let changed = JobSettings {
            audio_review_policy: crate::AudioReviewPolicy::ReviewAll,
            ..JobSettings::default()
        };
        let second = context(changed);

        for stage in [
            PipelineStage::Prepare,
            PipelineStage::Analyze,
            PipelineStage::Align,
        ] {
            assert_eq!(
                first.stage_fingerprint(stage),
                second.stage_fingerprint(stage),
                "review policy unexpectedly changed {} fingerprint",
                stage.label()
            );
        }
        assert_ne!(
            first.stage_fingerprint(PipelineStage::ReviewAudio),
            second.stage_fingerprint(PipelineStage::ReviewAudio)
        );
    }

    #[test]
    fn ocr_backend_invalidates_review_audio_but_not_alignment() {
        let first = context(JobSettings::default());
        let mut second = context(JobSettings::default());
        second.ocr_backend = "ocr:two".into();

        for stage in [
            PipelineStage::Prepare,
            PipelineStage::Analyze,
            PipelineStage::Align,
        ] {
            assert_eq!(
                first.stage_fingerprint(stage),
                second.stage_fingerprint(stage),
                "OCR backend unexpectedly changed {} fingerprint",
                stage.label()
            );
        }
        assert_ne!(
            first.stage_fingerprint(PipelineStage::ReviewAudio),
            second.stage_fingerprint(PipelineStage::ReviewAudio)
        );
    }

    #[test]
    fn audio_encoding_only_invalidates_encode_and_downstream() {
        let first = context(JobSettings::default());
        let changed = JobSettings {
            audio: AudioEncoding::new(AudioCodec::Aac, Some(AudioBitrate::Kbps96)).unwrap(),
            ..JobSettings::default()
        };
        let second = context(changed);

        for stage in [
            PipelineStage::Prepare,
            PipelineStage::Analyze,
            PipelineStage::Align,
            PipelineStage::ReviewAudio,
        ] {
            assert_eq!(
                first.stage_fingerprint(stage),
                second.stage_fingerprint(stage)
            );
        }
        for stage in [
            PipelineStage::Encode,
            PipelineStage::BuildEpub,
            PipelineStage::Validate,
        ] {
            assert_ne!(
                first.stage_fingerprint(stage),
                second.stage_fingerprint(stage)
            );
        }
    }
}
