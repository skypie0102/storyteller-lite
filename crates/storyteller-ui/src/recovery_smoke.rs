use crate::app_paths::recovery_app_root;
use std::fs;
use storyteller_core::{
    read_queue_recovery, write_queue_recovery, Job, JobInputs, JobQueue, JobSettings, JobStatus,
    PipelineStage, QueueState, ResumeContext,
};

pub(crate) fn run() -> Result<(), String> {
    let smoke_root =
        recovery_app_root().join(format!("package-recovery-smoke-{}", std::process::id()));
    if smoke_root.exists() {
        fs::remove_dir_all(&smoke_root).map_err(|error| {
            format!(
                "Could not clear stale packaged recovery smoke folder {}: {error}",
                smoke_root.display()
            )
        })?;
    }
    fs::create_dir_all(&smoke_root).map_err(|error| {
        format!(
            "Could not create packaged recovery smoke folder {}: {error}",
            smoke_root.display()
        )
    })?;

    let result = run_at(&smoke_root);
    let _ = fs::remove_dir_all(&smoke_root);
    result
}

fn run_at(root: &std::path::Path) -> Result<(), String> {
    let snapshot = root.join("queue-recovery.json");
    let settings = JobSettings::default();
    let context = resume_context(&settings);

    let mut running = Job::new(inputs(root, "running"), settings.clone())?;
    let running_id = running.id;
    running.start()?;
    complete_checkpoint(&mut running, PipelineStage::Prepare, &context)?;

    let mut review = Job::new(inputs(root, "review"), settings)?;
    let review_id = review.id;
    review.start()?;
    for stage in [
        PipelineStage::Prepare,
        PipelineStage::Analyze,
        PipelineStage::Align,
        PipelineStage::ReviewAudio,
    ] {
        complete_checkpoint(&mut review, stage, &context)?;
    }
    review.require_review()?;

    let mut queue = JobQueue::default();
    queue.enqueue(running);
    queue.enqueue(review);
    if write_queue_recovery(&snapshot, &queue)? != 2 {
        return Err("Packaged recovery smoke did not persist both recoverable jobs.".into());
    }

    let recovered = read_queue_recovery(&snapshot)?;
    if recovered.recovered_jobs != 2 {
        return Err(format!(
            "Packaged recovery smoke restored {} jobs instead of 2.",
            recovered.recovered_jobs
        ));
    }
    if recovered.queue.state() != QueueState::Paused {
        return Err("Recovered packaged smoke queue was not paused.".into());
    }

    let running = recovered.queue.job(running_id)?;
    if running.status != JobStatus::Waiting {
        return Err("Interrupted Running job did not restore as Waiting.".into());
    }
    if running.resume_plan(&context)?.reusable() != [PipelineStage::Prepare] {
        return Err(
            "Interrupted Running job did not preserve its validated Prepare checkpoint.".into(),
        );
    }

    let review = recovered.queue.job(review_id)?;
    if review.status != JobStatus::Waiting {
        return Err("NeedsReview job did not restore as Waiting.".into());
    }
    if review.resume_plan(&context)?.reusable()
        != [
            PipelineStage::Prepare,
            PipelineStage::Analyze,
            PipelineStage::Align,
        ]
    {
        return Err("NeedsReview recovery did not rewind before Review Audio.".into());
    }

    println!(
        "packaged recovery smoke passed: restored Running/NeedsReview jobs paused from {}",
        snapshot.display()
    );
    Ok(())
}

fn inputs(root: &std::path::Path, label: &str) -> JobInputs {
    JobInputs {
        title: format!("Package smoke {label}"),
        epub_path: root.join(format!("{label}.epub")),
        audiobook_path: root.join(format!("{label}.m4b")),
        output_path: root.join(format!("{label} (readaloud).epub")),
    }
}

fn resume_context(settings: &JobSettings) -> ResumeContext {
    ResumeContext {
        epub_source: "package-smoke:epub".into(),
        audiobook_source: "package-smoke:audio".into(),
        whisper_backend: "package-smoke:whisper".into(),
        alignment_backend: "package-smoke:align".into(),
        audio_backend: "package-smoke:audio-backend".into(),
        ocr_backend: "package-smoke:ocr".into(),
        epub_backend: "package-smoke:epub-backend".into(),
        effective_language: "auto".into(),
        effective_whisper_model: "package-smoke:model".into(),
        settings: settings.clone(),
    }
}

fn complete_checkpoint(
    job: &mut Job,
    stage: PipelineStage,
    context: &ResumeContext,
) -> Result<(), String> {
    job.progress.start_stage(stage, "package recovery smoke")?;
    job.progress.complete_stage(stage, 1)?;
    job.checkpoint_completed_stage(stage, context)
}
