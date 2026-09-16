from pathlib import Path


def replace_exact(path: str, old: str, new: str, expected: int = 1) -> None:
    file_path = Path(path)
    text = file_path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != expected:
        raise SystemExit(f"{path}: expected {expected} occurrences, found {count}: {old!r}")
    file_path.write_text(text.replace(old, new), encoding="utf-8")


replace_exact(
    "crates/storyteller-ui/src/recovery_state.rs",
    '''use crate::app_paths::recovery_app_root;
use std::path::PathBuf;
use storyteller_core::{read_queue_recovery, write_queue_recovery, JobQueue, QueueRecovery};
''',
    '''use crate::app_paths::recovery_app_root;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use storyteller_core::{read_queue_recovery, write_queue_recovery, JobQueue, QueueRecovery};
''',
)

replace_exact(
    "crates/storyteller-ui/src/recovery_state.rs",
    '''pub(crate) fn save_queue(queue: &JobQueue) -> Result<usize, String> {
    write_queue_recovery(&recovery_path(), queue)
}

fn recovery_path() -> PathBuf {
    recovery_app_root().join("queue-recovery.json")
}
''',
    '''pub(crate) fn save_queue(queue: &JobQueue) -> Result<usize, String> {
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
''',
)

replace_exact(
    "crates/storyteller-ui/src/recovery_state.rs",
    '''    #[test]
    fn recovery_filename_is_stable() {
        assert_eq!(
            recovery_path().file_name().and_then(|value| value.to_str()),
            Some("queue-recovery.json")
        );
    }
''',
    '''    #[test]
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
''',
)

replace_exact(
    "crates/storyteller-ui/src/worker_bridge.rs",
    '''    runtime_settings_was_open: bool,
    recovery_loaded: bool,
    last_recovery_save: Option<Instant>,
''',
    '''    runtime_settings_was_open: bool,
    recovery_loaded: bool,
    recovery_persistence_blocked: bool,
    last_recovery_save: Option<Instant>,
''',
)

replace_exact(
    "crates/storyteller-ui/src/worker_bridge.rs",
    '''            Err(error) => {
                eprintln!("Queue recovery could not be loaded: {error}");
                refresh_if_open(
                    ui_weak,
                    queue,
                    queue_rows,
                    stage_rows,
                    detail_stage_rows,
                    Some(format!("Recovery state could not be loaded: {error}")),
                );
            }
''',
    '''            Err(error) => {
                eprintln!("Queue recovery could not be loaded: {error}");
                let status = match recovery_state::quarantine_queue() {
                    Ok(Some(preserved)) => {
                        self.last_recovery_save = Some(Instant::now());
                        format!(
                            "Recovery state could not be loaded: {error} The unreadable snapshot was preserved at {}.",
                            preserved.display()
                        )
                    }
                    Ok(None) => {
                        self.last_recovery_save = Some(Instant::now());
                        format!("Recovery state could not be loaded: {error}")
                    }
                    Err(preserve_error) => {
                        self.recovery_persistence_blocked = true;
                        eprintln!(
                            "Unreadable queue recovery could not be preserved; automatic recovery saving is disabled for this session: {preserve_error}"
                        );
                        format!(
                            "Recovery state could not be loaded: {error} The unreadable snapshot could not be preserved, so automatic recovery saving is disabled for this session: {preserve_error}"
                        )
                    }
                };
                refresh_if_open(
                    ui_weak,
                    queue,
                    queue_rows,
                    stage_rows,
                    detail_stage_rows,
                    Some(status),
                );
            }
''',
)

replace_exact(
    "crates/storyteller-ui/src/worker_bridge.rs",
    '''    fn persist_recovery_if_due(&mut self, queue: &Rc<RefCell<JobQueue>>) {
        if !self.recovery_loaded {
            return;
        }
''',
    '''    fn persist_recovery_if_due(&mut self, queue: &Rc<RefCell<JobQueue>>) {
        if !self.recovery_loaded || self.recovery_persistence_blocked {
            return;
        }
''',
)
