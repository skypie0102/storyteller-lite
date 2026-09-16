use crate::{
    job::StageCheckpoint, AudioBitrate, AudioCodec, AudioEncoding, AudioReviewPolicy, Job,
    JobInputs, JobQueue, JobSettings, JobStatus, PipelineProgress, PipelineStage,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const QUEUE_RECOVERY_VERSION: u32 = 1;

#[derive(Debug)]
pub struct QueueRecovery {
    pub queue: JobQueue,
    pub recovered_jobs: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct QueueRecoveryFile {
    version: u32,
    jobs: Vec<JobRecoveryRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JobRecoveryRecord {
    id: String,
    title: String,
    epub_path: PathBuf,
    audiobook_path: PathBuf,
    output_path: PathBuf,
    audio_codec: String,
    audio_bitrate_kbps: Option<u16>,
    language: Option<String>,
    whisper_model: String,
    audio_review_policy: AudioReviewPolicy,
    whisper_workers: usize,
    previous_status: RecoveryStatus,
    checkpoints: Vec<CheckpointRecord>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum RecoveryStatus {
    Waiting,
    Running,
    NeedsReview,
}

#[derive(Debug, Serialize, Deserialize)]
struct CheckpointRecord {
    stage: String,
    fingerprint: String,
}

pub fn write_queue_recovery(path: &Path, queue: &JobQueue) -> Result<usize, String> {
    let jobs = queue
        .jobs()
        .iter()
        .filter_map(JobRecoveryRecord::from_job)
        .collect::<Vec<_>>();
    let backup = backup_recovery_path(path)?;
    if jobs.is_empty() {
        for candidate in [path, backup.as_path()] {
            if candidate.exists() {
                fs::remove_file(candidate).map_err(|error| {
                    format!(
                        "Could not remove completed queue recovery file {}: {error}",
                        candidate.display()
                    )
                })?;
            }
        }
        return Ok(0);
    }

    let recovered_jobs = jobs.len();
    let parent = path
        .parent()
        .ok_or("Queue recovery path has no parent directory.")?;
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "Could not create queue recovery directory {}: {error}",
            parent.display()
        )
    })?;
    let encoded = serde_json::to_vec_pretty(&QueueRecoveryFile {
        version: QUEUE_RECOVERY_VERSION,
        jobs,
    })
    .map_err(|error| format!("Could not serialize queue recovery state: {error}"))?;

    let temporary = temporary_recovery_path(path)?;
    if temporary.exists() {
        fs::remove_file(&temporary).map_err(|error| {
            format!(
                "Could not remove stale queue recovery temporary file {}: {error}",
                temporary.display()
            )
        })?;
    }
    let mut file = fs::File::create(&temporary).map_err(|error| {
        format!(
            "Could not create queue recovery temporary file {}: {error}",
            temporary.display()
        )
    })?;
    file.write_all(&encoded).map_err(|error| {
        format!(
            "Could not write queue recovery temporary file {}: {error}",
            temporary.display()
        )
    })?;
    file.sync_all().map_err(|error| {
        format!(
            "Could not flush queue recovery temporary file {}: {error}",
            temporary.display()
        )
    })?;
    drop(file);

    let rotated_primary = path.exists();
    if rotated_primary {
        if backup.exists() {
            fs::remove_file(&backup).map_err(|error| {
                format!(
                    "Could not remove stale queue recovery backup {}: {error}",
                    backup.display()
                )
            })?;
        }
        fs::rename(path, &backup).map_err(|error| {
            format!(
                "Could not preserve previous queue recovery file {} as {}: {error}",
                path.display(),
                backup.display()
            )
        })?;
    }

    if let Err(error) = fs::rename(&temporary, path) {
        let restoration = if rotated_primary && backup.exists() {
            match fs::rename(&backup, path) {
                Ok(()) => " Previous recovery snapshot was restored.".to_string(),
                Err(restore_error) => format!(
                    " Previous recovery snapshot remains at {} because restoring it failed: {restore_error}.",
                    backup.display()
                ),
            }
        } else {
            String::new()
        };
        return Err(format!(
            "Could not publish queue recovery file {}: {error}.{restoration}",
            path.display()
        ));
    }
    if backup.exists() {
        let _ = fs::remove_file(&backup);
    }
    Ok(recovered_jobs)
}

pub fn read_queue_recovery(path: &Path) -> Result<QueueRecovery, String> {
    let backup = backup_recovery_path(path)?;
    let source = if path.exists() {
        path
    } else if backup.exists() {
        backup.as_path()
    } else {
        return Ok(QueueRecovery {
            queue: JobQueue::default(),
            recovered_jobs: 0,
        });
    };
    let bytes = fs::read(source).map_err(|error| {
        format!(
            "Could not read queue recovery file {}: {error}",
            source.display()
        )
    })?;
    let file: QueueRecoveryFile = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Queue recovery file is invalid: {error}"))?;
    if file.version != QUEUE_RECOVERY_VERSION {
        return Err(format!(
            "Queue recovery file version {} is unsupported; expected {}.",
            file.version, QUEUE_RECOVERY_VERSION
        ));
    }

    let mut ids = HashSet::new();
    let mut queue = JobQueue::default();
    for record in file.jobs {
        let job = record.into_job()?;
        if !ids.insert(job.id) {
            return Err(format!(
                "Queue recovery file contains duplicate job id {}.",
                job.id
            ));
        }
        queue.enqueue(job);
    }
    let recovered_jobs = queue.jobs().len();
    if recovered_jobs > 0 {
        queue.request_pause_after_current();
    }
    Ok(QueueRecovery {
        queue,
        recovered_jobs,
    })
}

impl JobRecoveryRecord {
    fn from_job(job: &Job) -> Option<Self> {
        let previous_status = match job.status {
            JobStatus::Waiting => RecoveryStatus::Waiting,
            JobStatus::Running => RecoveryStatus::Running,
            JobStatus::NeedsReview => RecoveryStatus::NeedsReview,
            JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled => return None,
        };
        Some(Self {
            id: job.id.to_string(),
            title: job.inputs.title.clone(),
            epub_path: job.inputs.epub_path.clone(),
            audiobook_path: job.inputs.audiobook_path.clone(),
            output_path: job.inputs.output_path.clone(),
            audio_codec: codec_name(job.settings.audio.codec).into(),
            audio_bitrate_kbps: job.settings.audio.bitrate.map(AudioBitrate::kbps),
            language: job.settings.language.clone(),
            whisper_model: job.settings.whisper_model.clone(),
            audio_review_policy: job.settings.audio_review_policy,
            whisper_workers: job.settings.whisper_workers,
            previous_status,
            checkpoints: job
                .checkpoints
                .iter()
                .map(|checkpoint| CheckpointRecord {
                    stage: stage_name(checkpoint.stage).into(),
                    fingerprint: checkpoint.fingerprint.clone(),
                })
                .collect(),
        })
    }

    fn into_job(self) -> Result<Job, String> {
        let id = self
            .id
            .parse()
            .map_err(|error| format!("Recovered job id is invalid: {error}"))?;
        let codec = parse_codec(&self.audio_codec)?;
        let bitrate = self.audio_bitrate_kbps.map(parse_bitrate).transpose()?;
        let audio = AudioEncoding::new(codec, bitrate)?;
        let settings = JobSettings {
            audio,
            language: self.language,
            whisper_model: self.whisper_model,
            audio_review_policy: self.audio_review_policy,
            whisper_workers: self.whisper_workers,
        };
        let mut job = Job::new(
            JobInputs {
                title: self.title,
                epub_path: self.epub_path,
                audiobook_path: self.audiobook_path,
                output_path: self.output_path,
            },
            settings,
        )?;
        job.id = id;
        job.status = JobStatus::Waiting;
        job.progress = PipelineProgress::default();
        job.last_error = None;
        job.runtime_seconds = 0;

        let review_index = PipelineStage::ReviewAudio.index();
        let mut checkpoints = Vec::new();
        let mut last_index = None;
        for checkpoint in self.checkpoints {
            let stage = parse_stage(&checkpoint.stage)?;
            if checkpoint.fingerprint.trim().is_empty() {
                return Err(format!(
                    "Recovered {} checkpoint has a blank fingerprint.",
                    stage.label()
                ));
            }
            if let Some(previous) = last_index {
                if stage.index() <= previous {
                    return Err("Recovered checkpoints are not in strict stage order.".into());
                }
            }
            last_index = Some(stage.index());
            if self.previous_status == RecoveryStatus::NeedsReview && stage.index() >= review_index
            {
                continue;
            }
            checkpoints.push(StageCheckpoint {
                stage,
                fingerprint: checkpoint.fingerprint,
            });
        }
        ensure_contiguous(&checkpoints)?;
        job.checkpoints = checkpoints;
        Ok(job)
    }
}

fn ensure_contiguous(checkpoints: &[StageCheckpoint]) -> Result<(), String> {
    for (index, checkpoint) in checkpoints.iter().enumerate() {
        if checkpoint.stage.index() != index {
            return Err(format!(
                "Recovered checkpoint prefix skips {}.",
                PipelineStage::ALL[index].label()
            ));
        }
    }
    Ok(())
}

fn codec_name(codec: AudioCodec) -> &'static str {
    match codec {
        AudioCodec::Copy => "copy",
        AudioCodec::Opus => "opus",
        AudioCodec::Aac => "aac",
    }
}

fn parse_codec(value: &str) -> Result<AudioCodec, String> {
    match value {
        "copy" => Ok(AudioCodec::Copy),
        "opus" => Ok(AudioCodec::Opus),
        "aac" => Ok(AudioCodec::Aac),
        _ => Err(format!("Recovered audio codec is unsupported: {value}")),
    }
}

fn parse_bitrate(value: u16) -> Result<AudioBitrate, String> {
    match value {
        32 => Ok(AudioBitrate::Kbps32),
        64 => Ok(AudioBitrate::Kbps64),
        96 => Ok(AudioBitrate::Kbps96),
        _ => Err(format!(
            "Recovered audio bitrate is unsupported: {value} kbps"
        )),
    }
}

fn stage_name(stage: PipelineStage) -> &'static str {
    match stage {
        PipelineStage::Prepare => "prepare",
        PipelineStage::Analyze => "analyze",
        PipelineStage::Align => "align",
        PipelineStage::ReviewAudio => "review_audio",
        PipelineStage::Encode => "encode",
        PipelineStage::BuildEpub => "build_epub",
        PipelineStage::Validate => "validate",
    }
}

fn parse_stage(value: &str) -> Result<PipelineStage, String> {
    match value {
        "prepare" => Ok(PipelineStage::Prepare),
        "analyze" => Ok(PipelineStage::Analyze),
        "align" => Ok(PipelineStage::Align),
        "review_audio" => Ok(PipelineStage::ReviewAudio),
        "encode" => Ok(PipelineStage::Encode),
        "build_epub" => Ok(PipelineStage::BuildEpub),
        "validate" => Ok(PipelineStage::Validate),
        _ => Err(format!("Recovered pipeline stage is unsupported: {value}")),
    }
}

fn temporary_recovery_path(path: &Path) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("Queue recovery filename is not valid UTF-8.")?;
    Ok(path.with_file_name(format!(".{file_name}.tmp")))
}

fn backup_recovery_path(path: &Path) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("Queue recovery filename is not valid UTF-8.")?;
    Ok(path.with_file_name(format!("{file_name}.bak")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResumeContext;
    use std::fs;
    use uuid::Uuid;

    fn inputs(title: &str) -> JobInputs {
        JobInputs {
            title: title.into(),
            epub_path: format!("{title}.epub").into(),
            audiobook_path: format!("{title}.m4b").into(),
            output_path: format!("{title} (readaloud).epub").into(),
        }
    }

    fn context(settings: &JobSettings) -> ResumeContext {
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
            settings: settings.clone(),
        }
    }

    fn complete_checkpoint(job: &mut Job, stage: PipelineStage, context: &ResumeContext) {
        job.progress.start_stage(stage, "test").unwrap();
        job.progress.complete_stage(stage, 1).unwrap();
        job.checkpoint_completed_stage(stage, context).unwrap();
    }

    fn recovery_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "storyteller-queue-recovery-{}.json",
            Uuid::new_v4()
        ))
    }

    #[test]
    fn interrupted_publication_falls_back_to_previous_backup() {
        let path = recovery_path();
        let backup = backup_recovery_path(&path).unwrap();
        let mut queue = JobQueue::default();
        queue.enqueue(Job::new(inputs("backup"), JobSettings::default()).unwrap());

        write_queue_recovery(&path, &queue).unwrap();
        fs::rename(&path, &backup).unwrap();

        let recovered = read_queue_recovery(&path).unwrap();
        assert_eq!(recovered.recovered_jobs, 1);
        assert_eq!(recovered.queue.jobs()[0].inputs.title, "backup");
        assert_eq!(recovered.queue.state(), crate::QueueState::Paused);

        let _ = fs::remove_file(path);
        let _ = fs::remove_file(backup);
    }

    #[test]
    fn successful_replacement_cleans_backup_and_keeps_latest_snapshot() {
        let path = recovery_path();
        let backup = backup_recovery_path(&path).unwrap();
        let mut first = JobQueue::default();
        first.enqueue(Job::new(inputs("first"), JobSettings::default()).unwrap());
        write_queue_recovery(&path, &first).unwrap();

        let mut second = JobQueue::default();
        second.enqueue(Job::new(inputs("second"), JobSettings::default()).unwrap());
        write_queue_recovery(&path, &second).unwrap();

        assert!(!backup.exists());
        let recovered = read_queue_recovery(&path).unwrap();
        assert_eq!(recovered.recovered_jobs, 1);
        assert_eq!(recovered.queue.jobs()[0].inputs.title, "second");

        let _ = fs::remove_file(path);
        let _ = fs::remove_file(backup);
    }

    #[test]
    fn running_job_restores_waiting_and_queue_is_paused() {
        let path = recovery_path();
        let settings = JobSettings::default();
        let context = context(&settings);
        let mut job = Job::new(inputs("running"), settings).unwrap();
        let id = job.id;
        job.start().unwrap();
        complete_checkpoint(&mut job, PipelineStage::Prepare, &context);
        let mut queue = JobQueue::default();
        queue.enqueue(job);

        assert_eq!(write_queue_recovery(&path, &queue).unwrap(), 1);
        let recovered = read_queue_recovery(&path).unwrap();
        assert_eq!(recovered.recovered_jobs, 1);
        assert_eq!(recovered.queue.state(), crate::QueueState::Paused);
        let restored = recovered.queue.job(id).unwrap();
        assert_eq!(restored.status, JobStatus::Waiting);
        assert_eq!(
            restored.resume_plan(&context).unwrap().reusable(),
            &[PipelineStage::Prepare]
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn review_wait_rewinds_review_checkpoint_before_restore() {
        let path = recovery_path();
        let settings = JobSettings::default();
        let context = context(&settings);
        let mut job = Job::new(inputs("review"), settings).unwrap();
        let id = job.id;
        job.start().unwrap();
        for stage in [
            PipelineStage::Prepare,
            PipelineStage::Analyze,
            PipelineStage::Align,
            PipelineStage::ReviewAudio,
        ] {
            complete_checkpoint(&mut job, stage, &context);
        }
        job.require_review().unwrap();
        let mut queue = JobQueue::default();
        queue.enqueue(job);

        write_queue_recovery(&path, &queue).unwrap();
        let recovered = read_queue_recovery(&path).unwrap();
        let restored = recovered.queue.job(id).unwrap();
        assert_eq!(restored.status, JobStatus::Waiting);
        assert_eq!(
            restored.resume_plan(&context).unwrap().reusable(),
            &[
                PipelineStage::Prepare,
                PipelineStage::Analyze,
                PipelineStage::Align,
            ]
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn terminal_jobs_remove_recovery_file() {
        let path = recovery_path();
        let mut queue = JobQueue::default();
        let id = queue.enqueue(Job::new(inputs("done"), JobSettings::default()).unwrap());
        queue.start_next().unwrap();
        write_queue_recovery(&path, &queue).unwrap();
        assert!(path.exists());
        queue.finish(id, crate::JobOutcome::Completed, 1).unwrap();
        assert_eq!(write_queue_recovery(&path, &queue).unwrap(), 0);
        assert!(!path.exists());
    }

    #[test]
    fn unsupported_recovery_version_is_rejected() {
        let path = recovery_path();
        fs::write(&path, br#"{"version":99,"jobs":[]}"#).unwrap();
        assert!(read_queue_recovery(&path)
            .unwrap_err()
            .contains("unsupported"));
        let _ = fs::remove_file(path);
    }
}
