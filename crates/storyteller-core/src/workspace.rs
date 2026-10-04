use crate::{
    fingerprint_source_file, CancellationToken, InvalidResumeStage, JobId, PipelineStage,
    ResumePlan, StageArtifacts, ValidatedResumePlan,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use uuid::Uuid;

const ARTIFACT_MANIFEST_VERSION: u32 = 2;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactManifest {
    version: u32,
    outputs: StageArtifacts,
    files: Vec<ArtifactRecord>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactRecord {
    path: PathBuf,
    bytes: u64,
    sha256: String,
}

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
        artifacts: &StageArtifacts,
        cancellation: &CancellationToken,
    ) -> Result<(), String> {
        if artifacts.stage() != stage {
            return Err("Stage artifact types do not match the pipeline stage.".into());
        }
        let stage_dir = self.stage_dir(stage);
        fs::create_dir_all(&stage_dir)
            .map_err(|error| format!("Could not create {} workspace: {error}", stage.label()))?;

        let mut seen = HashSet::new();
        let mut files = Vec::new();
        for relative in artifacts.paths() {
            if !seen.insert(relative) {
                return Err("Stage output roles must use distinct artifact paths.".into());
            }
            let path = artifact_path(&stage_dir, relative)?;
            let bytes = fs::metadata(&path)
                .map_err(|error| format!("Could not inspect stage artifact: {error}"))?
                .len();
            let sha256 = fingerprint_source_file(&path, cancellation, stage.label())?;
            files.push(ArtifactRecord {
                path: relative.to_owned(),
                bytes,
                sha256,
            });
        }
        let manifest = ArtifactManifest {
            version: ARTIFACT_MANIFEST_VERSION,
            outputs: artifacts.clone(),
            files,
        };
        let json = serde_json::to_vec_pretty(&manifest)
            .map_err(|error| format!("Could not serialize stage artifacts: {error}"))?;
        if cancellation.is_requested() {
            return Err("Stage artifact capture was cancelled.".into());
        }
        write_atomic(&stage_dir.join(".artifacts"), &json)
    }

    pub fn validate_resume_plan(
        &self,
        plan: &ResumePlan,
        cancellation: &CancellationToken,
    ) -> Result<ValidatedResumePlan, String> {
        let mut reusable = Vec::new();
        let mut invalid = None;
        for stage in plan.reusable() {
            // Publication is an external effect, so a cached report cannot prove the output still exists.
            let result = if *stage == PipelineStage::Validate {
                Err("Output publication must be verified again.".into())
            } else {
                self.validate_stage_artifacts(*stage, cancellation)
            };
            if cancellation.is_requested() {
                return Err("Stage artifact verification was cancelled.".into());
            }
            match result {
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
        Ok(ValidatedResumePlan::new(reusable, invalid))
    }

    fn validate_stage_artifacts(
        &self,
        stage: PipelineStage,
        cancellation: &CancellationToken,
    ) -> Result<(), String> {
        let stage_dir = self.stage_dir(stage);
        let manifest_path = stage_dir.join(".artifacts");
        let metadata = fs::symlink_metadata(&manifest_path)
            .map_err(|error| format!("Stage artifact manifest is unavailable: {error}"))?;
        if !metadata.is_file() || metadata.len() > MAX_MANIFEST_BYTES {
            return Err("Stage artifact manifest is not a bounded regular file.".into());
        }
        let data = fs::read(&manifest_path).map_err(|error| {
            format!(
                "{} artifact manifest is unavailable: {error}",
                stage.label()
            )
        })?;
        let manifest: ArtifactManifest = serde_json::from_slice(&data).map_err(|_| {
            "Stage artifacts use an unsealed or invalid manifest; rebuild this stage.".to_string()
        })?;
        if manifest.version != ARTIFACT_MANIFEST_VERSION || manifest.outputs.stage() != stage {
            return Err("Stage artifact manifest version or stage does not match.".into());
        }
        let paths = manifest.outputs.paths();
        if paths.len() != manifest.files.len() {
            return Err("Stage artifact manifest is incomplete.".into());
        }
        let mut seen = HashSet::new();
        for (relative, record) in paths.into_iter().zip(&manifest.files) {
            if record.path != relative || !seen.insert(relative) {
                return Err("Stage artifact manifest has conflicting output roles.".into());
            }
            let path = artifact_path(&stage_dir, relative)?;
            let bytes = fs::metadata(&path)
                .map_err(|error| format!("Could not inspect stage artifact: {error}"))?
                .len();
            if bytes != record.bytes
                || fingerprint_source_file(&path, cancellation, stage.label())? != record.sha256
            {
                return Err(format!(
                    "{} artifact contents changed: {}",
                    stage.label(),
                    path.display()
                ));
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

fn artifact_path(stage_dir: &Path, relative: &Path) -> Result<PathBuf, String> {
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        || relative
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(".artifacts"))
    {
        return Err(
            "Artifact path must be a regular relative path inside its stage workspace.".into(),
        );
    }
    let mut path = stage_dir.to_owned();
    for component in std::iter::once(None).chain(relative.components().map(Some)) {
        if let Some(component) = component {
            path.push(component);
        }
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            format!(
                "Stage artifact is unavailable at {}: {error}",
                path.display()
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err("Stage artifacts cannot traverse symbolic links.".into());
        }
    }
    if !fs::metadata(&path)
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Stage artifact is not a regular file.".into());
    }
    Ok(path)
}

/// Flush a complete metadata file before atomically replacing its previous version.
pub(crate) fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create metadata directory: {error}"))?;
    let temporary = parent.join(format!(".storyteller-metadata-{}.tmp", Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("Could not create metadata temporary file: {error}"))?;
    let result = (|| {
        file.write_all(data)
            .map_err(|error| format!("Could not write metadata: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("Could not flush metadata: {error}"))?;
        drop(file);
        fs::rename(&temporary, path)
            .map_err(|error| format!("Could not publish metadata: {error}"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
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

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> JobWorkspace {
        JobWorkspace::new(
            std::env::temp_dir().join(format!("storyteller-artifacts-{}", Uuid::new_v4())),
        )
    }

    fn outputs(stage: PipelineStage) -> StageArtifacts {
        match stage {
            PipelineStage::Prepare => StageArtifacts::Prepare {
                epub: "source.epub".into(),
                audiobook: "audiobook.m4b".into(),
            },
            PipelineStage::Analyze => StageArtifacts::Analyze {
                corpus: "corpus.json".into(),
                plan: "plan.json".into(),
                transcript: "transcript.json".into(),
            },
            PipelineStage::Align => StageArtifacts::Align {
                alignment: "alignment.json".into(),
            },
            PipelineStage::ReviewAudio => StageArtifacts::ReviewAudio {
                report: "review.json".into(),
            },
            PipelineStage::Encode => StageArtifacts::Encode {
                audio: "audio.m4b".into(),
                descriptor: "descriptor.json".into(),
            },
            PipelineStage::BuildEpub => StageArtifacts::BuildEpub {
                candidate: "candidate.epub".into(),
                effective_alignment: "effective.json".into(),
            },
            PipelineStage::Validate => StageArtifacts::Validate {
                report: "validation.json".into(),
            },
        }
    }

    fn seal(workspace: &JobWorkspace, stage: PipelineStage) {
        let artifacts = outputs(stage);
        let dir = workspace.stage_dir(stage);
        fs::create_dir_all(&dir).unwrap();
        for path in artifacts.paths() {
            fs::write(dir.join(path), b"original").unwrap();
        }
        workspace
            .capture_stage_artifacts(stage, &artifacts, &CancellationToken::default())
            .unwrap();
    }

    #[test]
    fn same_size_content_change_invalidates_the_contiguous_resume_prefix() {
        let workspace = workspace();
        for stage in [
            PipelineStage::Prepare,
            PipelineStage::Analyze,
            PipelineStage::Align,
        ] {
            seal(&workspace, stage);
        }
        fs::write(
            workspace
                .stage_dir(PipelineStage::Analyze)
                .join("transcript.json"),
            b"modified",
        )
        .unwrap();
        let plan = ResumePlan::new(vec![
            PipelineStage::Prepare,
            PipelineStage::Analyze,
            PipelineStage::Align,
        ]);
        let validated = workspace
            .validate_resume_plan(&plan, &CancellationToken::default())
            .unwrap();
        assert_eq!(validated.reusable(), &[PipelineStage::Prepare]);
        assert_eq!(validated.invalid().unwrap().stage, PipelineStage::Analyze);
        assert!(validated
            .invalid()
            .unwrap()
            .reason
            .contains("contents changed"));
        workspace.clear().unwrap();
    }

    #[test]
    fn missing_required_output_cannot_replace_a_complete_manifest() {
        let workspace = workspace();
        seal(&workspace, PipelineStage::Prepare);
        let dir = workspace.stage_dir(PipelineStage::Prepare);
        let before = fs::read(dir.join(".artifacts")).unwrap();
        fs::remove_file(dir.join("audiobook.m4b")).unwrap();
        assert!(workspace
            .capture_stage_artifacts(
                PipelineStage::Prepare,
                &outputs(PipelineStage::Prepare),
                &CancellationToken::default()
            )
            .is_err());
        assert_eq!(fs::read(dir.join(".artifacts")).unwrap(), before);
        assert!(workspace
            .validate_stage_artifacts(PipelineStage::Prepare, &CancellationToken::default())
            .is_err());
        workspace.clear().unwrap();
    }

    #[test]
    fn legacy_empty_and_truncated_manifests_are_never_trusted() {
        let workspace = workspace();
        seal(&workspace, PipelineStage::Prepare);
        let manifest = workspace
            .stage_dir(PipelineStage::Prepare)
            .join(".artifacts");
        for data in ["", "source.epub\naudiobook.m4b\n", "{\"version\":2,"] {
            fs::write(&manifest, data).unwrap();
            assert!(workspace
                .validate_stage_artifacts(PipelineStage::Prepare, &CancellationToken::default())
                .is_err());
        }
        workspace.clear().unwrap();
    }

    #[test]
    fn tampered_output_roles_or_manifest_version_are_rejected() {
        let workspace = workspace();
        seal(&workspace, PipelineStage::Prepare);
        let path = workspace
            .stage_dir(PipelineStage::Prepare)
            .join(".artifacts");
        let mut manifest: ArtifactManifest =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest.files.pop();
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(workspace
            .validate_stage_artifacts(PipelineStage::Prepare, &CancellationToken::default())
            .unwrap_err()
            .contains("incomplete"));
        seal(&workspace, PipelineStage::Prepare);
        let mut manifest: ArtifactManifest =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest.files.swap(0, 1);
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(workspace
            .validate_stage_artifacts(PipelineStage::Prepare, &CancellationToken::default())
            .is_err());
        manifest.version = 99;
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(workspace
            .validate_stage_artifacts(PipelineStage::Prepare, &CancellationToken::default())
            .is_err());
        workspace.clear().unwrap();
    }

    #[test]
    fn cancellation_preserves_previous_seal_and_aborts_resume_verification() {
        let workspace = workspace();
        seal(&workspace, PipelineStage::Prepare);
        let path = workspace
            .stage_dir(PipelineStage::Prepare)
            .join(".artifacts");
        let before = fs::read(&path).unwrap();
        let token = CancellationToken::default();
        token.request();
        assert!(workspace
            .capture_stage_artifacts(
                PipelineStage::Prepare,
                &outputs(PipelineStage::Prepare),
                &token
            )
            .unwrap_err()
            .contains("cancelled"));
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(workspace
            .validate_resume_plan(&ResumePlan::new(vec![PipelineStage::Prepare]), &token)
            .unwrap_err()
            .contains("cancelled"));
        workspace.clear().unwrap();
    }

    #[test]
    fn typed_outputs_reject_wrong_stage_duplicate_and_escaping_paths() {
        let workspace = workspace();
        seal(&workspace, PipelineStage::Prepare);
        let token = CancellationToken::default();
        assert!(workspace
            .capture_stage_artifacts(
                PipelineStage::Analyze,
                &outputs(PipelineStage::Prepare),
                &token
            )
            .is_err());
        for path in ["source.epub", "../outside.epub", "", ".artifacts"] {
            let artifacts = StageArtifacts::Prepare {
                epub: "source.epub".into(),
                audiobook: path.into(),
            };
            assert!(workspace
                .capture_stage_artifacts(PipelineStage::Prepare, &artifacts, &token)
                .is_err());
        }
        workspace.clear().unwrap();
    }

    #[test]
    fn durable_review_edits_require_a_new_content_seal() {
        let workspace = workspace();
        seal(&workspace, PipelineStage::ReviewAudio);
        fs::write(
            workspace
                .stage_dir(PipelineStage::ReviewAudio)
                .join("review.json"),
            b"accepted review decisions",
        )
        .unwrap();
        let token = CancellationToken::default();
        assert!(workspace
            .validate_stage_artifacts(PipelineStage::ReviewAudio, &token)
            .is_err());
        workspace
            .capture_stage_artifacts(
                PipelineStage::ReviewAudio,
                &outputs(PipelineStage::ReviewAudio),
                &token,
            )
            .unwrap();
        workspace
            .validate_stage_artifacts(PipelineStage::ReviewAudio, &token)
            .unwrap();
        workspace.clear().unwrap();
    }

    #[test]
    fn relaunch_always_rechecks_external_publication() {
        let workspace = workspace();
        for stage in PipelineStage::ALL {
            seal(&workspace, stage);
        }
        let plan = ResumePlan::new(PipelineStage::ALL.to_vec());
        let validated = workspace
            .validate_resume_plan(&plan, &CancellationToken::default())
            .unwrap();
        assert_eq!(validated.reusable(), &PipelineStage::ALL[..6]);
        assert_eq!(validated.next_stage(), Some(PipelineStage::Validate));
        workspace.clear().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_artifact_cannot_escape_owned_workspace() {
        let workspace = workspace();
        seal(&workspace, PipelineStage::Prepare);
        let dir = workspace.stage_dir(PipelineStage::Prepare);
        fs::remove_file(dir.join("source.epub")).unwrap();
        std::os::unix::fs::symlink(dir.join("audiobook.m4b"), dir.join("source.epub")).unwrap();
        assert!(workspace
            .capture_stage_artifacts(
                PipelineStage::Prepare,
                &outputs(PipelineStage::Prepare),
                &CancellationToken::default()
            )
            .unwrap_err()
            .contains("symbolic links"));
        workspace.clear().unwrap();
    }
}
