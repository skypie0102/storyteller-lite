from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)


path = Path("crates/storyteller-core/src/audio_review.rs")
text = path.read_text(encoding="utf-8")

text = replace_once(
    text,
    '''pub fn create_audio_review_report_with_draft(
    alignment_path: &Path,
    destination: &Path,
    draft_path: Option<&Path>,
    _policy: AudioReviewPolicy,
) -> Result<AudioReviewSummary, String> {''',
    '''pub fn create_audio_review_report_with_draft(
    alignment_path: &Path,
    destination: &Path,
    draft_path: Option<&Path>,
    policy: AudioReviewPolicy,
) -> Result<AudioReviewSummary, String> {''',
    "policy parameter",
)

text = replace_once(
    text,
    '''    let last_matched = alignment
        .segments
        .iter()
        .rposition(|segment| segment.status == AlignmentStatus::Matched);

    let unmatched = alignment''',
    '''    let last_matched = alignment
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

    let unmatched = alignment''',
    "edge anchors",
)

text = replace_once(
    text,
    '''            let decision = saved_decisions.get(&id).cloned().unwrap_or_default();
            let edge = edge_kind(alignment_index, first_matched, last_matched);
            AudioReviewItem {''',
    '''            let edge = edge_kind(alignment_index, first_matched, last_matched);
            let decision = review_decision_for_policy(
                saved_decisions.get(&id),
                policy,
                edge,
                introduction_anchor,
                credits_anchor,
            );
            AudioReviewItem {''',
    "policy decision selection",
)

text = replace_once(
    text,
    '''fn edge_suggestion(edge: AudioReviewEdge) -> AudioReviewSuggestion {
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

#[cfg(test)]''',
    '''fn edge_suggestion(edge: AudioReviewEdge) -> AudioReviewSuggestion {
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

#[cfg(test)]''',
    "smart decision helpers",
)

old_test = '''    #[test]
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
'''
new_test = '''    #[test]
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
        assert!(report.unmatched.iter().all(|item| item.decision.is_pending()));
        assert!(!report.is_complete());
    }
'''
text = replace_once(text, old_test, new_test, "smart/review-all tests")

text = replace_once(
    text,
    '''        assert!(matches!(
            restored.unmatched[0].decision,
            AudioReviewDecision::Excluded { .. }
        ));
        assert_eq!(restored.pending_count(), 1);
    }

    #[test]
    fn legacy_continue_action_bulk_excludes_only_pending_items() {''',
    '''        assert!(matches!(
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
        assert_eq!(read_audio_review_report(&report_path).unwrap().pending_count(), 0);

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
    fn legacy_continue_action_bulk_excludes_only_pending_items() {''',
    "manual/stale automatic tests",
)

text = replace_once(
    text,
    '''            AudioReviewPolicy::Smart,
        )
        .unwrap();
        accept_unmatched_audio_exclusion_with_draft(&report_path, &draft_path).unwrap();''',
    '''            AudioReviewPolicy::ReviewAll,
        )
        .unwrap();
        accept_unmatched_audio_exclusion_with_draft(&report_path, &draft_path).unwrap();''',
    "bulk exclusion review-all setup",
)

path.write_text(text, encoding="utf-8", newline="\n")
