use crate::{AlignmentDocument, AlignmentStatus};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};

const REVIEW_DRAFT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AudioReviewPolicy {
    #[default]
    Smart,
    ReviewAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioReviewDecisionSource {
    Automatic,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioReviewClassification {
    Introduction,
    Credits,
    GraphicReadout,
    ExtraAudio,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioReviewDestination {
    pub href: String,
    #[serde(default)]
    pub line_index: Option<usize>,
    #[serde(default)]
    pub image_href: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AudioReviewDecision {
    #[default]
    Pending,
    Assigned {
        destination: AudioReviewDestination,
        #[serde(default)]
        classification: Option<AudioReviewClassification>,
        source: AudioReviewDecisionSource,
    },
    Excluded {
        reason: String,
        source: AudioReviewDecisionSource,
    },
}

impl AudioReviewDecision {
    pub const fn is_pending(&self) -> bool {
        matches!(self, Self::Pending)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioReviewSuggestion {
    pub classification: AudioReviewClassification,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioReviewReport {
    pub total_segments: usize,
    pub matched_segments: usize,
    pub match_percent: f64,
    pub unmatched: Vec<AudioReviewItem>,
    /// Legacy compatibility bit. New code records explicit per-segment decisions.
    #[serde(default)]
    pub accepted_unmatched_exclusion: bool,
}

impl AudioReviewReport {
    pub fn pending_count(&self) -> usize {
        if self.accepted_unmatched_exclusion {
            return 0;
        }
        self.unmatched
            .iter()
            .filter(|item| item.decision.is_pending())
            .count()
    }

    pub fn is_complete(&self) -> bool {
        self.unmatched.is_empty()
            || self.accepted_unmatched_exclusion
            || self
                .unmatched
                .iter()
                .all(|item| !item.decision.is_pending())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioReviewItem {
    #[serde(default)]
    pub id: String,
    pub alignment_index: usize,
    pub audio_start_ms: u64,
    pub audio_end_ms: u64,
    pub transcript_text: String,
    #[serde(default)]
    pub suggestion: Option<AudioReviewSuggestion>,
    #[serde(default)]
    pub decision: AudioReviewDecision,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioReviewSummary {
    pub total_segments: usize,
    pub unmatched_segments: usize,
    pub pending_segments: usize,
    pub match_percent: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AudioReviewDraft {
    version: u32,
    decisions: BTreeMap<String, AudioReviewDecision>,
}

pub fn create_audio_review_report(
    alignment_path: &Path,
    destination: &Path,
) -> Result<AudioReviewSummary, String> {
    create_audio_review_report_with_draft(
        alignment_path,
        destination,
        None,
        AudioReviewPolicy::Smart,
    )
}

pub fn create_audio_review_report_with_draft(
    alignment_path: &Path,
    destination: &Path,
    draft_path: Option<&Path>,
    _policy: AudioReviewPolicy,
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

    let saved_decisions = match draft_path {
        Some(path) => read_review_draft(path)?,
        None => BTreeMap::new(),
    };
    let first_matched = alignment
        .segments
        .iter()
        .position(|segment| segment.status == AlignmentStatus::Matched);
    let last_matched = alignment
        .segments
        .iter()
        .rposition(|segment| segment.status == AlignmentStatus::Matched);

    let unmatched = alignment
        .segments
        .iter()
        .enumerate()
        .filter(|(_, segment)| segment.status == AlignmentStatus::Unmatched)
        .map(|(alignment_index, segment)| {
            let id = review_item_id(
                alignment_index,
                segment.audio_start_ms,
                segment.audio_end_ms,
                &segment.transcript_text,
            );
            let decision = saved_decisions.get(&id).cloned().unwrap_or_default();
            AudioReviewItem {
                id,
                alignment_index,
                audio_start_ms: segment.audio_start_ms,
                audio_end_ms: segment.audio_end_ms,
                transcript_text: segment.transcript_text.clone(),
                suggestion: edge_suggestion(alignment_index, first_matched, last_matched),
                decision,
            }
        })
        .collect::<Vec<_>>();
    let report = AudioReviewReport {
        total_segments: alignment.total_segments,
        matched_segments: alignment.matched_segments,
        match_percent: alignment.match_percent,
        unmatched,
        accepted_unmatched_exclusion: false,
    };
    validate_audio_review_report(&report)?;
    let summary = AudioReviewSummary {
        total_segments: report.total_segments,
        unmatched_segments: report.unmatched.len(),
        pending_segments: report.pending_count(),
        match_percent: report.match_percent,
    };

    write_audio_review_report(destination, &report)?;
    if let Some(path) = draft_path {
        write_review_draft(path, &report)?;
    }
    Ok(summary)
}

pub fn read_audio_review_report(path: &Path) -> Result<AudioReviewReport, String> {
    let data = fs::read(path).map_err(|error| {
        format!(
            "Could not read audio review report {}: {error}",
            path.display()
        )
    })?;
    let mut report: AudioReviewReport = serde_json::from_slice(&data).map_err(|error| {
        format!(
            "Could not parse audio review report {}: {error}",
            path.display()
        )
    })?;
    for item in &mut report.unmatched {
        if item.id.trim().is_empty() {
            item.id = review_item_id(
                item.alignment_index,
                item.audio_start_ms,
                item.audio_end_ms,
                &item.transcript_text,
            );
        }
        if report.accepted_unmatched_exclusion && item.decision.is_pending() {
            item.decision = AudioReviewDecision::Excluded {
                reason: "Legacy whole-report unmatched-audio exclusion.".into(),
                source: AudioReviewDecisionSource::Manual,
            };
        }
    }
    validate_audio_review_report(&report)?;
    Ok(report)
}

pub fn apply_audio_review_decision(
    report_path: &Path,
    draft_path: &Path,
    item_id: &str,
    decision: AudioReviewDecision,
) -> Result<(), String> {
    validate_decision(&decision)?;
    let mut report = read_audio_review_report(report_path)?;
    let item = report
        .unmatched
        .iter_mut()
        .find(|item| item.id == item_id)
        .ok_or_else(|| format!("Audio review item {item_id} was not found."))?;
    item.decision = decision;
    report.accepted_unmatched_exclusion = false;
    validate_audio_review_report(&report)?;
    write_audio_review_report(report_path, &report)?;
    write_review_draft(draft_path, &report)
}

pub fn accept_unmatched_audio_exclusion(path: &Path) -> Result<(), String> {
    accept_unmatched_audio_exclusion_inner(path, None)
}

pub fn accept_unmatched_audio_exclusion_with_draft(
    report_path: &Path,
    draft_path: &Path,
) -> Result<(), String> {
    accept_unmatched_audio_exclusion_inner(report_path, Some(draft_path))
}

fn accept_unmatched_audio_exclusion_inner(
    report_path: &Path,
    draft_path: Option<&Path>,
) -> Result<(), String> {
    let mut report = read_audio_review_report(report_path)?;
    if report.unmatched.is_empty() {
        return Ok(());
    }
    for item in &mut report.unmatched {
        if item.decision.is_pending() {
            item.decision = AudioReviewDecision::Excluded {
                reason: "User chose Continue without unmatched audio.".into(),
                source: AudioReviewDecisionSource::Manual,
            };
        }
    }
    report.accepted_unmatched_exclusion = true;
    write_audio_review_report(report_path, &report)?;
    if let Some(path) = draft_path {
        write_review_draft(path, &report)?;
    }
    Ok(())
}

fn read_review_draft(path: &Path) -> Result<BTreeMap<String, AudioReviewDecision>, String> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let data = fs::read(path).map_err(|error| {
        format!(
            "Could not read audio review draft {}: {error}",
            path.display()
        )
    })?;
    let draft: AudioReviewDraft = serde_json::from_slice(&data).map_err(|error| {
        format!(
            "Could not parse audio review draft {}: {error}",
            path.display()
        )
    })?;
    if draft.version != REVIEW_DRAFT_VERSION {
        return Err(format!(
            "Audio review draft {} uses unsupported version {}.",
            path.display(),
            draft.version
        ));
    }
    for decision in draft.decisions.values() {
        validate_decision(decision)?;
    }
    Ok(draft.decisions)
}

fn write_review_draft(path: &Path, report: &AudioReviewReport) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Could not create audio review draft directory {}: {error}",
                parent.display()
            )
        })?;
    }
    let decisions = report
        .unmatched
        .iter()
        .map(|item| (item.id.clone(), item.decision.clone()))
        .collect::<BTreeMap<_, _>>();
    let draft = AudioReviewDraft {
        version: REVIEW_DRAFT_VERSION,
        decisions,
    };
    let json = serde_json::to_vec_pretty(&draft)
        .map_err(|error| format!("Could not serialize audio review draft: {error}"))?;
    fs::write(path, json).map_err(|error| {
        format!(
            "Could not write audio review draft {}: {error}",
            path.display()
        )
    })
}

fn write_audio_review_report(path: &Path, report: &AudioReviewReport) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Could not create audio review destination {}: {error}",
                parent.display()
            )
        })?;
    }
    let json = serde_json::to_vec_pretty(report)
        .map_err(|error| format!("Could not serialize audio review report: {error}"))?;
    fs::write(path, json).map_err(|error| {
        format!(
            "Could not write audio review report {}: {error}",
            path.display()
        )
    })
}

fn validate_audio_review_report(report: &AudioReviewReport) -> Result<(), String> {
    if report.matched_segments > report.total_segments {
        return Err("Audio review report matched count exceeds its total segment count.".into());
    }
    let mut ids = BTreeMap::<&str, usize>::new();
    let mut previous_end = None::<u64>;
    for item in &report.unmatched {
        if item.id.trim().is_empty() {
            return Err("Audio review report contains an item without an identifier.".into());
        }
        if ids.insert(item.id.as_str(), item.alignment_index).is_some() {
            return Err("Audio review report contains duplicate item identifiers.".into());
        }
        if item.audio_end_ms <= item.audio_start_ms {
            return Err("Audio review report contains an invalid time range.".into());
        }
        if let Some(previous_end) = previous_end {
            if item.audio_start_ms < previous_end {
                return Err("Audio review report contains overlapping unmatched ranges.".into());
            }
        }
        previous_end = Some(item.audio_end_ms);
        validate_decision(&item.decision)?;
    }
    Ok(())
}

fn validate_decision(decision: &AudioReviewDecision) -> Result<(), String> {
    match decision {
        AudioReviewDecision::Pending => Ok(()),
        AudioReviewDecision::Assigned { destination, .. } => {
            if destination.href.trim().is_empty() {
                return Err("Assigned audio review destination href cannot be blank.".into());
            }
            if destination
                .image_href
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            {
                return Err("Assigned audio review image href cannot be blank.".into());
            }
            Ok(())
        }
        AudioReviewDecision::Excluded { reason, .. } => {
            if reason.trim().is_empty() {
                Err("Excluded audio review decision requires a reason.".into())
            } else {
                Ok(())
            }
        }
    }
}

fn review_item_id(
    alignment_index: usize,
    audio_start_ms: u64,
    audio_end_ms: u64,
    transcript_text: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(alignment_index.to_le_bytes());
    hasher.update(audio_start_ms.to_le_bytes());
    hasher.update(audio_end_ms.to_le_bytes());
    hasher.update(transcript_text.as_bytes());
    format!("review-{:x}", hasher.finalize())
}

fn edge_suggestion(
    alignment_index: usize,
    first_matched: Option<usize>,
    last_matched: Option<usize>,
) -> Option<AudioReviewSuggestion> {
    match (first_matched, last_matched) {
        (Some(first), _) if alignment_index < first => Some(AudioReviewSuggestion {
            classification: AudioReviewClassification::Introduction,
            reason: "Unmatched narration occurs before the first matched book segment.".into(),
        }),
        (_, Some(last)) if alignment_index > last => Some(AudioReviewSuggestion {
            classification: AudioReviewClassification::Credits,
            reason: "Unmatched narration occurs after the last matched book segment.".into(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AlignmentSegment, CorpusPosition};
    use std::path::PathBuf;

    fn sample_alignment() -> AlignmentDocument {
        AlignmentDocument {
            algorithm: "test".into(),
            language: Some("en".into()),
            total_segments: 4,
            matched_segments: 2,
            match_percent: 50.0,
            segments: vec![
                AlignmentSegment {
                    audio_start_ms: 0,
                    audio_end_ms: 1000,
                    transcript_text: "opening narration".into(),
                    status: AlignmentStatus::Unmatched,
                    match_percent: None,
                    book_start: None,
                    book_end: None,
                },
                AlignmentSegment {
                    audio_start_ms: 1000,
                    audio_end_ms: 2000,
                    transcript_text: "matched one".into(),
                    status: AlignmentStatus::Matched,
                    match_percent: Some(100.0),
                    book_start: Some(CorpusPosition {
                        href: "chapter.xhtml".into(),
                        line_index: 0,
                        char_offset: 0,
                    }),
                    book_end: Some(CorpusPosition {
                        href: "chapter.xhtml".into(),
                        line_index: 0,
                        char_offset: 7,
                    }),
                },
                AlignmentSegment {
                    audio_start_ms: 2000,
                    audio_end_ms: 3000,
                    transcript_text: "matched two".into(),
                    status: AlignmentStatus::Matched,
                    match_percent: Some(100.0),
                    book_start: Some(CorpusPosition {
                        href: "chapter.xhtml".into(),
                        line_index: 1,
                        char_offset: 0,
                    }),
                    book_end: Some(CorpusPosition {
                        href: "chapter.xhtml".into(),
                        line_index: 1,
                        char_offset: 7,
                    }),
                },
                AlignmentSegment {
                    audio_start_ms: 3000,
                    audio_end_ms: 4500,
                    transcript_text: "closing narration".into(),
                    status: AlignmentStatus::Unmatched,
                    match_percent: None,
                    book_start: None,
                    book_end: None,
                },
            ],
        }
    }

    fn temp_paths(label: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "storyteller-audio-review-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        (
            root.join("alignment.json"),
            root.join("review.json"),
            root.join("review-draft.json"),
        )
    }

    #[test]
    fn report_suggests_intro_and_credits_but_leaves_them_pending() {
        let (alignment_path, report_path, draft_path) = temp_paths("suggestions");
        fs::write(
            &alignment_path,
            serde_json::to_vec_pretty(&sample_alignment()).unwrap(),
        )
        .unwrap();

        let summary = create_audio_review_report_with_draft(
            &alignment_path,
            &report_path,
            Some(&draft_path),
            AudioReviewPolicy::Smart,
        )
        .unwrap();
        assert_eq!(summary.unmatched_segments, 2);
        assert_eq!(summary.pending_segments, 2);

        let report = read_audio_review_report(&report_path).unwrap();
        assert_eq!(
            report.unmatched[0]
                .suggestion
                .as_ref()
                .unwrap()
                .classification,
            AudioReviewClassification::Introduction
        );
        assert_eq!(
            report.unmatched[1]
                .suggestion
                .as_ref()
                .unwrap()
                .classification,
            AudioReviewClassification::Credits
        );
        assert!(!report.is_complete());
    }

    #[test]
    fn manual_decision_round_trips_through_durable_draft() {
        let (alignment_path, report_path, draft_path) = temp_paths("draft");
        fs::write(
            &alignment_path,
            serde_json::to_vec_pretty(&sample_alignment()).unwrap(),
        )
        .unwrap();
        create_audio_review_report_with_draft(
            &alignment_path,
            &report_path,
            Some(&draft_path),
            AudioReviewPolicy::Smart,
        )
        .unwrap();
        let first = read_audio_review_report(&report_path).unwrap().unmatched[0]
            .id
            .clone();
        apply_audio_review_decision(
            &report_path,
            &draft_path,
            &first,
            AudioReviewDecision::Excluded {
                reason: "User reviewed this segment.".into(),
                source: AudioReviewDecisionSource::Manual,
            },
        )
        .unwrap();

        create_audio_review_report_with_draft(
            &alignment_path,
            &report_path,
            Some(&draft_path),
            AudioReviewPolicy::Smart,
        )
        .unwrap();
        let restored = read_audio_review_report(&report_path).unwrap();
        assert!(matches!(
            restored.unmatched[0].decision,
            AudioReviewDecision::Excluded { .. }
        ));
        assert_eq!(restored.pending_count(), 1);
    }

    #[test]
    fn legacy_continue_action_bulk_excludes_only_pending_items() {
        let (alignment_path, report_path, draft_path) = temp_paths("bulk");
        fs::write(
            &alignment_path,
            serde_json::to_vec_pretty(&sample_alignment()).unwrap(),
        )
        .unwrap();
        create_audio_review_report_with_draft(
            &alignment_path,
            &report_path,
            Some(&draft_path),
            AudioReviewPolicy::Smart,
        )
        .unwrap();
        accept_unmatched_audio_exclusion_with_draft(&report_path, &draft_path).unwrap();
        let report = read_audio_review_report(&report_path).unwrap();
        assert!(report.is_complete());
        assert_eq!(report.pending_count(), 0);
        assert!(report.unmatched.iter().all(|item| matches!(
            item.decision,
            AudioReviewDecision::Excluded {
                source: AudioReviewDecisionSource::Manual,
                ..
            }
        )));
    }
}
