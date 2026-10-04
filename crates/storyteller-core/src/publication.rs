use crate::{
    fingerprint_source_file, validate_readaloud_epub, workspace::write_atomic, CancellationToken,
    EpubValidationSummary,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

/// Only independent structural validation can create a publishable candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedEpub {
    candidate: PathBuf,
    sha256: String,
    summary: EpubValidationSummary,
}

impl ValidatedEpub {
    pub fn validate(path: &Path, cancellation: &CancellationToken) -> Result<Self, String> {
        let before = fingerprint_source_file(path, cancellation, "EPUB candidate")?;
        let summary = validate_readaloud_epub(path, cancellation)?;
        let after = fingerprint_source_file(path, cancellation, "EPUB candidate")?;
        if before != after {
            return Err("EPUB candidate changed during validation.".into());
        }
        Ok(Self {
            candidate: path.to_owned(),
            sha256: after,
            summary,
        })
    }

    pub fn summary(&self) -> EpubValidationSummary {
        self.summary
    }
    pub fn fingerprint(&self) -> &str {
        &self.sha256
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PublicationIntent {
    version: u32,
    destination: PathBuf,
    sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublicationBoundary {
    Staged,
    IntentRecorded,
    Committed,
}

/// Publish complete, hash-matching bytes with an atomic no-overwrite operation.
/// The job-owned intent makes a crash after the commit recoverable without accepting
/// an unrelated existing output. Staging is on the output filesystem.
pub fn publish_validated_epub(
    candidate: &ValidatedEpub,
    destination: &Path,
    intent_path: &Path,
    cancellation: &CancellationToken,
) -> Result<(), String> {
    publish(
        candidate,
        destination,
        intent_path,
        cancellation,
        |_| Ok(()),
    )
}

fn publish(
    candidate: &ValidatedEpub,
    destination: &Path,
    intent_path: &Path,
    cancellation: &CancellationToken,
    mut boundary: impl FnMut(PublicationBoundary) -> Result<(), String>,
) -> Result<(), String> {
    check_cancelled(cancellation)?;
    let destination = absolute_path(destination)?;
    let intent_path = absolute_path(intent_path)?;
    let source = fs::canonicalize(&candidate.candidate)
        .map_err(|error| format!("EPUB candidate is unavailable: {error}"))?;
    if source == destination || intent_path == destination || intent_path == source {
        return Err(
            "EPUB candidate, output and publication intent must use separate paths.".into(),
        );
    }

    let prior = read_intent(&intent_path)?;
    if fs::symlink_metadata(&destination).is_ok() {
        return verify_existing(candidate, &destination, prior.as_ref(), cancellation);
    }
    let parent = destination
        .parent()
        .ok_or("Output EPUB path has no parent directory.")?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create output directory: {error}"))?;
    let temporary = parent.join(format!(".storyteller-publication-{}.tmp", Uuid::new_v4()));
    // create_new prevents truncating another attempt's staging file.
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("Could not create EPUB publication temporary file: {error}"))?;
    let result = (|| {
        let mut input = fs::File::open(&source)
            .map_err(|error| format!("Could not open EPUB candidate: {error}"))?;
        let mut hash = Sha256::new();
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            check_cancelled(cancellation)?;
            let count = input
                .read(&mut buffer)
                .map_err(|error| format!("Could not read EPUB candidate: {error}"))?;
            if count == 0 {
                break;
            }
            output
                .write_all(&buffer[..count])
                .map_err(|error| format!("Could not stage output EPUB: {error}"))?;
            hash.update(&buffer[..count]);
        }
        if format!("sha256:{:x}", hash.finalize()) != candidate.sha256 {
            return Err("EPUB candidate changed after validation; publication was stopped.".into());
        }
        output
            .sync_all()
            .map_err(|error| format!("Could not flush staged EPUB: {error}"))?;
        drop(output);
        boundary(PublicationBoundary::Staged)?;
        check_cancelled(cancellation)?;
        let intent = PublicationIntent {
            version: 1,
            destination: destination.clone(),
            sha256: candidate.sha256.clone(),
        };
        let encoded = serde_json::to_vec_pretty(&intent)
            .map_err(|error| format!("Could not serialize publication intent: {error}"))?;
        write_atomic(&intent_path, &encoded)?;
        boundary(PublicationBoundary::IntentRecorded)?;
        check_cancelled(cancellation)?;
        match commit_without_overwrite(&temporary, &destination) {
            Ok(()) => {
                // This is the commit point. Cancellation after it must not report an
                // unpublished book when the validated output is already visible.
                boundary(PublicationBoundary::Committed)?;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                verify_existing(candidate, &destination, Some(&intent), cancellation)
            }
            Err(error) => Err(format!(
                "Could not atomically publish EPUB without overwriting {}: {error}",
                destination.display()
            )),
        }
    })();
    // Only this attempt's uniquely owned name is removed. A committed output remains.
    let _ = fs::remove_file(&temporary);
    result
}

#[cfg(not(windows))]
fn commit_without_overwrite(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    fs::hard_link(temporary, destination)
}

#[cfg(windows)]
fn commit_without_overwrite(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[link_name = "MoveFileExW"]
        fn move_file_ex_w(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    fn wide(path: &Path) -> std::io::Result<Vec<u16>> {
        let mut encoded: Vec<u16> = path.as_os_str().encode_wide().collect();
        if encoded.contains(&0) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Publication path contains a null character.",
            ));
        }
        encoded.push(0);
        Ok(encoded)
    }
    let existing = wide(temporary)?;
    let new = wide(destination)?;
    // MOVEFILE_WRITE_THROUGH only: never set REPLACE_EXISTING or COPY_ALLOWED.
    // Both terminated UTF-16 buffers remain alive for the call. The staging file
    // shares the destination directory, so this cannot turn into a cross-volume copy.
    // https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-movefileexw
    let succeeded = unsafe { move_file_ex_w(existing.as_ptr(), new.as_ptr(), 0x8) };
    if succeeded == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn verify_existing(
    candidate: &ValidatedEpub,
    destination: &Path,
    prior: Option<&PublicationIntent>,
    cancellation: &CancellationToken,
) -> Result<(), String> {
    let authorized = prior.is_some_and(|intent| {
        intent.destination == destination && intent.sha256 == candidate.sha256
    });
    let regular = fs::symlink_metadata(destination).is_ok_and(|metadata| metadata.is_file());
    if !authorized
        || !regular
        || fingerprint_source_file(destination, cancellation, "Published EPUB")? != candidate.sha256
    {
        return Err(format!(
            "Output EPUB already exists and will not be overwritten: {}",
            destination.display()
        ));
    }
    Ok(())
}

fn read_intent(path: &Path) -> Result<Option<PublicationIntent>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Could not inspect publication intent: {error}")),
    };
    if !metadata.is_file() || metadata.len() > 16 * 1024 {
        return Err(
            "Publication intent is not a bounded regular file; preserve it for recovery.".into(),
        );
    }
    let data =
        fs::read(path).map_err(|error| format!("Could not read publication intent: {error}"))?;
    let intent: PublicationIntent = serde_json::from_slice(&data).map_err(|error| {
        format!("Publication intent is invalid; preserve it for recovery: {error}")
    })?;
    if intent.version != 1 {
        return Err("Publication intent uses an unsupported version.".into());
    }
    Ok(Some(intent))
}

fn absolute_path(path: &Path) -> Result<PathBuf, String> {
    let absolute = std::path::absolute(path)
        .map_err(|error| format!("Could not resolve publication path: {error}"))?;
    let parent = absolute.parent().ok_or("Publication path has no parent.")?;
    // Existing ancestors are resolved so aliases cannot defeat path separation or recovery.
    let mut ancestor = parent;
    let mut missing = Vec::new();
    while !ancestor.exists() {
        missing.push(
            ancestor
                .file_name()
                .ok_or("Publication path has no existing ancestor.")?
                .to_owned(),
        );
        ancestor = ancestor
            .parent()
            .ok_or("Publication path has no existing ancestor.")?;
    }
    let mut resolved = fs::canonicalize(ancestor)
        .map_err(|error| format!("Could not resolve publication directory: {error}"))?;
    for component in missing.into_iter().rev() {
        resolved.push(component);
    }
    resolved.push(
        absolute
            .file_name()
            .ok_or("Publication path has no filename.")?,
    );
    Ok(resolved)
}

fn check_cancelled(cancellation: &CancellationToken) -> Result<(), String> {
    if cancellation.is_requested() {
        Err("EPUB publication was cancelled.".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
        candidate: ValidatedEpub,
        destination: PathBuf,
        intent: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("storyteller-publication-{}", Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            let path = root.join("candidate.epub");
            fs::write(&path, b"validated candidate bytes").unwrap();
            // Isolate filesystem failure boundaries. The EPUB integration test creates
            // its token through the real builder and independent structural validator.
            let sha256 =
                fingerprint_source_file(&path, &CancellationToken::default(), "fixture").unwrap();
            let candidate = ValidatedEpub {
                candidate: path,
                sha256,
                summary: EpubValidationSummary {
                    overlay_count: 1,
                    synchronized_segments: 1,
                    media_duration_ms: 1000,
                },
            };
            Self {
                destination: root.join("books").join("Novel (readaloud).epub"),
                intent: root.join("job").join("publication.json"),
                candidate,
                root,
            }
        }
        fn publish(&self) -> Result<(), String> {
            publish_validated_epub(
                &self.candidate,
                &self.destination,
                &self.intent,
                &CancellationToken::default(),
            )
        }
        fn temporary_files(&self) -> Vec<PathBuf> {
            let parent = self.destination.parent().unwrap();
            if !parent.exists() {
                return Vec::new();
            }
            fs::read_dir(parent)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| {
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(".storyteller-publication-")
                })
                .collect()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn invalid_epub_cannot_create_a_publication_token() {
        let fixture = Fixture::new();
        assert!(ValidatedEpub::validate(
            &fixture.candidate.candidate,
            &CancellationToken::default()
        )
        .is_err());
        assert!(!fixture.destination.exists());
    }

    #[test]
    fn failure_before_and_after_intent_never_exposes_partial_output() {
        for stop in [
            PublicationBoundary::Staged,
            PublicationBoundary::IntentRecorded,
        ] {
            let fixture = Fixture::new();
            let error = publish(
                &fixture.candidate,
                &fixture.destination,
                &fixture.intent,
                &CancellationToken::default(),
                |at| {
                    if at == stop {
                        Err("injected interruption".into())
                    } else {
                        Ok(())
                    }
                },
            )
            .unwrap_err();
            assert_eq!(error, "injected interruption");
            assert!(!fixture.destination.exists());
            assert!(fixture.temporary_files().is_empty());
            fixture.publish().unwrap();
            assert_eq!(
                fs::read(&fixture.destination).unwrap(),
                b"validated candidate bytes"
            );
        }
    }

    #[test]
    fn interruption_after_commit_recovers_exact_output_without_overwrite() {
        let fixture = Fixture::new();
        assert!(publish(
            &fixture.candidate,
            &fixture.destination,
            &fixture.intent,
            &CancellationToken::default(),
            |at| {
                if at == PublicationBoundary::Committed {
                    Err("injected interruption".into())
                } else {
                    Ok(())
                }
            }
        )
        .is_err());
        assert_eq!(
            fs::read(&fixture.destination).unwrap(),
            b"validated candidate bytes"
        );
        let modified = fs::metadata(&fixture.destination)
            .unwrap()
            .modified()
            .unwrap();
        fixture.publish().unwrap();
        assert_eq!(
            fs::metadata(&fixture.destination)
                .unwrap()
                .modified()
                .unwrap(),
            modified
        );
    }

    #[test]
    fn competing_output_created_at_commit_is_never_overwritten() {
        let fixture = Fixture::new();
        let error = publish(
            &fixture.candidate,
            &fixture.destination,
            &fixture.intent,
            &CancellationToken::default(),
            |at| {
                if at == PublicationBoundary::IntentRecorded {
                    fs::write(&fixture.destination, b"someone else's book").unwrap();
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert!(error.contains("will not be overwritten"));
        assert_eq!(
            fs::read(&fixture.destination).unwrap(),
            b"someone else's book"
        );
        assert!(fixture.temporary_files().is_empty());
    }

    #[test]
    fn identical_existing_output_without_job_intent_is_not_accepted() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.destination.parent().unwrap()).unwrap();
        fs::copy(&fixture.candidate.candidate, &fixture.destination).unwrap();
        assert!(fixture
            .publish()
            .unwrap_err()
            .contains("will not be overwritten"));
        assert!(!fixture.intent.exists());
    }

    #[test]
    fn changed_candidate_or_changed_committed_output_is_rejected() {
        let fixture = Fixture::new();
        fs::write(&fixture.candidate.candidate, b"changed candidate bytes").unwrap();
        assert!(fixture
            .publish()
            .unwrap_err()
            .contains("changed after validation"));
        assert!(!fixture.destination.exists());
        assert!(!fixture.intent.exists());
        assert!(fixture.temporary_files().is_empty());
        let fixture = Fixture::new();
        fixture.publish().unwrap();
        fs::write(&fixture.destination, b"changed published bytes").unwrap();
        assert!(fixture.publish().is_err());
        assert_eq!(
            fs::read(&fixture.destination).unwrap(),
            b"changed published bytes"
        );
    }

    #[test]
    fn cancellation_at_each_precommit_boundary_leaves_no_output() {
        for stop in [
            PublicationBoundary::Staged,
            PublicationBoundary::IntentRecorded,
        ] {
            let fixture = Fixture::new();
            let token = CancellationToken::default();
            assert!(publish(
                &fixture.candidate,
                &fixture.destination,
                &fixture.intent,
                &token,
                |at| {
                    if at == stop {
                        token.request();
                    }
                    Ok(())
                }
            )
            .unwrap_err()
            .contains("cancelled"));
            assert!(!fixture.destination.exists());
            assert!(fixture.temporary_files().is_empty());
            fixture.publish().unwrap();
        }
    }

    #[test]
    fn cancellation_after_commit_reports_success() {
        let fixture = Fixture::new();
        let token = CancellationToken::default();
        publish(
            &fixture.candidate,
            &fixture.destination,
            &fixture.intent,
            &token,
            |at| {
                if at == PublicationBoundary::Committed {
                    token.request();
                }
                Ok(())
            },
        )
        .unwrap();
        assert!(token.is_requested());
        assert_eq!(
            fs::read(&fixture.destination).unwrap(),
            b"validated candidate bytes"
        );
    }

    #[test]
    fn corrupt_intent_is_preserved_and_blocks_publication() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.intent.parent().unwrap()).unwrap();
        fs::write(&fixture.intent, b"{truncated").unwrap();
        assert!(fixture
            .publish()
            .unwrap_err()
            .contains("preserve it for recovery"));
        assert_eq!(fs::read(&fixture.intent).unwrap(), b"{truncated");
        assert!(!fixture.destination.exists());
    }

    #[test]
    fn abandoned_staging_file_is_not_reused_or_deleted() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.destination.parent().unwrap()).unwrap();
        let abandoned = fixture
            .destination
            .parent()
            .unwrap()
            .join(".storyteller-publication-abandoned.tmp");
        fs::write(&abandoned, b"incomplete old attempt").unwrap();
        fixture.publish().unwrap();
        assert_eq!(fs::read(&abandoned).unwrap(), b"incomplete old attempt");
        assert_eq!(fixture.temporary_files(), vec![abandoned]);
    }

    #[test]
    fn path_alias_cannot_replace_candidate_or_intent() {
        let fixture = Fixture::new();
        let alias = fixture.root.join(".").join("candidate.epub");
        assert!(publish_validated_epub(
            &fixture.candidate,
            &alias,
            &fixture.intent,
            &CancellationToken::default()
        )
        .is_err());
        assert_eq!(
            fs::read(&fixture.candidate.candidate).unwrap(),
            b"validated candidate bytes"
        );
        assert!(publish_validated_epub(
            &fixture.candidate,
            &fixture.destination,
            &fixture.destination,
            &CancellationToken::default()
        )
        .is_err());
    }
}
