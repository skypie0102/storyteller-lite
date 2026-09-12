use std::{path::PathBuf, time::Instant};
use storyteller_core::{
    prepare_job_sources, spawn_pipeline_worker_with_preflight, HardwareProfile, Job, JobWorkspace,
    PipelineBackend, PipelineEnvironment, PipelineStage, PipelineWorkerHandle, ResourceRequest,
    ResourceScheduler, RuntimeCoordinator, StagePlan, StageRunContext, StageRunError,
    StageRunOutput,
};

pub(crate) struct LitePipelineBackend {
    attempt_started: Instant,
}

impl LitePipelineBackend {
    pub(crate) fn new() -> Self {
        Self {
            attempt_started: Instant::now(),
        }
    }

    fn elapsed_millis(&self) -> u64 {
        self.attempt_started
            .elapsed()
            .as_millis()
            .min(u64::MAX as u128) as u64
    }
}

impl PipelineBackend for LitePipelineBackend {
    fn plan_stage(&mut self, _job: &Job, stage: PipelineStage) -> Result<StagePlan, String> {
        match stage {
            PipelineStage::Prepare => Ok(StagePlan::run(
                "Preparing source files",
                ResourceRequest::io_heavy(1),
                self.elapsed_millis(),
            )),
            _ => Err(format!(
                "{} backend is not implemented in Storyteller Lite yet.",
                stage.label()
            )),
        }
    }

    fn run_stage(
        &mut self,
        context: &mut StageRunContext<'_>,
    ) -> Result<StageRunOutput, StageRunError> {
        if context.stage() != PipelineStage::Prepare {
            return Err(StageRunError::failed(
                format!(
                    "{} backend is not implemented in Storyteller Lite yet.",
                    context.stage().label()
                ),
                self.elapsed_millis(),
            ));
        }

        let stage_started = Instant::now();
        let activity_at = self.elapsed_millis();
        context
            .set_activity("Staging source EPUB and audiobook", activity_at)
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        let cancellation = context.cancellation_token();
        let prepared = match prepare_job_sources(context.job(), context.workspace(), &cancellation)
        {
            Ok(prepared) => prepared,
            Err(error) if cancellation.is_requested() => {
                return Err(StageRunError::cancelled(error, self.elapsed_millis()));
            }
            Err(error) => return Err(StageRunError::failed(error, self.elapsed_millis())),
        };

        let completed_at = self.elapsed_millis();
        context
            .set_stage_percent(100, completed_at)
            .map_err(|error| StageRunError::failed(error, completed_at))?;
        Ok(StageRunOutput::new(
            prepared.relative_artifacts(),
            stage_started.elapsed().as_secs(),
            completed_at,
        ))
    }
}

pub(crate) fn spawn_job_worker(job: Job) -> Result<PipelineWorkerHandle, String> {
    let scheduler = ResourceScheduler::automatic(&HardwareProfile {
        logical_cpu_threads: std::thread::available_parallelism()
            .map(|threads| threads.get())
            .unwrap_or(1),
        memory_gib: None,
        gpu_backend: None,
        gpu_vram_mib: None,
    })?;
    let mut runtime = RuntimeCoordinator::new(scheduler);
    runtime.register_job(job.id)?;
    let workspace = JobWorkspace::for_job(workspace_base(), job.id);

    spawn_pipeline_worker_with_preflight(
        job,
        workspace,
        runtime,
        pipeline_environment(),
        LitePipelineBackend::new(),
    )
}

fn pipeline_environment() -> PipelineEnvironment {
    PipelineEnvironment {
        whisper_backend: "unimplemented:whisper".into(),
        alignment_backend: "unimplemented:alignment".into(),
        ocr_backend: "unimplemented:ocr".into(),
        epub_backend: "unimplemented:epub".into(),
        effective_language: "unresolved:auto".into(),
        effective_whisper_model: "unresolved".into(),
    }
}

fn workspace_base() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Storyteller OneClick Lite")
        .join("jobs")
}
