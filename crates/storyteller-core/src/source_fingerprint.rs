use crate::{CancellationToken, Job, ResumeContext};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};

const HASH_BUFFER_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFingerprints {
    epub_source: String,
    audiobook_source: String,
}

impl SourceFingerprints {
    pub fn epub_source(&self) -> &str {
        &self.epub_source
    }

    pub fn audiobook_source(&self) -> &str {
        &self.audiobook_source
    }

    pub fn apply_to(&self, context: &mut ResumeContext) {
        context.epub_source.clone_from(&self.epub_source);
        context.audiobook_source.clone_from(&self.audiobook_source);
    }
}

pub fn fingerprint_job_sources(
    job: &Job,
    cancellation: &CancellationToken,
) -> Result<SourceFingerprints, String> {
    Ok(SourceFingerprints {
        epub_source: fingerprint_source_file(&job.inputs.epub_path, cancellation, "Source EPUB")?,
        audiobook_source: fingerprint_source_file(
            &job.inputs.audiobook_path,
            cancellation,
            "Audiobook",
        )?,
    })
}

pub fn fingerprint_source_file(
    path: &Path,
    cancellation: &CancellationToken,
    label: &str,
) -> Result<String, String> {
    if cancellation.is_requested() {
        return Err(format!("{label} fingerprinting was cancelled."));
    }
    let mut file = fs::File::open(path)
        .map_err(|error| format!("Could not open {label} at {}: {error}", path.display()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("Could not inspect {label} at {}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("{label} is not a regular file: {}", path.display()));
    }
    if metadata.len() == 0 {
        return Err(format!("{label} is empty: {}", path.display()));
    }

    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; HASH_BUFFER_BYTES];
    loop {
        if cancellation.is_requested() {
            return Err(format!("{label} fingerprinting was cancelled."));
        }
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("Could not read {label} at {}: {error}", path.display()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn temp_file(bytes: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("storyteller-hash-{}", Uuid::new_v4()));
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn sha256_matches_known_vector() {
        let path = temp_file(b"abc");
        let result = fingerprint_source_file(&path, &CancellationToken::default(), "test").unwrap();
        assert_eq!(
            result,
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn equal_content_has_equal_fingerprint_across_paths() {
        let first = temp_file(b"same");
        let second = temp_file(b"same");
        let token = CancellationToken::default();
        assert_eq!(
            fingerprint_source_file(&first, &token, "first").unwrap(),
            fingerprint_source_file(&second, &token, "second").unwrap()
        );
        let _ = fs::remove_file(first);
        let _ = fs::remove_file(second);
    }

    #[test]
    fn cancellation_is_checked_before_touching_source() {
        let token = CancellationToken::default();
        token.request();
        let missing = Path::new("definitely-missing-storyteller-source");
        let error = fingerprint_source_file(missing, &token, "source").unwrap_err();
        assert!(error.contains("cancelled"));
    }
}
