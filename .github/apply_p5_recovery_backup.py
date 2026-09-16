from pathlib import Path


def replace_exact(path: str, old: str, new: str, expected: int = 1) -> None:
    file_path = Path(path)
    text = file_path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != expected:
        raise SystemExit(f"{path}: expected {expected} occurrences, found {count}: {old!r}")
    file_path.write_text(text.replace(old, new), encoding="utf-8")


path = "crates/storyteller-core/src/job_recovery.rs"

replace_exact(
    path,
    '''use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};
''',
    '''use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
''',
)

old = '''pub fn write_queue_recovery(path: &Path, queue: &JobQueue) -> Result<usize, String> {
    let jobs = queue
        .jobs()
        .iter()
        .filter_map(JobRecoveryRecord::from_job)
        .collect::<Vec<_>>();
    if jobs.is_empty() {
        if path.exists() {
            fs::remove_file(path).map_err(|error| {
                format!(
                    "Could not remove completed queue recovery file {}: {error}",
                    path.display()
                )
            })?;
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
    fs::write(&temporary, encoded).map_err(|error| {
        format!(
            "Could not write queue recovery temporary file {}: {error}",
            temporary.display()
        )
    })?;
    if path.exists() {
        fs::remove_file(path).map_err(|error| {
            format!(
                "Could not replace queue recovery file {}: {error}",
                path.display()
            )
        })?;
    }
    fs::rename(&temporary, path).map_err(|error| {
        format!(
            "Could not publish queue recovery file {}: {error}",
            path.display()
        )
    })?;
    Ok(recovered_jobs)
}

pub fn read_queue_recovery(path: &Path) -> Result<QueueRecovery, String> {
    if !path.exists() {
        return Ok(QueueRecovery {
            queue: JobQueue::default(),
            recovered_jobs: 0,
        });
    }
    let bytes = fs::read(path).map_err(|error| {
        format!(
            "Could not read queue recovery file {}: {error}",
            path.display()
        )
    })?;
'''
new = '''pub fn write_queue_recovery(path: &Path, queue: &JobQueue) -> Result<usize, String> {
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
'''
replace_exact(path, old, new)

replace_exact(
    path,
    '''fn temporary_recovery_path(path: &Path) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("Queue recovery filename is not valid UTF-8.")?;
    Ok(path.with_file_name(format!(".{file_name}.tmp")))
}
''',
    '''fn temporary_recovery_path(path: &Path) -> Result<PathBuf, String> {
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
''',
)

marker = '''    #[test]
    fn running_job_restores_waiting_and_queue_is_paused() {
'''
addition = '''    #[test]
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

'''
replace_exact(path, marker, addition + marker)
