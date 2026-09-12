use crate::{AlignmentDocument, AlignmentStatus};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioReviewReport {
    pub total_segments: usize,
    pub matched_segments: usize,
    pub match_percent: f64,
    pub unmatched: Vec<AudioReviewItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioReviewItem {
    pub alignment_index: usize,
    pub audio_start_ms: u64,
    pub audio_end_ms: u64,
    pub transcript_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioReviewSummary {
    pub total_segments: usize,
    pub unmatched_segments: usize,
    pub match_percent: f64,
}

pub fn create_audio_review_report(
    alignment_path: &Path,
    destination: &Path,
) -> Result<AudioReviewSummary, String> {
    let data = fs::read(alignment_path).map_err(|error| {
        format!(
            "Could not read alignment map {}: {error}",
            alignment_path.display()
        )
    })?;
    let alignment: AlignmentDocument = serde_json::from_slice(&data).map_err(|error| {
        format!(
            "Could not parse alignment map {}: {error}",
            alignment_path.display()
        )
    })?;
    if alignment.segments.len() != alignment.total_segments {
        return Err("Alignment map segment count is inconsistent.".into());
    }

    let unmatched = alignment
        .segments
        .iter()
        .enumerate()
        .filter(|(_, segment)| segment.status == AlignmentStatus::Unmatched)
        .map(|(alignment_index, segment)| AudioReviewItem {
            alignment_index,
            audio_start_ms: segment.audio_start_ms,
            audio_end_ms: segment.audio_end_ms,
            transcript_text: segment.transcript_text.clone(),
        })
        .collect::<Vec<_>>();
    let report = AudioReviewReport {
        total_segments: alignment.total_segments,
        matched_segments: alignment.matched_segments,
        match_percent: alignment.match_percent,
        unmatched,
    };
    let summary = AudioReviewSummary {
        total_segments: report.total_segments,
        unmatched_segments: report.unmatched.len(),
        match_percent: report.match_percent,
    };

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Could not create audio review destination {}: {error}",
                parent.display()
            )
        })?;
    }
    let json = serde_json::to_vec_pretty(&report)
        .map_err(|error| format!("Could not serialize audio review report: {error}"))?;
    fs::write(destination, json).map_err(|error| {
        format!(
            "Could not write audio review report {}: {error}",
            destination.display()
        )
    })?;
    Ok(summary)
}

pub fn read_audio_review_report(path: &Path) -> Result<AudioReviewReport, String> {
    let data = fs::read(path)
        .map_err(|error| format!("Could not read audio review report {}: {error}", path.display()))?;
    let report: AudioReviewReport = serde_json::from_slice(&data).map_err(|error| {
        format!(
            "Could not parse audio review report {}: {error}",
            path.display()
        )
    })?;
    if report.unmatched.iter().any(|item| item.audio_end_ms < item.audio_start_ms) {
        return Err("Audio review report contains an invalid time range.".into());
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AlignmentSegment, CorpusPosition};

    #[test]
    fn review_report_contains_only_unmatched_segments() {
        let alignment = AlignmentDocument {
            algorithm: "test".into(),
            language: Some("en".into()),
            total_segments: 2,
            matched_segments: 1,
            match_percent: 50.0,
            segments: vec![
                AlignmentSegment {
                    audio_start_ms: 0,
                    audio_end_ms: 1000,
                    transcript_text: "matched".into(),
                    status: AlignmentStatus::Matched,
                    match_percent: Some(100.0),
                    book_start: Some(CorpusPosition {
                        href: "chapter.xhtml".into(),
                        char_offset: 0,
                    }),
                    book_end: Some(CorpusPosition {
                        href: "chapter.xhtml".into(),
                        char_offset: 7,
                    }),
                },
                AlignmentSegment {
                    audio_start_ms: 1000,
                    audio_end_ms: 2500,
                    transcript_text: "needs review".into(),
                    status: AlignmentStatus::Unmatched,
                    match_percent: None,
                    book_start: None,
                    book_end: None,
                },
            ],
        };
        let unmatched = alignment
            .segments
            .iter()
            .enumerate()
            .filter(|(_, segment)| segment.status == AlignmentStatus::Unmatched)
            .map(|(alignment_index, segment)| AudioReviewItem {
                alignment_index,
                audio_start_ms: segment.audio_start_ms,
                audio_end_ms: segment.audio_end_ms,
                transcript_text: segment.transcript_text.clone(),
            })
            .collect::<Vec<_>>();
        assert_eq!(unmatched.len(), 1);
        assert_eq!(unmatched[0].alignment_index, 1);
        assert_eq!(unmatched[0].audio_end_ms, 2500);
    }
}
