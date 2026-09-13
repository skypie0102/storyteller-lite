use crate::{AlignmentDocument, AlignmentStatus};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};

const REVIEW_DRAFT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
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
pub enum AudioReviewEdge {
    Introduction,
    Credits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioReviewSilenceEvidence {
    pub silent_ms: u64,
    pub duration_ms: u64,
    pub threshold_db: i16,
    pub minimum_silence_ms: u64,
}

impl AudioReviewSilenceEvidence {
    pub fn silence_percent(&self) -> u8 {
        if self.duration_ms == 0 {
            return 0;
        }
        ((self.silent_ms.saturating_mul(100) / self.duration_ms).min(100)) as u8
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioReviewClassification {
    Introduction,
    Credits,
    GraphicReadout,
    ExtraAudio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioReviewSupplementalPlacement {
    BeforeAnchor,
    AfterAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioReviewDestination {
    /// Existing EPUB content-document href used either as the direct destination or as
    /// the spine anchor for a generated supplemental page.
    pub href: String,
    #[serde(default)]
    pub line_index: Option<usize>,
    #[serde(default)]
    pub image_href: Option<String>,
    #[serde(default)]
    pub supplemental: Option<AudioReviewSupplementalPlacement>,
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
    pub edge: Option<AudioReviewEdge>,
    #[serde(default)]
    pub silence: Option<AudioReviewSilenceEvidence>,
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
    policy: AudioReviewPolicy,
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
    let introduction_anchor = first_matched
        .and_then(|index| alignment.segments.get(index))
        .and_then(|segment| segment.book_start.as_ref())
        .map(|position| position.href.as_str());
    let credits_anchor = last_matched
        .and_then(|index| alignment.segments.get(index))
        .and_then(|segment| segment.book_end.as_ref())
        .map(|position| position.href.as_str());

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
            let edge = edge_kind(alignment_index, first_matched, last_matched);
            let decision = review_decision_for_policy(
                saved_decisions.get(&id),
                policy,
                edge,
                introduction_anchor,
                credits_anchor,
            );
            AudioReviewItem {
                id,
                alignment_index,
                audio_start_ms: segment.audio_start_ms,
                audio_end_ms: segment.audio_end_ms,
                transcript_text: segment.transcript_text.clone(),
                suggestion: edge.map(edge_suggestion),
                edge,
                silence: None,
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

pub fn set_audio_review_silence_evidence(
    report_path: &Path,
    item_id: &str,
    evidence: AudioReviewSilenceEvidence,
) -> Result<(), String> {
    if evidence.duration_ms == 0 || evidence.silent_ms > evidence.duration_ms {
        return Err("Audio review silence evidence contains an invalid duration.".into());
    }
    let mut report = read_audio_review_report(report_path)?;
    let item = report
        .unmatched
        .iter_mut()
        .find(|item| item.id == item_id)
        .ok_or_else(|| format!("Audio review item {item_id} was not found."))?;
    item.silence = Some(evidence);
    validate_audio_review_report(&report)?;
    write_audio_review_report(report_path, &report)
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
        AudioReviewDecision::Assigned {
            destination,
            classification,
            ..
        } => {
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
            let destination_kinds = usize::from(destination.line_index.is_some())
                + usize::from(destination.image_href.is_some())
                + usize::from(destination.supplemental.is_some());
            if destination_kinds != 1 {
                return Err(
                    "Assigned audio review destination must identify exactly one text, image, or supplemental-page target."
                        .into(),
                );
            }
            match destination.supplemental {
                Some(AudioReviewSupplementalPlacement::BeforeAnchor)
                    if *classification != Some(AudioReviewClassification::Introduction) =>
                {
                    Err(
                        "A supplemental page before the anchor must be classified as Introduction."
                            .into(),
                    )
                }
                Some(AudioReviewSupplementalPlacement::AfterAnchor)
                    if *classification != Some(AudioReviewClassification::Credits) =>
                {
                    Err(
                        "A supplemental page after the anchor must be classified as Credits."
                            .into(),
                    )
                }
                _ => Ok(()),
            }
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

fn edge_kind(
    alignment_index: usize,
    first_matched: Option<usize>,
    last_matched: Option<usize>,
) -> Option<AudioReviewEdge> {
    match (first_matched, last_matched) {
        (Some(first), _) if alignment_index < first => Some(AudioReviewEdge::Introduction),
        (_, Some(last)) if alignment_index > last => Some(AudioReviewEdge::Credits),
        _ => None,
    }
}

fn edge_suggestion(edge: AudioReviewEdge) -> AudioReviewSuggestion {
    match edge {
        AudioReviewEdge::Introduction => AudioReviewSuggestion {
            classification: AudioReviewClassification::Introduction,
            reason: "Unmatched narration occurs before the first matched book segment.".into(),
        },
        AudioReviewEdge::Credits => AudioReviewSuggestion {
            classification: AudioReviewClassification::Credits,
            reason: "Unmatched narration occurs after the last matched book segment.".into(),
        },
    }
}

fn review_decision_for_policy(
    saved: Option<&AudioReviewDecision>,
    policy: AudioReviewPolicy,
    edge: Option<AudioReviewEdge>,
    introduction_anchor: Option<&str>,
    credits_anchor: Option<&str>,
) -> AudioReviewDecision {
    if let Some(saved) = saved {
        match saved {
            AudioReviewDecision::Assigned {
                source: AudioReviewDecisionSource::Manual,
                ..
            }
            | AudioReviewDecision::Excluded {
                source: AudioReviewDecisionSource::Manual,
                ..
            } => return saved.clone(),
            _ => {}
        }
    }

    if policy == AudioReviewPolicy::ReviewAll {
        return AudioReviewDecision::Pending;
    }

    edge.and_then(|edge| smart_edge_decision(edge, introduction_anchor, credits_anchor))
        .unwrap_or_default()
}

fn smart_edge_decision(
    edge: AudioReviewEdge,
    introduction_anchor: Option<&str>,
    credits_anchor: Option<&str>,
) -> Option<AudioReviewDecision> {
    let (anchor, placement, classification) = match edge {
        AudioReviewEdge::Introduction => (
            introduction_anchor?,
            AudioReviewSupplementalPlacement::BeforeAnchor,
            AudioReviewClassification::Introduction,
        ),
        AudioReviewEdge::Credits => (
            credits_anchor?,
            AudioReviewSupplementalPlacement::AfterAnchor,
            AudioReviewClassification::Credits,
        ),
    };
    Some(AudioReviewDecision::Assigned {
        destination: AudioReviewDestination {
            href: anchor.to_string(),
            line_index: None,
            image_href: None,
            supplemental: Some(placement),
        },
        classification: Some(classification),
        source: AudioReviewDecisionSource::Automatic,
    })
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
    fn smart_preserves_intro_and_credits_automatically() {
        let (alignment_path, report_path, draft_path) = temp_paths("smart-edge");
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
        assert_eq!(summary.pending_segments, 0);

        let report = read_audio_review_report(&report_path).unwrap();
        assert!(report.is_complete());
        assert!(matches!(
            &report.unmatched[0].decision,
            AudioReviewDecision::Assigned {
                destination: AudioReviewDestination {
                    href,
                    supplemental: Some(AudioReviewSupplementalPlacement::BeforeAnchor),
                    ..
                },
                classification: Some(AudioReviewClassification::Introduction),
                source: AudioReviewDecisionSource::Automatic,
            } if href == "chapter.xhtml"
        ));
        assert!(matches!(
            &report.unmatched[1].decision,
            AudioReviewDecision::Assigned {
                destination: AudioReviewDestination {
                    href,
                    supplemental: Some(AudioReviewSupplementalPlacement::AfterAnchor),
                    ..
                },
                classification: Some(AudioReviewClassification::Credits),
                source: AudioReviewDecisionSource::Automatic,
            } if href == "chapter.xhtml"
        ));
    }

    #[test]
    fn review_all_keeps_edge_narration_pending() {
        let (alignment_path, report_path, draft_path) = temp_paths("review-all");
        fs::write(
            &alignment_path,
            serde_json::to_vec_pretty(&sample_alignment()).unwrap(),
        )
        .unwrap();

        let summary = create_audio_review_report_with_draft(
            &alignment_path,
            &report_path,
            Some(&draft_path),
            AudioReviewPolicy::ReviewAll,
        )
        .unwrap();
        assert_eq!(summary.unmatched_segments, 2);
        assert_eq!(summary.pending_segments, 2);
        let report = read_audio_review_report(&report_path).unwrap();
        assert!(report
            .unmatched
            .iter()
            .all(|item| item.decision.is_pending()));
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
            AudioReviewDecision::Excluded {
                source: AudioReviewDecisionSource::Manual,
                ..
            }
        ));
        assert!(matches!(
            restored.unmatched[1].decision,
            AudioReviewDecision::Assigned {
                source: AudioReviewDecisionSource::Automatic,
                ..
            }
        ));
        assert_eq!(restored.pending_count(), 0);
    }

    #[test]
    fn review_all_does_not_restore_stale_automatic_decisions() {
        let (alignment_path, report_path, draft_path) = temp_paths("policy-switch");
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
        assert_eq!(
            read_audio_review_report(&report_path)
                .unwrap()
                .pending_count(),
            0
        );

        let summary = create_audio_review_report_with_draft(
            &alignment_path,
            &report_path,
            Some(&draft_path),
            AudioReviewPolicy::ReviewAll,
        )
        .unwrap();
        assert_eq!(summary.pending_segments, 2);
        assert!(read_audio_review_report(&report_path)
            .unwrap()
            .unmatched
            .iter()
            .all(|item| item.decision.is_pending()));
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
            AudioReviewPolicy::ReviewAll,
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
