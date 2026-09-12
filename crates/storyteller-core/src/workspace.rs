use crate::{InvalidResumeStage, JobId, PipelineStage, ResumePlan, ValidatedResumePlan};
use std::{fs, path::{Path, PathBuf}};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobWorkspace {
    root: PathBuf,
}

impl JobWorkspace {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn for_job(base: PathBuf, job_id: JobId) -> Self {
        Self::new(base.join(job_id.to_string()))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn stage_dir(&self, stage: PipelineStage) -> PathBuf {
        self.root.join(stage_slug(stage))
    }

    pub fn capture_stage_artifacts(
        &self,
        stage: PipelineStage,
        artifacts: &[PathBuf],
    ) -> Result<(), String> {
        let stage_dir = self.stage_dir(stage);
        fs::create_dir_all(&stage_dir).map_err(|error| {
            format!("Could not create {} workspace: {error}", stage.label())
        })?;

        let mut manifest = String::new();
        for artifact in artifacts {
            if artifact.is_absolute() || artifact.components().any(|part| matches!(part, std::path::Component::ParentDir)) {
                return Err(format!(
                    "{} artifact path must stay inside its stage workspace.",
                    stage.label()
                ));
            }
            let path = stage_dir.join(artifact);
            let metadata = fs::metadata(&path).map_err(|error| {
                format!("Missing {} artifact {}: {error}", stage.label(), path.display())
            })?;
            if !metadata.is_file() || metadata.len() == 0 {
                return Err(format!(
                    "{} artifact is not a non-empty regular file: {}",
                    stage.label(),
                    path.display()
                ));
            }
            manifest.push_str(&artifact.to_string_lossy());
            manifest.push('\n');
        }
        fs::write(stage_dir.join(".artifacts"), manifest).map_err(|error| {
            format!("Could not record {} artifacts: {error}", stage.label())
        })
    }

    pub fn validate_resume_plan(&self, plan: &ResumePlan) -> ValidatedResumePlan {
        let mut reusable = Vec::new();
        let mut invalid = None;
        for stage in plan.reusable() {
            match self.validate_stage_artifacts(*stage) {
                Ok(()) => reusable.push(*stage),
                Err(reason) => {
                    invalid = Some(InvalidResumeStage {
                        stage: *stage,
                        reason,
                    });
                    break;
                }
            }
        }
        ValidatedResumePlan::new(reusable, invalid)
    }

    fn validate_stage_artifacts(&self, stage: PipelineStage) -> Result<(), String> {
        let stage_dir = self.stage_dir(stage);
        let manifest_path = stage_dir.join(".artifacts");
        let manifest = fs::read_to_string(&manifest_path).map_err(|error| {
            format!(
                "{} artifact manifest is unavailable: {error}",
                stage.label()
            )
        })?;
        for line in manifest.lines().filter(|line| !line.trim().is_empty()) {
            let relative = PathBuf::from(line);
            if relative.is_absolute()
                || relative
                    .components()
                    .any(|part| matches!(part, std::path::Component::ParentDir))
            {
                return Err(format!("{} artifact manifest is invalid.", stage.label()));
            }
            let path = stage_dir.join(relative);
            let metadata = fs::metadata(&path).map_err(|error| {
                format!("{} artifact is unavailable: {error}", path.display())
            })?;
            if !metadata.is_file() || metadata.len() == 0 {
                return Err(format!("{} artifact is invalid.", path.display()));
            }
        }
        Ok(())
    }

    pub fn reset_from(&self, stage: PipelineStage) -> Result<(), String> {
        for current in PipelineStage::ALL.into_iter().skip(stage.index()) {
            let path = self.stage_dir(current);
            if path.exists() {
                fs::remove_dir_all(&path).map_err(|error| {
                    format!("Could not reset {} workspace: {error}", current.label())
                })?;
            }
        }
        Ok(())
    }

    pub fn clear(&self) -> Result<(), String> {
        if self.root.exists() {
            fs::remove_dir_all(&self.root)
                .map_err(|error| format!("Could not clear job workspace: {error}"))?;
        }
        Ok(())
    }
}

fn stage_slug(stage: PipelineStage) -> &'static str {
    match stage {
        PipelineStage::Prepare => "prepare",
        PipelineStage::Analyze => "analyze",
        PipelineStage::Align => "align",
        PipelineStage::ReviewAudio => "review-audio",
        PipelineStage::Encode => "encode",
        PipelineStage::BuildEpub => "build-epub",
        PipelineStage::Validate => "validate",
    }
}
