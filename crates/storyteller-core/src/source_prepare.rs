use crate::{CancellationToken, Job, JobWorkspace, PipelineStage};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const COPY_BUFFER_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedSources {
    epub: PathBuf,
    audiobook: PathBuf,
}

impl PreparedSources {
    pub fn epub(&self) -> &Path {
        &self.epub
    }

    pub fn audiobook(&self) -> &Path {
        &self.audiobook
    }

    pub fn relative_artifacts(&self) -> Vec<PathBuf> {
        vec![
            PathBuf::from(
                self.epub
                    .file_name()
                    .expect("prepared EPUB has a file name"),
            ),
            PathBuf::from(
                self.audiobook
                    .file_name()
                    .expect("prepared audiobook has a file name"),
            ),
        ]
    }
}

pub fn prepare_job_sources(
    job: &Job,
    workspace: &JobWorkspace,
    cancellation: &CancellationToken,
) -> Result<PreparedSources, String> {
    validate_source_file(&job.inputs.epub_path, "Source EPUB")?;
    validate_source_file(&job.inputs.audiobook_path, "Audiobook")?;
    if cancellation.is_requested() {
        return Err("Source preparation was cancelled.".into());
    }

    let stage_dir = workspace.stage_dir(PipelineStage::Prepare);
    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir)
            .map_err(|error| format!("Could not reset Prepare workspace: {error}"))?;
    }
    fs::create_dir_all(&stage_dir)
        .map_err(|error| format!("Could not create Prepare workspace: {error}"))?;

    let result = (|| {
        let epub = stage_dir.join("source.epub");
        copy_cancellable(&job.inputs.epub_path, &epub, cancellation)?;

        let audiobook = prepared_audiobook_path(job, &stage_dir);
        if cancellation.is_requested() {
            return Err("Source preparation was cancelled.".into());
        }
        if fs::hard_link(&job.inputs.audiobook_path, &audiobook).is_err() {
            copy_cancellable(&job.inputs.audiobook_path, &audiobook, cancellation)?;
        }
        validate_source_file(&epub, "Prepared EPUB")?;
        validate_source_file(&audiobook, "Prepared audiobook")?;
        Ok(PreparedSources { epub, audiobook })
    })();

    if result.is_err() && stage_dir.exists() {
        let _ = fs::remove_dir_all(&stage_dir);
    }
    result
}

pub fn prepared_job_sources(
    job: &Job,
    workspace: &JobWorkspace,
) -> Result<PreparedSources, String> {
    let stage_dir = workspace.stage_dir(PipelineStage::Prepare);
    let epub = stage_dir.join("source.epub");
    let audiobook = prepared_audiobook_path(job, &stage_dir);
    validate_source_file(&epub, "Prepared EPUB")?;
    validate_source_file(&audiobook, "Prepared audiobook")?;
    Ok(PreparedSources { epub, audiobook })
}

fn prepared_audiobook_path(job: &Job, stage_dir: &Path) -> PathBuf {
    let extension = job
        .inputs
        .audiobook_path
        .extension()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("audio");
    stage_dir.join(format!("audiobook.{extension}"))
}

fn validate_source_file(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("{label} is unavailable at {}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("{label} is not a regular file: {}", path.display()));
    }
    if metadata.len() == 0 {
        return Err(format!("{label} is empty: {}", path.display()));
    }
    Ok(())
}

fn copy_cancellable(
    source: &Path,
    destination: &Path,
    cancellation: &CancellationToken,
) -> Result<(), String> {
    if cancellation.is_requested() {
        return Err("Source preparation was cancelled.".into());
    }
    let mut input = fs::File::open(source)
        .map_err(|error| format!("Could not open {}: {error}", source.display()))?;
    let mut output = fs::File::create(destination)
        .map_err(|error| format!("Could not create {}: {error}", destination.display()))?;
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    loop {
        if cancellation.is_requested() {
            return Err("Source preparation was cancelled.".into());
        }
        let count = input
            .read(&mut buffer)
            .map_err(|error| format!("Could not read {}: {error}", source.display()))?;
        if count == 0 {
            break;
        }
        output
            .write_all(&buffer[..count])
            .map_err(|error| format!("Could not write {}: {error}", destination.display()))?;
    }
    output
        .sync_all()
        .map_err(|error| format!("Could not finish {}: {error}", destination.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{JobInputs, JobSettings};
    use uuid::Uuid;

    fn temp_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("storyteller-{label}-{}", Uuid::new_v4()))
    }

    #[test]
    fn prepare_preserves_sources_and_stages_deterministic_names() {
        let root = temp_dir("prepare");
        fs::create_dir_all(&root).unwrap();
        let epub = root.join("book.epub");
        let audio = root.join("book.m4b");
        fs::write(&epub, b"epub-data").unwrap();
        fs::write(&audio, b"audio-data").unwrap();
        let job = Job::new(
            JobInputs {
                title: "Book".into(),
                epub_path: epub.clone(),
                audiobook_path: audio.clone(),
                output_path: root.join("Book (readaloud).epub"),
            },
            JobSettings::default(),
        )
        .unwrap();
        let workspace = JobWorkspace::new(root.join("work"));
        let prepared =
            prepare_job_sources(&job, &workspace, &CancellationToken::default()).unwrap();
        assert_eq!(fs::read(&epub).unwrap(), b"epub-data");
        assert_eq!(prepared.epub().file_name().unwrap(), "source.epub");
        assert_eq!(prepared.audiobook().file_name().unwrap(), "audiobook.m4b");
        assert_eq!(fs::read(prepared.audiobook()).unwrap(), b"audio-data");
        assert_eq!(prepared_job_sources(&job, &workspace).unwrap(), prepared);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn empty_input_is_rejected() {
        let root = temp_dir("empty");
        fs::create_dir_all(&root).unwrap();
        let epub = root.join("book.epub");
        let audio = root.join("book.m4b");
        fs::write(&epub, b"").unwrap();
        fs::write(&audio, b"audio").unwrap();
        let job = Job::new(
            JobInputs {
                title: "Book".into(),
                epub_path: epub,
                audiobook_path: audio,
                output_path: root.join("out.epub"),
            },
            JobSettings::default(),
        )
        .unwrap();
        let error = prepare_job_sources(
            &job,
            &JobWorkspace::new(root.join("work")),
            &CancellationToken::default(),
        )
        .unwrap_err();
        assert!(error.contains("empty"));
        let _ = fs::remove_dir_all(root);
    }
}
