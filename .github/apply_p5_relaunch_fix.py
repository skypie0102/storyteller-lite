from pathlib import Path

path = Path("crates/storyteller-core/src/job_recovery.rs")
text = path.read_text(encoding="utf-8")

old_import = '''use crate::{
    AudioBitrate, AudioCodec, AudioEncoding, AudioReviewPolicy, Job, JobInputs, JobQueue, JobSettings,
    JobStatus, PipelineProgress, PipelineStage, ResumeContext, StageCheckpoint,
};
'''
new_import = '''use crate::{
    job::StageCheckpoint, AudioBitrate, AudioCodec, AudioEncoding, AudioReviewPolicy, Job, JobInputs,
    JobQueue, JobSettings, JobStatus, PipelineProgress, PipelineStage,
};
'''
if text.count(old_import) != 1:
    raise SystemExit("expected exactly one recovery import block")
text = text.replace(old_import, new_import)

old_jobs = '''    if jobs.is_empty() {
        if path.exists() {
'''
new_jobs = '''    if jobs.is_empty() {
        if path.exists() {
'''
if text.count(old_jobs) != 1:
    raise SystemExit("expected recovery empty-jobs block")
# Count before moving jobs into the serialized file.
text = text.replace(
    '''    let parent = path
''',
    '''    let recovered_jobs = jobs.len();
    let parent = path
''',
    1,
)

old_verify = '''    let decoded: QueueRecoveryFile = serde_json::from_slice(
        &fs::read(path)
            .map_err(|error| format!("Could not verify queue recovery file: {error}"))?,
    )
    .map_err(|error| format!("Written queue recovery file is invalid: {error}"))?;
    Ok(decoded.jobs.len())
'''
new_verify = '''    Ok(recovered_jobs)
'''
if text.count(old_verify) != 1:
    raise SystemExit("expected recovery verification reread block")
text = text.replace(old_verify, new_verify)

old_test_import = '''    use super::*;
    use std::fs;
    use uuid::Uuid;
'''
new_test_import = '''    use super::*;
    use crate::ResumeContext;
    use std::fs;
    use uuid::Uuid;
'''
if text.count(old_test_import) != 1:
    raise SystemExit("expected recovery test import block")
text = text.replace(old_test_import, new_test_import)

path.write_text(text, encoding="utf-8")
