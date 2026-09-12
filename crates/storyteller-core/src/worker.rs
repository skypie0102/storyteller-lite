use crate::{
    fingerprint_job_sources, run_pipeline, CancellationToken, Job, JobOutcome, JobWorkspace,
    PipelineBackend, PipelineRunState, ResumeContext, RuntimeCoordinator, SourceFingerprints,
};
use std::{sync::mpsc, thread};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineEnvironment {
    pub whisper_backend: String,
    pub alignment_backend: String,
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
            ("OCR backend", self.ocr_backend.as_str()),
            ("EPUB backend", self.epub_backend.as_str()),
            ("Effective language", self.effective_language.as_str()),
            ("Effective Whisper model", self.effective_whisper_model.as_str()),
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
        if let Err(error) = job.progress.set_activity("Fingerprinting sources") {
            return Err(error);
        }
        let _ = sender.send(job.clone());

        let sources = match fingerprint_job_sources(job, cancellation) {
            Ok(sources) => sources,
            Err(error) if cancellation.is_requested() => {
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
