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
    quarantine_recovery_path(&recovery_path())
}

fn recovery_path() -> PathBuf {
    recovery_app_root().join("queue-recovery.json")
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

    #[test]
    fn recovery_filename_is_stable() {
        assert_eq!(
            recovery_path().file_name().and_then(|value| value.to_str()),
            Some("queue-recovery.json")
        );
    }

    #[test]
    fn unreadable_snapshot_is_quarantined_without_deletion() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "storyteller-recovery-quarantine-{}-{stamp}.json",
            std::process::id()
        ));
        fs::write(&path, b"{not-valid-json").unwrap();

        let preserved = quarantine_recovery_path(&path).unwrap().unwrap();
        assert!(!path.exists());
        assert_eq!(fs::read(&preserved).unwrap(), b"{not-valid-json");

        let _ = fs::remove_file(preserved);
    }
}
