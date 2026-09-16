use std::{env, path::PathBuf};
use storyteller_core::{read_queue_recovery, write_queue_recovery, JobQueue, QueueRecovery};

pub(crate) fn load_queue() -> Result<QueueRecovery, String> {
    read_queue_recovery(&recovery_path())
}

pub(crate) fn save_queue(queue: &JobQueue) -> Result<usize, String> {
    write_queue_recovery(&recovery_path(), queue)
}

fn recovery_path() -> PathBuf {
    app_data_root().join("queue-recovery.json")
}

fn app_data_root() -> PathBuf {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join("Storyteller OneClick Lite")
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
}
