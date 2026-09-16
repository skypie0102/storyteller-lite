use crate::app_paths::recovery_app_root;
use std::path::PathBuf;
use storyteller_core::{read_queue_recovery, write_queue_recovery, JobQueue, QueueRecovery};

pub(crate) fn load_queue() -> Result<QueueRecovery, String> {
    read_queue_recovery(&recovery_path())
}

pub(crate) fn save_queue(queue: &JobQueue) -> Result<usize, String> {
    write_queue_recovery(&recovery_path(), queue)
}

fn recovery_path() -> PathBuf {
    recovery_app_root().join("queue-recovery.json")
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
