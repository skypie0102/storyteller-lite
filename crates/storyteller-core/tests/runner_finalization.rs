use std::{
    fs,
    sync::{Arc, Mutex},
};
use storyteller_core::{
    run_pipeline, AudioEncoding, CancellationToken, HardwareProfile, Job, JobInputs, JobSettings,
    JobStatus, JobWorkspace, PipelineBackend, PipelineRunState, PipelineStage, ResourceRequest,
    ResourceScheduler, ResumeContext, RuntimeCoordinator, StageArtifacts, StagePlan,
    StageRunContext, StageRunError, StageRunOutput, StageStatus,
};
use uuid::Uuid;

#[derive(Clone)]
struct FinalizationBackend {
    finalized: Arc<Mutex<Vec<PipelineStage>>>,
    fail_validate: bool,
    wrong_outputs: bool,
    cancel_before_capture: bool,
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
        let artifacts = test_artifacts(context.stage());
        for path in artifacts.paths() {
            fs::write(stage_dir.join(path), context.stage().label())
                .map_err(|error| StageRunError::failed(error.to_string(), 0))?;
        }
        if self.cancel_before_capture {
            context.cancellation_token().request();
        }
        if self.wrong_outputs {
            return Ok(StageRunOutput::new(
                test_artifacts(PipelineStage::Analyze),
                1,
                1,
            ));
        }
        Ok(StageRunOutput::new(artifacts, 1, 1))
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

fn test_artifacts(stage: PipelineStage) -> StageArtifacts {
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
            descriptor: "encoded-audio.json".into(),
        },
        PipelineStage::BuildEpub => StageArtifacts::BuildEpub {
            candidate: "readaloud.epub".into(),
            effective_alignment: "effective-alignment.json".into(),
        },
        PipelineStage::Validate => StageArtifacts::Validate {
            report: "validation.json".into(),
        },
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
        transcription_backend: "test:transcription".into(),
        alignment_backend: "test:align".into(),
        audio_backend: "test:audio".into(),
        ocr_backend: "test:ocr".into(),
        epub_backend: "test:epub".into(),
        effective_language: "en".into(),
        effective_transcription_model: "test:model".into(),
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

fn ignore_job(_job: &Job) {}

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
        wrong_outputs: false,
        cancel_before_capture: false,
    };
    let workspace = JobWorkspace::new(root.join("workspace"));
    let mut observer = ignore_job;
    let result = run_pipeline(
        &mut job,
        &workspace,
        &mut runtime,
        &resume_context(settings),
        &CancellationToken::default(),
        &mut backend,
        &mut observer,
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
        wrong_outputs: false,
        cancel_before_capture: false,
    };
    let workspace = JobWorkspace::new(root.join("workspace"));
    let mut observer = ignore_job;
    let result = run_pipeline(
        &mut job,
        &workspace,
        &mut runtime,
        &resume_context(settings),
        &CancellationToken::default(),
        &mut backend,
        &mut observer,
    )
    .unwrap();

    assert_eq!(
        result,
        PipelineRunState::Failed("publication failed".into())
    );
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

#[test]
fn mismatched_outputs_and_cancelled_capture_never_finalize_or_checkpoint() {
    for cancel in [false, true] {
        let root = std::env::temp_dir().join(format!("storyteller-unsealed-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let mut job = sample_job(&root);
        job.start().unwrap();
        let context = resume_context(job.settings.clone());
        let mut runtime = runtime(&job);
        let finalized = Arc::new(Mutex::new(Vec::new()));
        let mut backend = FinalizationBackend {
            finalized: Arc::clone(&finalized),
            fail_validate: false,
            wrong_outputs: !cancel,
            cancel_before_capture: cancel,
        };
        let workspace = JobWorkspace::new(root.join("workspace"));
        let result = run_pipeline(
            &mut job,
            &workspace,
            &mut runtime,
            &context,
            &CancellationToken::default(),
            &mut backend,
            &mut ignore_job,
        )
        .unwrap();
        if cancel {
            assert_eq!(result, PipelineRunState::Cancelled);
        } else {
            assert!(matches!(result, PipelineRunState::Failed(_)));
        }
        assert!(finalized.lock().unwrap().is_empty());
        assert!(job.resume_plan(&context).unwrap().reusable().is_empty());
        assert!(!workspace
            .stage_dir(PipelineStage::Prepare)
            .join(".artifacts")
            .exists());
        fs::remove_dir_all(root).unwrap();
    }
}
