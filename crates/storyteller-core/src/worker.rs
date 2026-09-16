use crate::{
    fingerprint_job_sources, run_pipeline, CancellationToken, Job, JobOutcome, JobWorkspace,
    PipelineBackend, PipelineRunState, ResumeContext, RuntimeCoordinator, SourceFingerprints,
    ValidatedResumePlan,
};
use std::{sync::mpsc, thread};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineEnvironment {
    pub whisper_backend: String,
    pub alignment_backend: String,
    pub audio_backend: String,
    pub ocr_backend: String,
    pub epub_backend: String,
    pub effective_language: String,
    pub effective_whisper_model: String,
}

impl PipelineEnvironment {
    pub fn validate(&self) -> Result<(), String> {
        for (label, value) in [
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

    pub fn resume_context(
        &self,
        job: &Job,
        sources: &SourceFingerprints,
    ) -> Result<ResumeContext, String> {
        self.validate()?;
        let context = ResumeContext {
            epub_source: sources.epub_source().into(),
            audiobook_source: sources.audiobook_source().into(),
            whisper_backend: self.whisper_backend.clone(),
            alignment_backend: self.alignment_backend.clone(),
            audio_backend: self.audio_backend.clone(),
            ocr_backend: self.ocr_backend.clone(),
            epub_backend: self.epub_backend.clone(),
            effective_language: self.effective_language.clone(),
            effective_whisper_model: self.effective_whisper_model.clone(),
            settings: job.settings.clone(),
        };
        context.validate()?;
        Ok(context)
    }
}

#[derive(Debug)]
pub struct WorkerResult {
    pub job: Job,
    pub run_result: Result<PipelineRunState, String>,
}

pub struct PipelineWorkerHandle {
    cancellation: CancellationToken,
    updates: mpsc::Receiver<Job>,
    join: thread::JoinHandle<WorkerResult>,
}

impl PipelineWorkerHandle {
    pub fn request_cancellation(&self) {
        self.cancellation.request();
    }

    pub fn drain_updates(&self) -> Vec<Job> {
        self.updates.try_iter().collect()
    }

    pub fn is_finished(&self) -> bool {
        self.join.is_finished()
    }

    pub fn join(self) -> Result<WorkerResult, String> {
        self.join
            .join()
            .map_err(|_| "Pipeline worker thread panicked.".to_string())
    }
}

pub fn spawn_pipeline_worker<B: PipelineBackend>(
    job: Job,
    workspace: JobWorkspace,
    mut runtime: RuntimeCoordinator,
    resume_context: ResumeContext,
    mut backend: B,
) -> Result<PipelineWorkerHandle, String> {
    resume_context.validate()?;
    spawn_worker(job, move |job, cancellation, sender| {
        let validated_resume = match apply_validated_resume(job, &workspace, &resume_context) {
            Ok(plan) => plan,
            Err(error) => return finish_preflight_failure(job, sender, error),
        };
        job.progress.set_activity(resume_activity(&validated_resume))?;
        let _ = sender.send(job.clone());

        let mut observer = |snapshot: &Job| {
            let _ = sender.send(snapshot.clone());
        };
        run_pipeline(
            job,
            &workspace,
            &mut runtime,
            &resume_context,
            cancellation,
            &mut backend,
            &mut observer,
        )
    })
}

pub fn spawn_pipeline_worker_with_preflight<B: PipelineBackend>(
    job: Job,
    workspace: JobWorkspace,
    mut runtime: RuntimeCoordinator,
    environment: PipelineEnvironment,
    mut backend: B,
) -> Result<PipelineWorkerHandle, String> {
    environment.validate()?;
    spawn_worker(job, move |job, cancellation, sender| {
        job.progress.set_activity("Fingerprinting sources")?;
        let _ = sender.send(job.clone());

        let sources = match fingerprint_job_sources(job, cancellation) {
            Ok(sources) => sources,
            Err(_) if cancellation.is_requested() => {
                let _ = job.progress.set_activity("Processing cancelled");
                let _ = job.finish(JobOutcome::Cancelled, 0);
                let _ = sender.send(job.clone());
                return Ok(PipelineRunState::Cancelled);
            }
            Err(error) => {
                let _ = job.progress.set_activity("Source preflight failed");
                let _ = job.finish(JobOutcome::Failed(error.clone()), 0);
                let _ = sender.send(job.clone());
                return Ok(PipelineRunState::Failed(error));
            }
        };
        let resume_context = match environment.resume_context(job, &sources) {
            Ok(context) => context,
            Err(error) => {
                let _ = job.progress.set_activity("Source preflight failed");
                let _ = job.finish(JobOutcome::Failed(error.clone()), 0);
                let _ = sender.send(job.clone());
                return Ok(PipelineRunState::Failed(error));
            }
        };
        let validated_resume = match apply_validated_resume(job, &workspace, &resume_context) {
            Ok(plan) => plan,
            Err(error) => return finish_preflight_failure(job, sender, error),
        };
        job.progress.set_activity(resume_activity(&validated_resume))?;
        let _ = sender.send(job.clone());

        let mut observer = |snapshot: &Job| {
            let _ = sender.send(snapshot.clone());
        };
        run_pipeline(
            job,
            &workspace,
            &mut runtime,
            &resume_context,
            cancellation,
            &mut backend,
            &mut observer,
        )
    })
}

fn apply_validated_resume(
    job: &mut Job,
    workspace: &JobWorkspace,
    resume_context: &ResumeContext,
) -> Result<ValidatedResumePlan, String> {
    job.invalidate_stale_cache(resume_context)?;
    let resume_plan = job.resume_plan(resume_context)?;
    let validated = workspace.validate_resume_plan(&resume_plan);
    job.apply_validated_resume_plan(&validated)?;
    Ok(validated)
}

fn resume_activity(plan: &ValidatedResumePlan) -> String {
    if let Some(invalid) = plan.invalid() {
        return format!(
            "Rebuilding from {}: {}",
            invalid.stage.label(),
            invalid.reason
        );
    }
    match (plan.reusable().len(), plan.next_stage()) {
        (0, _) => "Starting pipeline".into(),
        (count, Some(stage)) => format!("Resuming at {} ({count} cached stages)", stage.label()),
        (count, None) => format!("Verified all {count} pipeline stages from cache"),
    }
}

fn finish_preflight_failure(
    job: &mut Job,
    sender: &mpsc::Sender<Job>,
    error: String,
) -> Result<PipelineRunState, String> {
    let _ = job.progress.set_activity("Resume preflight failed");
    let _ = job.finish(JobOutcome::Failed(error.clone()), 0);
    let _ = sender.send(job.clone());
    Ok(PipelineRunState::Failed(error))
}

fn spawn_worker<F>(job: Job, run: F) -> Result<PipelineWorkerHandle, String>
where
    F: FnOnce(&mut Job, &CancellationToken, &mpsc::Sender<Job>) -> Result<PipelineRunState, String>
        + Send
        + 'static,
{
    if !job.status.is_active() {
        return Err("Pipeline worker requires an active job.".into());
    }
    let cancellation = CancellationToken::default();
    let thread_cancellation = cancellation.clone();
    let (sender, updates) = mpsc::channel();
    let join = thread::Builder::new()
        .name(format!("storyteller-job-{}", job.id))
        .spawn(move || {
            let mut job = job;
            let _ = sender.send(job.clone());
            let run_result = run(&mut job, &thread_cancellation, &sender);
            let _ = sender.send(job.clone());
            WorkerResult { job, run_result }
        })
        .map_err(|error| format!("Could not spawn pipeline worker: {error}"))?;

    Ok(PipelineWorkerHandle {
        cancellation,
        updates,
        join,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JobInputs, JobSettings, PipelineStage, StageStatus};
    use std::{fs, path::PathBuf};
    use uuid::Uuid;

    fn job() -> Job {
        Job::new(
            JobInputs {
                title: "Resume Test".into(),
                epub_path: PathBuf::from("source.epub"),
                audiobook_path: PathBuf::from("audio.m4b"),
                output_path: PathBuf::from("output.epub"),
            },
            JobSettings::default(),
        )
        .unwrap()
    }

    fn context(job: &Job) -> ResumeContext {
        ResumeContext {
            epub_source: "epub:one".into(),
            audiobook_source: "audio:one".into(),
            whisper_backend: "whisper:one".into(),
            alignment_backend: "align:one".into(),
            audio_backend: "audio:one".into(),
            ocr_backend: "ocr:one".into(),
            epub_backend: "epub:one".into(),
            effective_language: "auto".into(),
            effective_whisper_model: "model:one".into(),
            settings: job.settings.clone(),
        }
    }

    fn checkpoint(job: &mut Job, stage: PipelineStage, context: &ResumeContext) {
        job.progress.start_stage(stage, "test").unwrap();
        job.progress.complete_stage(stage, 1).unwrap();
        job.checkpoint_completed_stage(stage, context).unwrap();
    }

    fn temp_workspace() -> JobWorkspace {
        JobWorkspace::new(
            std::env::temp_dir().join(format!("storyteller-resume-{}", Uuid::new_v4())),
        )
    }

    fn capture_artifact(workspace: &JobWorkspace, stage: PipelineStage, name: &str) {
        let stage_dir = workspace.stage_dir(stage);
        fs::create_dir_all(&stage_dir).unwrap();
        fs::write(stage_dir.join(name), b"artifact").unwrap();
        workspace
            .capture_stage_artifacts(stage, &[PathBuf::from(name)])
            .unwrap();
    }

    #[test]
    fn resume_preflight_reuses_only_contiguous_valid_artifacts() {
        let mut job = job();
        let context = context(&job);
        checkpoint(&mut job, PipelineStage::Prepare, &context);
        checkpoint(&mut job, PipelineStage::Analyze, &context);

        let workspace = temp_workspace();
        capture_artifact(&workspace, PipelineStage::Prepare, "prepared.bin");

        let validated = apply_validated_resume(&mut job, &workspace, &context).unwrap();
        assert_eq!(validated.reusable(), &[PipelineStage::Prepare]);
        assert_eq!(
            validated.invalid().map(|invalid| invalid.stage),
            Some(PipelineStage::Analyze)
        );
        assert_eq!(
            job.progress.stages()[PipelineStage::Prepare.index()].status,
            StageStatus::Cached
        );
        assert_eq!(
            job.progress.stages()[PipelineStage::Analyze.index()].status,
            StageStatus::Pending
        );
        assert_eq!(
            job.resume_plan(&context).unwrap().reusable(),
            &[PipelineStage::Prepare]
        );

        let _ = workspace.clear();
    }

    #[test]
    fn resume_preflight_invalidates_changed_backend_from_first_affected_stage() {
        let mut job = job();
        let context = context(&job);
        checkpoint(&mut job, PipelineStage::Prepare, &context);
        checkpoint(&mut job, PipelineStage::Analyze, &context);

        let workspace = temp_workspace();
        capture_artifact(&workspace, PipelineStage::Prepare, "prepared.bin");
        capture_artifact(&workspace, PipelineStage::Analyze, "transcript.json");

        let mut changed = context.clone();
        changed.whisper_backend = "whisper:two".into();
        let validated = apply_validated_resume(&mut job, &workspace, &changed).unwrap();

        assert_eq!(validated.reusable(), &[PipelineStage::Prepare]);
        assert!(validated.invalid().is_none());
        assert_eq!(
            job.progress.stages()[PipelineStage::Prepare.index()].status,
            StageStatus::Cached
        );
        assert_eq!(
            job.progress.stages()[PipelineStage::Analyze.index()].status,
            StageStatus::Pending
        );

        let _ = workspace.clear();
    }
}
