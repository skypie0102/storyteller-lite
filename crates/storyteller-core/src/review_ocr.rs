use crate::{review_image::AudioReviewImageCandidate, CancellationToken};
use std::{
    fs::File,
    io::Read,
    path::Path,
};
use zip::{result::ZipError, ZipArchive};

const MAX_OCR_IMAGE_BYTES: u64 = 25 * 1024 * 1024;
const READ_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewImageTextSource {
    Embedded,
    Ocr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewImageTextEvidence {
    pub source: ReviewImageTextSource,
    pub text: Vec<String>,
}

pub trait ReviewImageOcrEngine {
    fn recognize(
        &self,
        image: &[u8],
        media_type: &str,
        cancellation: &CancellationToken,
    ) -> Result<Vec<String>, String>;
}

pub fn review_image_text_evidence(
    epub_path: &Path,
    candidate: &AudioReviewImageCandidate,
    ocr: &dyn ReviewImageOcrEngine,
    cancellation: &CancellationToken,
) -> Result<ReviewImageTextEvidence, String> {
    if cancellation.is_requested() {
        return Err("EPUB image OCR was cancelled.".into());
    }

    let embedded = normalize_text(&candidate.embedded_text);
    if !embedded.is_empty() {
        return Ok(ReviewImageTextEvidence {
            source: ReviewImageTextSource::Embedded,
            text: embedded,
        });
    }

    let image = read_candidate_image(epub_path, candidate, cancellation)?;
    let text = match ocr.recognize(&image, &candidate.media_type, cancellation) {
        Ok(text) => text,
        Err(_) if cancellation.is_requested() => {
            return Err("EPUB image OCR was cancelled.".into());
        }
        Err(error) => {
            return Err(format!(
                "OCR failed for EPUB image {}: {error}",
                candidate.image_href
            ));
        }
    };
    if cancellation.is_requested() {
        return Err("EPUB image OCR was cancelled.".into());
    }

    Ok(ReviewImageTextEvidence {
        source: ReviewImageTextSource::Ocr,
        text: normalize_text(&text),
    })
}

fn read_candidate_image(
    epub_path: &Path,
    candidate: &AudioReviewImageCandidate,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, String> {
    if candidate.byte_size == 0 || candidate.byte_size > MAX_OCR_IMAGE_BYTES {
        return Err(format!(
            "EPUB image {} is outside the OCR size limit.",
            candidate.image_href
        ));
    }

    let file = File::open(epub_path)
        .map_err(|error| format!("Could not open EPUB {}: {error}", epub_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| format!("Could not read EPUB ZIP container: {error}"))?;
    let mut entry = match archive.by_name(&candidate.image_href) {
        Ok(entry) => entry,
        Err(ZipError::FileNotFound) => {
            return Err(format!(
                "EPUB image {} is no longer present in the archive.",
                candidate.image_href
            ));
        }
        Err(error) => {
            return Err(format!(
                "Could not read EPUB image {}: {error}",
                candidate.image_href
            ));
        }
    };
    if !entry.is_file() {
        return Err(format!(
            "EPUB image {} is not a regular file.",
            candidate.image_href
        ));
    }
    if entry.size() != candidate.byte_size {
        return Err(format!(
            "EPUB image {} changed after image candidates were discovered.",
            candidate.image_href
        ));
    }
    if entry.size() > MAX_OCR_IMAGE_BYTES {
        return Err(format!(
            "EPUB image {} exceeds the OCR size limit.",
            candidate.image_href
        ));
    }

    let mut image = Vec::with_capacity(entry.size() as usize);
    let mut buffer = vec![0u8; READ_BUFFER_BYTES];
    loop {
        if cancellation.is_requested() {
            return Err("EPUB image OCR was cancelled.".into());
        }
        let count = entry
            .read(&mut buffer)
            .map_err(|error| format!("Could not read EPUB image {}: {error}", candidate.image_href))?;
        if count == 0 {
            break;
        }
        if image.len().saturating_add(count) > MAX_OCR_IMAGE_BYTES as usize {
            return Err(format!(
                "EPUB image {} expanded beyond the OCR size limit.",
                candidate.image_href
            ));
        }
        image.extend_from_slice(&buffer[..count]);
    }
    Ok(image)
}

fn normalize_text(values: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for value in values {
        for line in value.lines() {
            let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
            if line.is_empty()
                || normalized
                    .iter()
                    .any(|existing: &String| existing.eq_ignore_ascii_case(&line))
            {
                continue;
            }
            normalized.push(line);
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        io::Write,
        path::PathBuf,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Mutex,
        },
    };
    use uuid::Uuid;
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    #[derive(Debug)]
    struct RecordingOcr {
        calls: AtomicUsize,
        image: Mutex<Vec<u8>>,
        result: Vec<String>,
    }

    impl RecordingOcr {
        fn new(result: Vec<String>) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                image: Mutex::new(Vec::new()),
                result,
            }
        }
    }

    impl ReviewImageOcrEngine for RecordingOcr {
        fn recognize(
            &self,
            image: &[u8],
            _media_type: &str,
            _cancellation: &CancellationToken,
        ) -> Result<Vec<String>, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.image.lock().unwrap() = image.to_vec();
            Ok(self.result.clone())
        }
    }

    fn candidate(byte_size: u64, embedded_text: Vec<String>) -> AudioReviewImageCandidate {
        AudioReviewImageCandidate {
            document_href: "OPS/Text/figure.xhtml".into(),
            image_href: "OPS/Images/diagram.png".into(),
            media_type: "image/png".into(),
            byte_size,
            document_spine_index: 1,
            image_ordinal: 0,
            embedded_text,
        }
    }

    fn temp_epub(image: &[u8]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("storyteller-review-ocr-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("book.epub");
        let file = File::create(&path).unwrap();
        let mut zip = ZipWriter::new(file);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("OPS/Images/diagram.png", deflated).unwrap();
        zip.write_all(image).unwrap();
        zip.finish().unwrap();
        path
    }

    #[test]
    fn embedded_text_short_circuits_epub_io_and_ocr() {
        let ocr = RecordingOcr::new(vec!["should not run".into()]);
        let evidence = review_image_text_evidence(
            Path::new("missing.epub"),
            &candidate(
                123,
                vec!["  Family   tree  ".into(), "family tree".into()],
            ),
            &ocr,
            &CancellationToken::default(),
        )
        .unwrap();

        assert_eq!(evidence.source, ReviewImageTextSource::Embedded);
        assert_eq!(evidence.text, vec!["Family tree"]);
        assert_eq!(ocr.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn ocr_reads_only_the_selected_candidate_when_embedded_text_is_absent() {
        let image = b"selected-image-bytes";
        let epub = temp_epub(image);
        let ocr = RecordingOcr::new(vec![
            " Diagram   label ".into(),
            "diagram label".into(),
            "Second line".into(),
        ]);

        let evidence = review_image_text_evidence(
            &epub,
            &candidate(image.len() as u64, Vec::new()),
            &ocr,
            &CancellationToken::default(),
        )
        .unwrap();

        assert_eq!(evidence.source, ReviewImageTextSource::Ocr);
        assert_eq!(evidence.text, vec!["Diagram label", "Second line"]);
        assert_eq!(ocr.calls.load(Ordering::SeqCst), 1);
        assert_eq!(ocr.image.lock().unwrap().as_slice(), image);
        let _ = fs::remove_dir_all(epub.parent().unwrap());
    }

    #[test]
    fn stale_candidate_size_is_rejected_before_ocr_runs() {
        let epub = temp_epub(b"image");
        let ocr = RecordingOcr::new(vec!["text".into()]);
        let error = review_image_text_evidence(
            &epub,
            &candidate(999, Vec::new()),
            &ocr,
            &CancellationToken::default(),
        )
        .unwrap_err();

        assert!(error.contains("changed after image candidates were discovered"));
        assert_eq!(ocr.calls.load(Ordering::SeqCst), 0);
        let _ = fs::remove_dir_all(epub.parent().unwrap());
    }

    #[test]
    fn cancellation_short_circuits_before_epub_io_or_ocr() {
        let cancellation = CancellationToken::default();
        cancellation.request();
        let ocr = RecordingOcr::new(vec!["text".into()]);
        let error = review_image_text_evidence(
            Path::new("missing.epub"),
            &candidate(100, Vec::new()),
            &ocr,
            &cancellation,
        )
        .unwrap_err();

        assert_eq!(error, "EPUB image OCR was cancelled.");
        assert_eq!(ocr.calls.load(Ordering::SeqCst), 0);
    }
}
