use crate::PipelineStage;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Required outputs for each stage, relative to that stage's owned workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case", deny_unknown_fields)]
pub enum StageArtifacts {
    Prepare {
        epub: PathBuf,
        audiobook: PathBuf,
    },
    Analyze {
        corpus: PathBuf,
        plan: PathBuf,
        transcript: PathBuf,
    },
    Align {
        alignment: PathBuf,
    },
    ReviewAudio {
        report: PathBuf,
    },
    Encode {
        audio: PathBuf,
        descriptor: PathBuf,
    },
    BuildEpub {
        candidate: PathBuf,
        effective_alignment: PathBuf,
    },
    Validate {
        report: PathBuf,
    },
}

impl StageArtifacts {
    pub fn stage(&self) -> PipelineStage {
        match self {
            Self::Prepare { .. } => PipelineStage::Prepare,
            Self::Analyze { .. } => PipelineStage::Analyze,
            Self::Align { .. } => PipelineStage::Align,
            Self::ReviewAudio { .. } => PipelineStage::ReviewAudio,
            Self::Encode { .. } => PipelineStage::Encode,
            Self::BuildEpub { .. } => PipelineStage::BuildEpub,
            Self::Validate { .. } => PipelineStage::Validate,
        }
    }

    pub fn paths(&self) -> Vec<&Path> {
        match self {
            Self::Prepare { epub, audiobook } => vec![epub, audiobook],
            Self::Analyze {
                corpus,
                plan,
                transcript,
            } => vec![corpus, plan, transcript],
            Self::Align { alignment } => vec![alignment],
            Self::ReviewAudio { report } | Self::Validate { report } => vec![report],
            Self::Encode { audio, descriptor } => vec![audio, descriptor],
            Self::BuildEpub {
                candidate,
                effective_alignment,
            } => vec![candidate, effective_alignment],
        }
    }
}
