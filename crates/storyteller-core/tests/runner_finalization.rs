use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use storyteller_core::{
    run_pipeline, AudioEncoding, CancellationToken, HardwareProfile, Job, JobInputs, JobSettings,
    JobStatus, JobWorkspace, PipelineBackend, PipelineRunState, PipelineStage, ResourceRequest,
    ResourceScheduler, ResumeContext, RuntimeCoordinator, StagePlan, StageRunContext,
    StageRunError, StageRunOutput, StageStatus,
};
use uuid::Uuid;

#[derive(Clone)]
struct FinalizationBackend {
    finalized: Arc<Mutex<Vec<PipelineStage>>>,
    fail_validate: bool,
}

impl PipelineBackend for FinalizationBackend {
    fn plan_stage(&mut self, _job: &Job, stage: PipelineStage) -> Result<StagePlan, String> {
        Ok(StagePlan::run(
            format!("Running {}", stage.label()),
            ResourceRequest::io_heavy(1),
            0,
        ))
    }

    fn run_stage(
        &mut self,
        context: &mut StageRunContext<'_>,
    ) -> Result<StageRunOutput, StageRunError> {
        let stage_dir = context.workspace().stage_dir(context.stage());
        fs::create_dir_all(&stage_dir)
            .map_err(|error| StageRunError::failed(error.to_string(), 0))?;
        fs::write(stage_dir.join("artifact.txt"), context.stage().label())
            .map_err(|error| StageRunError::failed(error.to_string(), 0))?;
        Ok(StageRunOutput::new(
            vec![PathBuf::from("artifact.txt")],
            1,
            1,
        ))
    }

    fn finalize_stage(
        &mut self,
        context: &mut StageRunContext<'_>,
        _output: &StageRunOutput,
    ) -> Result<(), StageRunError> {
        if !context
            .workspace()
            .stage_dir(context.stage())
            .join(".artifacts")
            .is_file()
        {
            return Err(StageRunError::failed(
                "Stage finalization ran before artifact capture.",
                0,
            ));
        }
        self.finalized.lock().unwrap().push(context.stage());
        if self.fail_validate && context.stage() == PipelineStage::Validate {
            return Err(StageRunError::failed("publication failed", 0));
        }
        Ok(())
    }
}

fn sample_job(root: &std::path::Path) -> Job {
    Job::new(
        JobInputs {
            title: "Finalization".into(),
            epub_path: root.join("source.epub"),
            audiobook_path: root.join("audio.m4b"),
            output_path: root.join("Finalization (readaloud).epub"),
        },
        JobSettings {
            audio: AudioEncoding::copy(),
            ..JobSettings::default()
        },
    )
    .unwrap()
}

fn resume_context(settings: JobSettings) -> ResumeContext {
    ResumeContext {
        epub_source: "sha256:epub".into(),
        audiobook_source: "sha256:audio".into(),
        whisper_backend: "test:whisper".into(),
        alignment_backend: "test:align".into(),
        audio_backend: "test:audio".into(),
        ocr_backend: "test:ocr".into(),
        epub_backend: "test:epub".into(),
        effective_language: "en".into(),
        effective_whisper_model: "test:model".into(),
        settings,
    }
}

fn runtime(job: &Job) -> RuntimeCoordinator {
    let scheduler = ResourceScheduler::automatic(&HardwareProfile {
        logical_cpu_threads: 2,
        memory_gib: None,
        gpu_backend: None,
        gpu_vram_mib: None,
    })
    .unwrap();
    let mut runtime = RuntimeCoordinator::new(scheduler);
    runtime.register_job(job.id).unwrap();
    runtime
}

#[test]
fn finalization_runs_after_artifact_capture_for_every_stage() {
    let root = std::env::temp_dir().join(format!("storyteller-finalize-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let mut job = sample_job(&root);
    job.start().unwrap();
    let settings = job.settings.clone();
    let mut runtime = runtime(&job);
    let finalized = Arc::new(Mutex::new(Vec::new()));
    let mut backend = FinalizationBackend {
        finalized: Arc::clone(&finalized),
        fail_validate: false,
    };
    let workspace = JobWorkspace::new(root.join("workspace"));
    let result = run_pipeline(
        &mut job,
        &workspace,
        &mut runtime,
        &resume_context(settings),
        &CancellationToken::default(),
        &mut backend,
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(result, PipelineRunState::Completed);
    assert_eq!(job.status, JobStatus::Completed);
    assert_eq!(*finalized.lock().unwrap(), PipelineStage::ALL);
    assert!(job
        .progress
        .stages()
        .iter()
        .all(|stage| stage.status == StageStatus::Completed));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn failed_validate_finalization_keeps_validate_failed() {
    let root = std::env::temp_dir().join(format!("storyteller-finalize-fail-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let mut job = sample_job(&root);
    job.start().unwrap();
    let settings = job.settings.clone();
    let mut runtime = runtime(&job);
    let finalized = Arc::new(Mutex::new(Vec::new()));
    let mut backend = FinalizationBackend {
        finalized,
        fail_validate: true,
    };
    let workspace = JobWorkspace::new(root.join("workspace"));
    let result = run_pipeline(
        &mut job,
        &workspace,
        &mut runtime,
        &resume_context(settings),
        &CancellationToken::default(),
        &mut backend,
        &mut |_| {},
    )
    .unwrap();

    assert_eq!(result, PipelineRunState::Failed("publication failed".into()));
    assert_eq!(job.status, JobStatus::Failed);
    assert_eq!(
        job.progress.stages()[PipelineStage::Validate.index()].status,
        StageStatus::Failed
    );
    assert!(workspace
        .stage_dir(PipelineStage::Validate)
        .join(".artifacts")
        .is_file());
    let _ = fs::remove_dir_all(root);
}
