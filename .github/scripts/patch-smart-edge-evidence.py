from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)


# Persist edge identity and bounded silence evidence in review.json.
audio_path = Path("crates/storyteller-core/src/audio_review.rs")
audio = audio_path.read_text(encoding="utf-8")
audio = replace_once(
    audio,
    '''#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioReviewClassification {''',
    '''#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
pub enum AudioReviewClassification {''',
    "edge and silence evidence types",
)
audio = replace_once(
    audio,
    '''    #[serde(default)]
    pub suggestion: Option<AudioReviewSuggestion>,
    #[serde(default)]
    pub decision: AudioReviewDecision,''',
    '''    #[serde(default)]
    pub suggestion: Option<AudioReviewSuggestion>,
    #[serde(default)]
    pub edge: Option<AudioReviewEdge>,
    #[serde(default)]
    pub silence: Option<AudioReviewSilenceEvidence>,
    #[serde(default)]
    pub decision: AudioReviewDecision,''',
    "review item evidence fields",
)
audio = replace_once(
    audio,
    '''            let decision = saved_decisions.get(&id).cloned().unwrap_or_default();
            AudioReviewItem {
                id,
                alignment_index,
                audio_start_ms: segment.audio_start_ms,
                audio_end_ms: segment.audio_end_ms,
                transcript_text: segment.transcript_text.clone(),
                suggestion: edge_suggestion(alignment_index, first_matched, last_matched),
                decision,
            }''',
    '''            let decision = saved_decisions.get(&id).cloned().unwrap_or_default();
            let edge = edge_kind(alignment_index, first_matched, last_matched);
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
            }''',
    "review item edge assignment",
)
audio = replace_once(
    audio,
    '''pub fn accept_unmatched_audio_exclusion(path: &Path) -> Result<(), String> {
    accept_unmatched_audio_exclusion_inner(path, None)
}''',
    '''pub fn set_audio_review_silence_evidence(
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
}''',
    "silence evidence writer",
)
old_edge = '''fn edge_suggestion(
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
'''
new_edge = '''fn edge_kind(
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
'''
audio = replace_once(audio, old_edge, new_edge, "edge helper split")
audio_path.write_text(audio, encoding="utf-8", newline="\n")

# Export the evidence types/writer.
lib_path = Path("crates/storyteller-core/src/lib.rs")
lib = lib_path.read_text(encoding="utf-8")
lib = replace_once(
    lib,
    '''    read_audio_review_report, AudioReviewClassification, AudioReviewDecision,
    AudioReviewDecisionSource, AudioReviewDestination, AudioReviewItem, AudioReviewPolicy,
    AudioReviewReport, AudioReviewSuggestion, AudioReviewSummary,''',
    '''    read_audio_review_report, set_audio_review_silence_evidence, AudioReviewClassification,
    AudioReviewDecision, AudioReviewDecisionSource, AudioReviewDestination, AudioReviewEdge,
    AudioReviewItem, AudioReviewPolicy, AudioReviewReport, AudioReviewSilenceEvidence,
    AudioReviewSuggestion, AudioReviewSummary,''',
    "lib review evidence exports",
)
lib_path.write_text(lib, encoding="utf-8", newline="\n")

# Review Audio analyzes only already-bounded leading/trailing unmatched segments.
backend_path = Path("crates/storyteller-ui/src/pipeline_backend.rs")
backend = backend_path.read_text(encoding="utf-8")
backend = replace_once(
    backend,
    '''    env, fs,
    path::{Path, PathBuf},
    time::{Instant, UNIX_EPOCH},''',
    '''    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Instant, UNIX_EPOCH},''',
    "backend Command import",
)
backend = replace_once(
    backend,
    '''    prepared_job_sources, publish_validated_epub, read_audio_review_report,
    spawn_pipeline_worker_with_preflight, validate_readaloud_epub, write_validation_report,
    AudioCodec, HardwareProfile, Job, JobWorkspace, LiveMetrics, PipelineBackend,
    PipelineEnvironment, PipelineStage, PipelineWorkerHandle, ResourceRequest, ResourceScheduler,
    RuntimeCoordinator, StagePlan, StageRunContext, StageRunError, StageRunOutput,''',
    '''    prepared_job_sources, publish_validated_epub, read_audio_review_report,
    run_cancellable_command, set_audio_review_silence_evidence, spawn_pipeline_worker_with_preflight,
    validate_readaloud_epub, write_validation_report, AudioCodec, AudioReviewEdge,
    AudioReviewSilenceEvidence, HardwareProfile, Job, JobWorkspace, LiveMetrics, PipelineBackend,
    PipelineEnvironment, PipelineStage, PipelineWorkerHandle, ResourceRequest, ResourceScheduler,
    RuntimeCoordinator, StagePlan, StageRunContext, StageRunError, StageRunOutput,''',
    "backend review evidence imports",
)
backend = replace_once(
    backend,
    '''        context.set_metrics(
            LiveMetrics {
                match_percent: Some(summary.match_percent),
                backend: Some("Storyteller audio review gate".into()),
                ..LiveMetrics::default()
            },
            self.elapsed_millis(),
        );''',
    '''        if summary.unmatched_segments > 0 {
            let prepared = prepared_job_sources(context.job(), context.workspace())
                .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
            let report = read_audio_review_report(&report_path)
                .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
            let ffmpeg = resolve_executable("STORYTELLER_FFMPEG", "ffmpeg");
            let cancellation = context.cancellation_token();
            for item in report.unmatched.iter().filter(|item| item.edge.is_some()) {
                context
                    .set_activity(
                        match item.edge {
                            Some(AudioReviewEdge::Introduction) => "Checking leading unmatched audio evidence",
                            Some(AudioReviewEdge::Credits) => "Checking trailing unmatched audio evidence",
                            None => "Checking unmatched audio evidence",
                        },
                        self.elapsed_millis(),
                    )
                    .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
                match detect_review_silence(
                    &ffmpeg,
                    prepared.audiobook(),
                    item.audio_start_ms,
                    item.audio_end_ms,
                    &cancellation,
                ) {
                    Ok(evidence) => set_audio_review_silence_evidence(&report_path, &item.id, evidence)
                        .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?,
                    Err(error) if cancellation.is_requested() => {
                        return Err(StageRunError::cancelled(error, self.elapsed_millis()));
                    }
                    Err(_) => {
                        // Silence evidence is advisory. A failed probe must not erase or auto-resolve review work.
                    }
                }
            }
        }
        context.set_metrics(
            LiveMetrics {
                match_percent: Some(summary.match_percent),
                backend: Some("Storyteller audio review gate".into()),
                ..LiveMetrics::default()
            },
            self.elapsed_millis(),
        );''',
    "bounded review evidence analysis",
)
insert_anchor = '''fn effective_language(job: &Job) -> String {'''
helper = '''const REVIEW_SILENCE_THRESHOLD_DB: i16 = -38;
const REVIEW_MINIMUM_SILENCE_MS: u64 = 350;

fn detect_review_silence(
    ffmpeg: &Path,
    audiobook: &Path,
    start_ms: u64,
    end_ms: u64,
    cancellation: &storyteller_core::CancellationToken,
) -> Result<AudioReviewSilenceEvidence, String> {
    let duration_ms = end_ms.saturating_sub(start_ms);
    if duration_ms == 0 {
        return Err("Audio review segment has no duration.".into());
    }
    let filter = format!(
        "asetpts=PTS-STARTPTS,silencedetect=noise={}dB:d={:.3}",
        REVIEW_SILENCE_THRESHOLD_DB,
        REVIEW_MINIMUM_SILENCE_MS as f64 / 1000.0
    );
    let mut command = Command::new(ffmpeg);
    command
        .arg("-hide_banner")
        .arg("-nostdin")
        .arg("-loglevel")
        .arg("info")
        .arg("-ss")
        .arg(format_seconds(start_ms))
        .arg("-t")
        .arg(format_seconds(duration_ms))
        .arg("-i")
        .arg(audiobook)
        .arg("-af")
        .arg(filter)
        .arg("-f")
        .arg("null")
        .arg("-");
    let output = run_cancellable_command(&mut command, cancellation, |_, _| {})
        .map_err(|error| error.to_string())?;
    if !output.success {
        return Err(format!(
            "FFmpeg silence probe failed with exit code {:?}.",
            output.exit_code
        ));
    }
    Ok(AudioReviewSilenceEvidence {
        silent_ms: parse_silence_duration_ms(&output.stderr, duration_ms),
        duration_ms,
        threshold_db: REVIEW_SILENCE_THRESHOLD_DB,
        minimum_silence_ms: REVIEW_MINIMUM_SILENCE_MS,
    })
}

fn parse_silence_duration_ms(stderr: &str, duration_ms: u64) -> u64 {
    let mut open_start = None::<f64>;
    let mut intervals = Vec::<(f64, f64)>::new();
    for line in stderr.lines() {
        if let Some(value) = value_after_marker(line, "silence_start:") {
            open_start = value.parse::<f64>().ok();
        }
        if let Some(value) = value_after_marker(line, "silence_end:") {
            if let (Some(start), Ok(end)) = (open_start.take(), value.parse::<f64>()) {
                intervals.push((start, end));
            }
        }
    }
    if let Some(start) = open_start {
        intervals.push((start, duration_ms as f64 / 1000.0));
    }
    intervals
        .into_iter()
        .map(|(start, end)| {
            let start_ms = (start.max(0.0) * 1000.0).round() as u64;
            let end_ms = (end.max(0.0) * 1000.0).round() as u64;
            end_ms.min(duration_ms).saturating_sub(start_ms.min(duration_ms))
        })
        .fold(0u64, u64::saturating_add)
        .min(duration_ms)
}

fn value_after_marker<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    let (_, value) = line.split_once(marker)?;
    value.split_whitespace().next()
}

fn format_seconds(milliseconds: u64) -> String {
    format!("{}.{:03}", milliseconds / 1000, milliseconds % 1000)
}

'''
backend = replace_once(backend, insert_anchor, helper + insert_anchor, "silence evidence helpers")
backend_path.write_text(backend, encoding="utf-8", newline="\n")

# Surface edge/silence evidence beside the durable decision in the allocator.
review_path = Path("crates/storyteller-ui/src/review_ui.rs")
review = review_path.read_text(encoding="utf-8")
review = replace_once(
    review,
    '''    AlignmentDocument, AudioReviewDecision, AudioReviewDecisionSource, AudioReviewDestination, Job,
    JobQueue, JobStatus, PipelineStage, DEFAULT_REVIEW_CANDIDATE_LIMIT,''',
    '''    AlignmentDocument, AudioReviewDecision, AudioReviewDecisionSource, AudioReviewDestination,
    AudioReviewEdge, AudioReviewItem, Job, JobQueue, JobStatus, PipelineStage,
    DEFAULT_REVIEW_CANDIDATE_LIMIT,''',
    "review UI evidence imports",
)
review = replace_once(
    review,
    '''    ui.set_review_item_decision_text(decision_text(&item.decision).into());''',
    '''    ui.set_review_item_decision_text(review_context_text(item).into());''',
    "review evidence display",
)
review = replace_once(
    review,
    '''fn decision_text(decision: &AudioReviewDecision) -> String {''',
    '''fn review_context_text(item: &AudioReviewItem) -> String {
    let decision = decision_text(&item.decision);
    let edge = match item.edge {
        Some(AudioReviewEdge::Introduction) => Some("Smart evidence: leading edge / Introduction candidate"),
        Some(AudioReviewEdge::Credits) => Some("Smart evidence: trailing edge / Credits candidate"),
        None => None,
    };
    let silence = item.silence.map(|evidence| {
        format!(
            "Silence evidence: {}% silent at {} dB (minimum gap {:.2}s)",
            evidence.silence_percent(),
            evidence.threshold_db,
            evidence.minimum_silence_ms as f64 / 1000.0
        )
    });
    [Some(decision), edge.map(str::to_string), silence]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" — ")
}

fn decision_text(decision: &AudioReviewDecision) -> String {''',
    "review context helper",
)
review_path.write_text(review, encoding="utf-8", newline="\n")
