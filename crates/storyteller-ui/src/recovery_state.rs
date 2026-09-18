use crate::app_paths::recovery_app_root;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use storyteller_core::{read_queue_recovery, write_queue_recovery, JobQueue, QueueRecovery};

pub(crate) fn load_queue() -> Result<QueueRecovery, String> {
    read_queue_recovery(&recovery_path())
}

pub(crate) fn save_queue(queue: &JobQueue) -> Result<usize, String> {
    write_queue_recovery(&recovery_path(), queue)
}

pub(crate) fn quarantine_queue() -> Result<Option<PathBuf>, String> {
    quarantine_recovery_candidates(&recovery_path())
}

fn recovery_path() -> PathBuf {
    recovery_app_root().join("queue-recovery.json")
}

fn backup_recovery_path(path: &Path) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("Queue recovery filename is not valid UTF-8.")?;
    Ok(path.with_file_name(format!("{file_name}.bak")))
}

fn quarantine_recovery_candidates(primary: &Path) -> Result<Option<PathBuf>, String> {
    if primary.exists() {
        return quarantine_recovery_path(primary);
    }
    let backup = backup_recovery_path(primary)?;
    quarantine_recovery_path(&backup)
}

fn quarantine_recovery_path(path: &Path) -> Result<Option<PathBuf>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("Queue recovery filename is not valid UTF-8.")?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    let preserved = path.with_file_name(format!("{file_name}.invalid-{stamp}"));
    if preserved.exists() {
        return Err(format!(
            "Could not preserve unreadable recovery state because {} already exists.",
            preserved.display()
        ));
    }
    fs::rename(path, &preserved).map_err(|error| {
        format!(
            "Could not preserve unreadable recovery state {} as {}: {error}",
            path.display(),
            preserved.display()
        )
    })?;
    Ok(Some(preserved))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_recovery_path(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "storyteller-recovery-{label}-{}-{stamp}.json",
            std::process::id()
        ))
    }

    #[test]
    fn recovery_filename_is_stable() {
        assert_eq!(
            recovery_path().file_name().and_then(|value| value.to_str()),
            Some("queue-recovery.json")
        );
    }

    #[test]
    fn unreadable_snapshot_is_quarantined_without_deletion() {
        let path = test_recovery_path("quarantine");
        fs::write(&path, b"{not-valid-json").unwrap();

        let preserved = quarantine_recovery_candidates(&path).unwrap().unwrap();
        assert!(!path.exists());
        assert_eq!(fs::read(&preserved).unwrap(), b"{not-valid-json");

        let _ = fs::remove_file(preserved);
    }

    #[test]
    fn unreadable_backup_is_quarantined_when_primary_is_missing() {
        let path = test_recovery_path("backup-quarantine");
        let backup = backup_recovery_path(&path).unwrap();
        fs::write(&backup, b"{broken-backup").unwrap();

        let preserved = quarantine_recovery_candidates(&path).unwrap().unwrap();
        assert!(!path.exists());
        assert!(!backup.exists());
        assert_eq!(fs::read(&preserved).unwrap(), b"{broken-backup");
        assert!(preserved
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.contains(".bak.invalid-")));

        let _ = fs::remove_file(preserved);
    }
}
