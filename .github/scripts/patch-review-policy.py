from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)


# Core policy type must participate in JobSettings hashing.
audio_review_path = Path("crates/storyteller-core/src/audio_review.rs")
audio_review = audio_review_path.read_text(encoding="utf-8")
audio_review = replace_once(
    audio_review,
    "#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]\n#[serde(rename_all = \"snake_case\")]\npub enum AudioReviewPolicy",
    "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]\n#[serde(rename_all = \"snake_case\")]\npub enum AudioReviewPolicy",
    "AudioReviewPolicy Hash derive",
)
audio_review_path.write_text(audio_review, encoding="utf-8", newline="\n")

# Snapshot review policy per queued job.
job_path = Path("crates/storyteller-core/src/job.rs")
job = job_path.read_text(encoding="utf-8")
job = replace_once(
    job,
    "use crate::{\n    PipelineProgress, PipelineStage, ResumeContext, ResumePlan, StageStatus, ValidatedResumePlan,\n};",
    "use crate::{\n    AudioReviewPolicy, PipelineProgress, PipelineStage, ResumeContext, ResumePlan, StageStatus,\n    ValidatedResumePlan,\n};",
    "job AudioReviewPolicy import",
)
job = replace_once(
    job,
    "    pub whisper_model: String,\n    /// Maximum number of independent transcription chunks that may run concurrently.",
    "    pub whisper_model: String,\n    /// Controls whether safe unmatched-audio cases may be resolved automatically or all are surfaced.\n    pub audio_review_policy: AudioReviewPolicy,\n    /// Maximum number of independent transcription chunks that may run concurrently.",
    "JobSettings policy field",
)
job = replace_once(
    job,
    '            whisper_model: "large-v3-turbo".into(),\n            whisper_workers: MIN_WHISPER_WORKERS,',
    '            whisper_model: "large-v3-turbo".into(),\n            audio_review_policy: AudioReviewPolicy::Smart,\n            whisper_workers: MIN_WHISPER_WORKERS,',
    "JobSettings policy default",
)
job_path.write_text(job, encoding="utf-8", newline="\n")

# Policy changes begin invalidation at Review Audio, not Analyze/Align.
resume_path = Path("crates/storyteller-core/src/resume.rs")
resume = resume_path.read_text(encoding="utf-8")
resume = replace_once(
    resume,
    "            PipelineStage::Align | PipelineStage::ReviewAudio => {\n                hasher.update(self.whisper_backend.as_bytes());\n                hasher.update(self.alignment_backend.as_bytes());\n                hasher.update(self.effective_language.as_bytes());\n                hasher.update(self.effective_whisper_model.as_bytes());\n            }",
    "            PipelineStage::Align => {\n                hasher.update(self.whisper_backend.as_bytes());\n                hasher.update(self.alignment_backend.as_bytes());\n                hasher.update(self.effective_language.as_bytes());\n                hasher.update(self.effective_whisper_model.as_bytes());\n            }\n            PipelineStage::ReviewAudio => {\n                hasher.update(self.whisper_backend.as_bytes());\n                hasher.update(self.alignment_backend.as_bytes());\n                hasher.update(self.effective_language.as_bytes());\n                hasher.update(self.effective_whisper_model.as_bytes());\n                hasher.update(format!(\"review-policy:{:?}\\n\", self.settings.audio_review_policy));\n            }",
    "ReviewAudio fingerprint arm",
)
insert_before = "    #[test]\n    fn audio_encoding_only_invalidates_encode_and_downstream() {"
policy_test = '''    #[test]
    fn review_policy_invalidates_review_audio_but_not_alignment() {
        let first = context(JobSettings::default());
        let changed = JobSettings {
            audio_review_policy: crate::AudioReviewPolicy::ReviewAll,
            ..JobSettings::default()
        };
        let second = context(changed);

        for stage in [
            PipelineStage::Prepare,
            PipelineStage::Analyze,
            PipelineStage::Align,
        ] {
            assert_eq!(
                first.stage_fingerprint(stage),
                second.stage_fingerprint(stage),
                "review policy unexpectedly changed {} fingerprint",
                stage.label()
            );
        }
        assert_ne!(
            first.stage_fingerprint(PipelineStage::ReviewAudio),
            second.stage_fingerprint(PipelineStage::ReviewAudio)
        );
    }

'''
resume = replace_once(resume, insert_before, policy_test + insert_before, "review policy fingerprint test")
resume_path.write_text(resume, encoding="utf-8", newline="\n")

# Review stage uses the snapshotted job policy instead of hardcoded Smart.
backend_path = Path("crates/storyteller-ui/src/pipeline_backend.rs")
backend = backend_path.read_text(encoding="utf-8")
backend = replace_once(
    backend,
    "            AudioReviewPolicy::Smart,\n        )",
    "            context.job().settings.audio_review_policy,\n        )",
    "Review Audio policy use",
)
backend = backend.replace("    AudioCodec, AudioReviewPolicy, HardwareProfile,", "    AudioCodec, HardwareProfile,")
backend_path.write_text(backend, encoding="utf-8", newline="\n")

# Queue UI parses and snapshots the selected policy.
main_path = Path("crates/storyteller-ui/src/main.rs")
main = main_path.read_text(encoding="utf-8")
main = replace_once(
    main,
    "    AudioBitrate, AudioCodec, AudioEncoding, Job, JobId, JobInputs, JobOutcome, JobQueue,\n    JobSettings, JobStatus, QueueMove, QueueState, StageStatus,",
    "    AudioBitrate, AudioCodec, AudioEncoding, AudioReviewPolicy, Job, JobId, JobInputs, JobOutcome,\n    JobQueue, JobSettings, JobStatus, QueueMove, QueueState, StageStatus,",
    "main AudioReviewPolicy import",
)
main = replace_once(
    main,
    "            let title = book_title(&epub_path);\n            let output_path = output_path(&epub_path, &title);\n            let settings = JobSettings {\n                audio,\n                whisper_workers,",
    "            let audio_review_policy = match parse_audio_review_policy(\n                ui.get_unmatched_audio_policy_text().as_str(),\n            ) {\n                Ok(policy) => policy,\n                Err(error) => {\n                    ui.set_status_text(error.into());\n                    return;\n                }\n            };\n            let title = book_title(&epub_path);\n            let output_path = output_path(&epub_path, &title);\n            let settings = JobSettings {\n                audio,\n                audio_review_policy,\n                whisper_workers,",
    "queue policy snapshot",
)
parse_anchor = "fn parse_whisper_workers(value: &str) -> Result<usize, String> {"
parse_policy = '''fn parse_audio_review_policy(value: &str) -> Result<AudioReviewPolicy, String> {
    match value.trim() {
        "Smart" => Ok(AudioReviewPolicy::Smart),
        "Review all" => Ok(AudioReviewPolicy::ReviewAll),
        value => Err(format!("Unsupported unmatched-audio policy: {value}")),
    }
}

'''
main = replace_once(main, parse_anchor, parse_policy + parse_anchor, "policy parser")
main_path.write_text(main, encoding="utf-8", newline="\n")

# Replace hardcoded Smart label with a two-mode selector in both creation rows.
slint_path = Path("crates/storyteller-ui/ui/app-window.slint")
slint = slint_path.read_text(encoding="utf-8")
slint = replace_once(
    slint,
    '    in-out property <string> whisper-workers-text: "1";\n',
    '    in-out property <string> whisper-workers-text: "1";\n    in-out property <string> unmatched-audio-policy-text: "Smart";\n',
    "unmatched audio policy property",
)
slint = replace_once(
    slint,
    '                Text { text: "Unmatched audio: Smart"; vertical-alignment: center; opacity: 0.72; }',
    '                Text { text: "Unmatched audio"; vertical-alignment: center; opacity: 0.72; }\n                ComboBox {\n                    model: ["Smart", "Review all"];\n                    current-index: root.unmatched-audio-policy-text == "Review all" ? 1 : 0;\n                    selected(value) => { root.unmatched-audio-policy-text = value; }\n                }',
    "primary unmatched policy selector",
)
active_audio_combo = '''                ComboBox {
                    model: ["32K", "64K", "96K"];
                    current-index: root.bitrate-text == "32K" ? 0 : root.bitrate-text == "96K" ? 2 : 1;
                    enabled: root.codec-text != "Copy";
                    selected(value) => { root.bitrate-text = value; }
                }
                Rectangle { horizontal-stretch: 1; }
                Button { text: "Queue"; primary: true; clicked => { root.queue-book(); } }
'''
active_with_policy = '''                ComboBox {
                    model: ["32K", "64K", "96K"];
                    current-index: root.bitrate-text == "32K" ? 0 : root.bitrate-text == "96K" ? 2 : 1;
                    enabled: root.codec-text != "Copy";
                    selected(value) => { root.bitrate-text = value; }
                }
                ComboBox {
                    model: ["Smart", "Review all"];
                    current-index: root.unmatched-audio-policy-text == "Review all" ? 1 : 0;
                    selected(value) => { root.unmatched-audio-policy-text = value; }
                }
                Rectangle { horizontal-stretch: 1; }
                Button { text: "Queue"; primary: true; clicked => { root.queue-book(); } }
'''
slint = replace_once(slint, active_audio_combo, active_with_policy, "queue another policy selector")
slint_path.write_text(slint, encoding="utf-8", newline="\n")
