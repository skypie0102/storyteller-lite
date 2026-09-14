use crate::{
    run_cancellable_command, AudioReviewImageCandidate, CancellationToken, CommandRunError,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};
use uuid::Uuid;
use zip::ZipArchive;

const MAX_IMAGE_BYTES: u64 = 25 * 1024 * 1024;
const COPY_BUFFER_BYTES: usize = 64 * 1024;
const MIN_TESSERACT_MEAN_CONFIDENCE: f32 = 20.0;
const MIN_OCR_ALNUM_CHARS: usize = 2;
const MAX_OCR_LINE_ALNUM_CHARS: usize = 240;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioReviewImageEvidenceSource {
    Embedded,
    Tesseract,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioReviewImageTextEvidence {
    pub source: AudioReviewImageEvidenceSource,
    pub lines: Vec<String>,
    #[serde(default)]
    pub confidence_percent: Option<u8>,
}

/// Returns text evidence for one already-bounded image candidate.
///
/// Embedded EPUB hints are always preferred and returned without invoking OCR. Tesseract is
/// optional and is launched only when the caller explicitly supplies an executable and the
/// candidate has no usable embedded text. This keeps OCR lazy and out of the permanent runtime
/// contract.
pub fn review_image_text_evidence(
    epub_path: &Path,
    candidate: &AudioReviewImageCandidate,
    tesseract_executable: Option<&Path>,
    cancellation: &CancellationToken,
) -> Result<Option<AudioReviewImageTextEvidence>, String> {
    if cancellation.is_requested() {
        return Err("EPUB image text evidence was cancelled.".into());
    }

    let embedded = normalize_evidence_lines(candidate.embedded_text.iter().map(String::as_str));
    if !embedded.is_empty() {
        return Ok(Some(AudioReviewImageTextEvidence {
            source: AudioReviewImageEvidenceSource::Embedded,
            lines: embedded,
            confidence_percent: None,
        }));
    }

    let Some(tesseract_executable) = tesseract_executable else {
        return Ok(None);
    };
    if candidate.byte_size == 0 || candidate.byte_size > MAX_IMAGE_BYTES {
        return Err("OCR image candidate has an invalid or oversized byte length.".into());
    }

    let temp_root = std::env::temp_dir().join(format!("storyteller-image-ocr-{}", Uuid::new_v4()));
    fs::create_dir_all(&temp_root).map_err(|error| {
        format!(
            "Could not create temporary OCR directory {}: {error}",
            temp_root.display()
        )
    })?;
    let _cleanup = TempDirectory(temp_root.clone());
    let image_path = temp_root.join(format!("candidate.{}", image_extension(candidate)));
    extract_candidate_image(epub_path, candidate, &image_path, cancellation)?;

    let mut command = Command::new(tesseract_executable);
    command
        .arg(&image_path)
        .arg("stdout")
        .arg("-l")
        .arg("eng")
        .arg("--psm")
        .arg("6")
        .arg("tsv");
    let output = run_cancellable_command(&mut command, cancellation, |_stream, _line| {}).map_err(
        |error| match error {
            CommandRunError::Cancelled => "EPUB image OCR was cancelled.".to_string(),
            other => format!("Could not run optional Tesseract OCR: {other}"),
        },
    )?;
    if !output.success {
        let detail = output.stderr.trim();
        return Err(if detail.is_empty() {
            "Optional Tesseract OCR exited unsuccessfully.".into()
        } else {
            format!("Optional Tesseract OCR failed: {detail}")
        });
    }

    parse_tesseract_tsv(&output.stdout)
}

fn extract_candidate_image(
    epub_path: &Path,
    candidate: &AudioReviewImageCandidate,
    destination: &Path,
    cancellation: &CancellationToken,
) -> Result<(), String> {
    let source = File::open(epub_path).map_err(|error| {
        format!(
            "Could not open EPUB {} for OCR: {error}",
            epub_path.display()
        )
    })?;
    let mut archive = ZipArchive::new(source)
        .map_err(|error| format!("Could not read EPUB ZIP container for OCR: {error}"))?;
    let mut entry = archive.by_name(&candidate.image_href).map_err(|error| {
        format!(
            "OCR image candidate {} is unavailable in the EPUB: {error}",
            candidate.image_href
        )
    })?;
    if !entry.is_file() || entry.size() == 0 || entry.size() > MAX_IMAGE_BYTES {
        return Err("OCR image candidate is missing, empty, or oversized.".into());
    }
    if entry.size() != candidate.byte_size {
        return Err(
            "OCR image candidate size no longer matches the discovered EPUB resource.".into(),
        );
    }

    let mut output = File::create(destination).map_err(|error| {
        format!(
            "Could not create temporary OCR image {}: {error}",
            destination.display()
        )
    })?;
    let mut copied = 0u64;
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    loop {
        if cancellation.is_requested() {
            return Err("EPUB image OCR was cancelled.".into());
        }
        let count = entry
            .read(&mut buffer)
            .map_err(|error| format!("Could not read OCR image candidate: {error}"))?;
        if count == 0 {
            break;
        }
        copied = copied
            .checked_add(count as u64)
            .ok_or("OCR image candidate size overflowed.")?;
        if copied > MAX_IMAGE_BYTES {
            return Err("OCR image candidate expanded beyond the allowed size.".into());
        }
        output
            .write_all(&buffer[..count])
            .map_err(|error| format!("Could not write temporary OCR image: {error}"))?;
    }
    output
        .flush()
        .map_err(|error| format!("Could not flush temporary OCR image: {error}"))?;
    if copied != candidate.byte_size {
        return Err("OCR image candidate copy length does not match discovery metadata.".into());
    }
    Ok(())
}

fn parse_tesseract_tsv(tsv: &str) -> Result<Option<AudioReviewImageTextEvidence>, String> {
    let mut confidences = Vec::<f32>::new();
    let mut words_by_line = BTreeMap::<(u32, u32, u32, u32), Vec<String>>::new();

    for (row_index, row) in tsv.lines().enumerate() {
        if row_index == 0 && row.starts_with("level\t") {
            continue;
        }
        if row.trim().is_empty() {
            continue;
        }
        let columns = row.splitn(12, '\t').collect::<Vec<_>>();
        if columns.len() != 12 {
            continue;
        }
        let confidence = match columns[10].trim().parse::<f32>() {
            Ok(value) if value >= 0.0 => value,
            _ => continue,
        };
        let text = normalize_line(columns[11]);
        if text.is_empty() {
            continue;
        }
        let key = (
            parse_index(columns[1]),
            parse_index(columns[2]),
            parse_index(columns[3]),
            parse_index(columns[4]),
        );
        words_by_line.entry(key).or_default().push(text);
        confidences.push(confidence);
    }

    if confidences.is_empty() {
        return Ok(None);
    }
    let mean = confidences.iter().sum::<f32>() / confidences.len() as f32;
    if mean < MIN_TESSERACT_MEAN_CONFIDENCE {
        return Ok(None);
    }

    let joined = words_by_line
        .values()
        .map(|words| words.join(" "))
        .collect::<Vec<_>>();
    let lines = normalize_evidence_lines(joined.iter().map(String::as_str));
    let alnum_count = lines
        .iter()
        .flat_map(|line| line.chars())
        .filter(|character| character.is_alphanumeric())
        .count();
    if alnum_count < MIN_OCR_ALNUM_CHARS {
        return Ok(None);
    }

    Ok(Some(AudioReviewImageTextEvidence {
        source: AudioReviewImageEvidenceSource::Tesseract,
        lines,
        confidence_percent: Some(mean.clamp(0.0, 100.0).round() as u8),
    }))
}

fn parse_index(value: &str) -> u32 {
    value.trim().parse().unwrap_or_default()
}

fn normalize_evidence_lines<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut lines = Vec::<String>::new();
    for value in values {
        let normalized = normalize_line(value);
        let alnum_count = normalized
            .chars()
            .filter(|character| character.is_alphanumeric())
            .count();
        if !(MIN_OCR_ALNUM_CHARS..=MAX_OCR_LINE_ALNUM_CHARS).contains(&alnum_count) {
            continue;
        }
        if lines
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&normalized))
        {
            continue;
        }
        lines.push(normalized);
    }
    lines
}

fn normalize_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn image_extension(candidate: &AudioReviewImageCandidate) -> String {
    let extension = candidate
        .image_href
        .rsplit_once('.')
        .map(|(_, value)| value)
        .unwrap_or("img")
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .take(8)
        .collect::<String>()
        .to_ascii_lowercase();
    if extension.is_empty() {
        "img".into()
    } else {
        extension
    }
}

struct TempDirectory(PathBuf);

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(hints: &[&str]) -> AudioReviewImageCandidate {
        AudioReviewImageCandidate {
            document_href: "OPS/Text/figure.xhtml".into(),
            image_href: "OPS/Images/diagram.png".into(),
            media_type: "image/png".into(),
            byte_size: 12,
            document_spine_index: 1,
            image_ordinal: 0,
            embedded_text: hints.iter().map(|value| (*value).to_string()).collect(),
        }
    }

    #[test]
    fn embedded_hints_short_circuit_ocr() {
        let evidence = review_image_text_evidence(
            Path::new("does-not-need-to-exist.epub"),
            &candidate(&[" Family   tree ", "family tree", "Figure two"]),
            Some(Path::new("definitely-missing-tesseract")),
            &CancellationToken::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(evidence.source, AudioReviewImageEvidenceSource::Embedded);
        assert_eq!(evidence.lines, vec!["Family tree", "Figure two"]);
        assert_eq!(evidence.confidence_percent, None);
    }

    #[test]
    fn no_hints_and_no_engine_returns_no_evidence() {
        assert_eq!(
            review_image_text_evidence(
                Path::new("does-not-need-to-exist.epub"),
                &candidate(&[]),
                None,
                &CancellationToken::default(),
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn tesseract_tsv_produces_lines_and_mean_confidence() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n5\t1\t1\t1\t1\t1\t0\t0\t10\t10\t90.0\tFamily\n5\t1\t1\t1\t1\t2\t10\t0\t10\t10\t80.0\ttree\n5\t1\t1\t1\t2\t1\t0\t10\t10\t10\t70.0\tSecond\n5\t1\t1\t1\t2\t2\t10\t10\t10\t10\t60.0\tfigure\n";
        let evidence = parse_tesseract_tsv(tsv).unwrap().unwrap();
        assert_eq!(evidence.source, AudioReviewImageEvidenceSource::Tesseract);
        assert_eq!(evidence.lines, vec!["Family tree", "Second figure"]);
        assert_eq!(evidence.confidence_percent, Some(75));
    }

    #[test]
    fn tesseract_tsv_rejects_low_confidence_or_empty_text() {
        let low = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n5\t1\t1\t1\t1\t1\t0\t0\t10\t10\t10.0\tWords\n";
        assert_eq!(parse_tesseract_tsv(low).unwrap(), None);
        let empty = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n";
        assert_eq!(parse_tesseract_tsv(empty).unwrap(), None);
    }
}
