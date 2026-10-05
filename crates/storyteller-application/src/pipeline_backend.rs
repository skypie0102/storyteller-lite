use crate::chunked_transcription::{
    transcribe_audiobook_in_chunks, ChunkedTranscriptionConfig, ChunkedTranscriptionProgress,
};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Instant, UNIX_EPOCH},
};
use storyteller_core::{
    align_transcript_to_corpus, apply_smart_graphic_readouts, build_readaloud_epub,
    create_audio_review_report_with_draft, encode_audiobook, extract_epub_corpus,
    materialize_reviewed_alignment, prepare_job_sources, prepared_job_sources,
    publish_validated_epub, read_audio_review_report, run_cancellable_command,
    set_audio_review_silence_evidence, spawn_pipeline_worker_with_runtime_preflight,
    write_validation_report, AudioCodec, AudioReviewEdge, AudioReviewPolicy,
    AudioReviewSilenceEvidence, HardwareProfile, Job, JobWorkspace, LiveMetrics, PipelineBackend,
    PipelineEnvironment, PipelineStage, PipelineWorkerHandle, ResourceRequest, ResourceScheduler,
    RuntimeCoordinator, StageArtifacts, StagePlan, StageRunContext, StageRunError, StageRunOutput,
    ValidatedEpub,
};

pub(crate) struct LitePipelineBackend {
    attempt_started: Instant,
    cpu_threads: usize,
    validated_candidate: Option<ValidatedEpub>,
}

impl LitePipelineBackend {
    pub(crate) fn new(cpu_threads: usize) -> Self {
        Self {
            attempt_started: Instant::now(),
            cpu_threads: cpu_threads.max(1),
            validated_candidate: None,
        }
    }

    fn elapsed_millis(&self) -> u64 {
        self.attempt_started
            .elapsed()
            .as_millis()
            .min(u64::MAX as u128) as u64
    }

    fn run_prepare(
        &mut self,
        context: &mut StageRunContext<'_>,
    ) -> Result<StageRunOutput, StageRunError> {
        let stage_started = Instant::now();
        let activity_at = self.elapsed_millis();
        context
            .set_activity("Staging source EPUB and audiobook", activity_at)
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        let cancellation = context.cancellation_token();
        let prepared = match prepare_job_sources(context.job(), context.workspace(), &cancellation)
        {
            Ok(prepared) => prepared,
            Err(error) if cancellation.is_requested() => {
                return Err(StageRunError::cancelled(error, self.elapsed_millis()));
            }
            Err(error) => return Err(StageRunError::failed(error, self.elapsed_millis())),
        };

        let completed_at = self.elapsed_millis();
        context
            .set_stage_percent(100, completed_at)
            .map_err(|error| StageRunError::failed(error, completed_at))?;
        Ok(StageRunOutput::new(
            StageArtifacts::Prepare {
                epub: PathBuf::from(prepared.epub().file_name().expect("prepared EPUB filename")),
                audiobook: PathBuf::from(
                    prepared
                        .audiobook()
                        .file_name()
                        .expect("prepared audio filename"),
                ),
            },
            stage_started.elapsed().as_secs(),
            completed_at,
        ))
    }

    fn run_analyze(
        &mut self,
        context: &mut StageRunContext<'_>,
    ) -> Result<StageRunOutput, StageRunError> {
        let stage_started = Instant::now();
        let prepared = prepared_job_sources(context.job(), context.workspace())
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        let runtime = AnalyzeRuntime::discover(context.job())
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        let stage_dir = context.workspace().stage_dir(PipelineStage::Analyze);
        reset_stage_dir(&stage_dir, PipelineStage::Analyze)
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        let cancellation = context.cancellation_token();

        let corpus_path = stage_dir.join("book-corpus.json");
        context
            .set_activity("Extracting EPUB reading order", self.elapsed_millis())
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        match extract_epub_corpus(prepared.epub(), &corpus_path, &cancellation) {
            Ok(_) => {}
            Err(error) if cancellation.is_requested() => {
                return Err(StageRunError::cancelled(error, self.elapsed_millis()));
            }
            Err(error) => return Err(StageRunError::failed(error, self.elapsed_millis())),
        }

        let workers = context.job().settings.transcription_workers;
        let transcript_path = stage_dir.join("transcript.json");
        let config = ChunkedTranscriptionConfig {
            ffmpeg: runtime.ffmpeg.clone(),
            engine: runtime.engine.clone(),
            workers,
        };
        let mut metrics = LiveMetrics {
            backend: Some(runtime.engine.label().into()),
            model: Some(runtime.model_name.clone()),
            ..LiveMetrics::default()
        };
        context.set_metrics(metrics.clone(), self.elapsed_millis());
        context
            .set_activity(
                format!(
                    "Planning and transcribing audiobook with {workers} Transcription worker{}",
                    if workers == 1 { "" } else { "s" }
                ),
                self.elapsed_millis(),
            )
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;

        let result = transcribe_audiobook_in_chunks(
            prepared.audiobook(),
            &stage_dir,
            &transcript_path,
            &config,
            &cancellation,
            &mut |progress: ChunkedTranscriptionProgress| {
                metrics.current_item = Some(progress.completed_chunks as u64);
                metrics.total_items = Some(progress.total_chunks as u64);
                if let Some(backend) = progress.backend {
                    metrics.backend = Some(backend);
                }
                context.set_metrics(metrics.clone(), 0);
                context.set_stage_percent(progress.percent, 0)
            },
        );
        let summary = match result {
            Ok(summary) => summary,
            Err(error) if cancellation.is_requested() => {
                return Err(StageRunError::cancelled(error, self.elapsed_millis()));
            }
            Err(error) => return Err(StageRunError::failed(error, self.elapsed_millis())),
        };
        validate_nonempty_file(&transcript_path, "Merged transcript")
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        validate_nonempty_file(
            &stage_dir.join("transcription-plan.json"),
            "Transcription plan",
        )
        .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;

        metrics.current_item = Some(summary.chunks as u64);
        metrics.total_items = Some(summary.chunks as u64);
        metrics.total_audio_seconds = Some(summary.duration_ms as f64 / 1000.0);
        context.set_metrics(metrics, self.elapsed_millis());
        context
            .set_activity(
                format!(
                    "Transcribed {} chunks with {} worker(s)",
                    summary.chunks, summary.effective_workers
                ),
                self.elapsed_millis(),
            )
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        context
            .set_stage_percent(100, self.elapsed_millis())
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;

        Ok(StageRunOutput::new(
            StageArtifacts::Analyze {
                corpus: PathBuf::from("book-corpus.json"),
                plan: PathBuf::from("transcription-plan.json"),
                transcript: PathBuf::from("transcript.json"),
            },
            stage_started.elapsed().as_secs(),
            self.elapsed_millis(),
        ))
    }

    fn run_align(
        &mut self,
        context: &mut StageRunContext<'_>,
    ) -> Result<StageRunOutput, StageRunError> {
        let stage_started = Instant::now();
        let analyze_dir = context.workspace().stage_dir(PipelineStage::Analyze);
        let corpus_path = analyze_dir.join("book-corpus.json");
        let transcript_path = analyze_dir.join("transcript.json");
        let stage_dir = context.workspace().stage_dir(PipelineStage::Align);
        reset_stage_dir(&stage_dir, PipelineStage::Align)
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        let alignment_path = stage_dir.join("alignment.json");
        let cancellation = context.cancellation_token();

        let mut metrics = LiveMetrics {
            backend: Some("Storyteller monotonic n-gram aligner".into()),
            ..LiveMetrics::default()
        };
        context.set_metrics(metrics.clone(), self.elapsed_millis());
        context
            .set_activity(
                "Aligning transcript segments to EPUB text",
                self.elapsed_millis(),
            )
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;

        let result = align_transcript_to_corpus(
            &corpus_path,
            &transcript_path,
            &alignment_path,
            &cancellation,
            &mut |progress| {
                metrics.current_item = Some(progress.processed_segments as u64);
                metrics.total_items = Some(progress.total_segments as u64);
                metrics.match_percent = Some(progress.match_percent);
                context.set_metrics(metrics.clone(), 0);
                let percent = if progress.total_segments == 0 {
                    0
                } else {
                    progress
                        .processed_segments
                        .saturating_mul(100)
                        .checked_div(progress.total_segments)
                        .unwrap_or(0)
                        .min(100) as u8
                };
                context.set_stage_percent(percent, 0)
            },
        );
        match result {
            Ok(_) => {}
            Err(error) if cancellation.is_requested() => {
                return Err(StageRunError::cancelled(error, self.elapsed_millis()));
            }
            Err(error) => return Err(StageRunError::failed(error, self.elapsed_millis())),
        }
        context
            .set_stage_percent(100, self.elapsed_millis())
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;

        Ok(StageRunOutput::new(
            StageArtifacts::Align {
                alignment: PathBuf::from("alignment.json"),
            },
            stage_started.elapsed().as_secs(),
            self.elapsed_millis(),
        ))
    }

    fn run_review_audio(
        &mut self,
        context: &mut StageRunContext<'_>,
    ) -> Result<StageRunOutput, StageRunError> {
        let stage_started = Instant::now();
        let alignment_path = context
            .workspace()
            .stage_dir(PipelineStage::Align)
            .join("alignment.json");
        let corpus_path = context
            .workspace()
            .stage_dir(PipelineStage::Analyze)
            .join("book-corpus.json");
        let stage_dir = context.workspace().stage_dir(PipelineStage::ReviewAudio);
        reset_stage_dir(&stage_dir, PipelineStage::ReviewAudio)
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        let report_path = stage_dir.join("review.json");
        let draft_path = context.workspace().root().join("review-draft.json");
        let policy = context.job().settings.audio_review_policy;

        context
            .set_activity(
                "Checking alignment for unmatched audio",
                self.elapsed_millis(),
            )
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        let summary = create_audio_review_report_with_draft(
            &alignment_path,
            &report_path,
            Some(&draft_path),
            policy,
        )
        .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        let mut graphic_assignments = 0usize;
        if summary.unmatched_segments > 0 {
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
                            Some(AudioReviewEdge::Introduction) => {
                                "Checking leading unmatched audio evidence"
                            }
                            Some(AudioReviewEdge::Credits) => {
                                "Checking trailing unmatched audio evidence"
                            }
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
                    Ok(evidence) => {
                        set_audio_review_silence_evidence(&report_path, &item.id, evidence)
                            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?
                    }
                    Err(error) if cancellation.is_requested() => {
                        return Err(StageRunError::cancelled(error, self.elapsed_millis()));
                    }
                    Err(_) => {
                        // Silence evidence is advisory. A failed probe must not erase or auto-resolve review work.
                    }
                }
            }

            if policy == AudioReviewPolicy::Smart {
                context
                    .set_activity(
                        "Checking pending audio against nearby EPUB graphics",
                        self.elapsed_millis(),
                    )
                    .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
                let tesseract = resolve_optional_executable("STORYTELLER_TESSERACT", "tesseract");
                graphic_assignments = match apply_smart_graphic_readouts(
                    prepared.epub(),
                    &alignment_path,
                    &corpus_path,
                    &report_path,
                    &draft_path,
                    policy,
                    tesseract.as_deref(),
                    &cancellation,
                ) {
                    Ok(assigned) => assigned,
                    Err(error) if cancellation.is_requested() => {
                        return Err(StageRunError::cancelled(error, self.elapsed_millis()));
                    }
                    Err(error) => {
                        return Err(StageRunError::failed(error, self.elapsed_millis()));
                    }
                };
            }
        }
        let final_report = read_audio_review_report(&report_path)
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        let pending_segments = final_report.pending_count();
        context.set_metrics(
            LiveMetrics {
                match_percent: Some(summary.match_percent),
                backend: Some("Storyteller audio review gate".into()),
                ..LiveMetrics::default()
            },
            self.elapsed_millis(),
        );
        context
            .set_stage_percent(100, self.elapsed_millis())
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;

        let output = StageRunOutput::new(
            StageArtifacts::ReviewAudio {
                report: PathBuf::from("review.json"),
            },
            stage_started.elapsed().as_secs(),
            self.elapsed_millis(),
        );
        if pending_segments == 0 {
            context
                .set_activity(
                    if summary.unmatched_segments == 0 {
                        "No unmatched audio segments require review".to_string()
                    } else if graphic_assignments > 0 {
                        format!(
                            "Smart review assigned {graphic_assignments} Graphic Readout segment{}",
                            if graphic_assignments == 1 { "" } else { "s" }
                        )
                    } else {
                        "Saved audio review decisions restored".to_string()
                    },
                    self.elapsed_millis(),
                )
                .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
            Ok(output)
        } else {
            context
                .set_activity(
                    format!(
                        "{} unmatched audio segment{} need review",
                        pending_segments,
                        if pending_segments == 1 { "" } else { "s" }
                    ),
                    self.elapsed_millis(),
                )
                .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
            Ok(output.requiring_review())
        }
    }

    fn run_encode(
        &mut self,
        context: &mut StageRunContext<'_>,
    ) -> Result<StageRunOutput, StageRunError> {
        let stage_started = Instant::now();
        let prepared = prepared_job_sources(context.job(), context.workspace())
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        let review_path = context
            .workspace()
            .stage_dir(PipelineStage::ReviewAudio)
            .join("review.json");
        let review = read_audio_review_report(&review_path)
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        if !review.is_complete() {
            return Err(StageRunError::failed(
                "Unmatched audio must be reviewed before encoding can continue.",
                self.elapsed_millis(),
            ));
        }

        let stage_dir = context.workspace().stage_dir(PipelineStage::Encode);
        let ffmpeg = resolve_executable("STORYTELLER_FFMPEG", "ffmpeg");
        let cancellation = context.cancellation_token();
        let encoding = context.job().settings.audio;
        let backend_label = match encoding.codec {
            AudioCodec::Copy => "Storyteller byte copy",
            AudioCodec::Opus | AudioCodec::Aac => "ffmpeg",
        };
        let activity = match encoding.codec {
            AudioCodec::Copy => "Copying audiobook without re-encoding",
            AudioCodec::Opus => "Encoding audiobook as Opus",
            AudioCodec::Aac => "Encoding audiobook as AAC",
        };
        let mut metrics = LiveMetrics {
            backend: Some(backend_label.into()),
            match_percent: Some(review.match_percent),
            ..LiveMetrics::default()
        };
        context.set_metrics(metrics.clone(), self.elapsed_millis());
        context
            .set_activity(activity, self.elapsed_millis())
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;

        let encoded = encode_audiobook(
            prepared.audiobook(),
            &stage_dir,
            encoding,
            &ffmpeg,
            &cancellation,
            &mut |progress| {
                metrics.processed_audio_seconds = Some(progress.processed_audio_seconds);
                context.set_metrics(metrics.clone(), 0);
                Ok(())
            },
        );
        let encoded = match encoded {
            Ok(encoded) => encoded,
            Err(error) if cancellation.is_requested() => {
                return Err(StageRunError::cancelled(error, self.elapsed_millis()));
            }
            Err(error) => return Err(StageRunError::failed(error, self.elapsed_millis())),
        };
        context
            .set_stage_percent(100, self.elapsed_millis())
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;

        Ok(StageRunOutput::new(
            StageArtifacts::Encode {
                audio: PathBuf::from(encoded.descriptor.file_name),
                descriptor: PathBuf::from("encoded-audio.json"),
            },
            stage_started.elapsed().as_secs(),
            self.elapsed_millis(),
        ))
    }

    fn run_build_epub(
        &mut self,
        context: &mut StageRunContext<'_>,
    ) -> Result<StageRunOutput, StageRunError> {
        let stage_started = Instant::now();
        let prepared = prepared_job_sources(context.job(), context.workspace())
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        let analyze_dir = context.workspace().stage_dir(PipelineStage::Analyze);
        let align_dir = context.workspace().stage_dir(PipelineStage::Align);
        let review_dir = context.workspace().stage_dir(PipelineStage::ReviewAudio);
        let encode_dir = context.workspace().stage_dir(PipelineStage::Encode);
        let stage_dir = context.workspace().stage_dir(PipelineStage::BuildEpub);
        reset_stage_dir(&stage_dir, PipelineStage::BuildEpub)
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        let candidate = stage_dir.join("readaloud.epub");
        let reviewed_alignment = stage_dir.join("effective-alignment.json");
        let cancellation = context.cancellation_token();

        context.set_metrics(
            LiveMetrics {
                backend: Some("Storyteller EPUB Media Overlay builder".into()),
                ..LiveMetrics::default()
            },
            self.elapsed_millis(),
        );
        context
            .set_activity(
                "Building synchronized EPUB Media Overlays",
                self.elapsed_millis(),
            )
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        materialize_reviewed_alignment(
            &align_dir.join("alignment.json"),
            &analyze_dir.join("book-corpus.json"),
            &review_dir.join("review.json"),
            &reviewed_alignment,
        )
        .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        let summary = build_readaloud_epub(
            prepared.epub(),
            &analyze_dir.join("book-corpus.json"),
            &reviewed_alignment,
            &review_dir.join("review.json"),
            &encode_dir.join("encoded-audio.json"),
            &encode_dir,
            &candidate,
            &cancellation,
        );
        let summary = match summary {
            Ok(summary) => summary,
            Err(error) if cancellation.is_requested() => {
                return Err(StageRunError::cancelled(error, self.elapsed_millis()));
            }
            Err(error) => return Err(StageRunError::failed(error, self.elapsed_millis())),
        };
        context.set_metrics(
            LiveMetrics {
                current_item: Some(summary.synchronized_segments as u64),
                total_items: Some(summary.synchronized_segments as u64),
                backend: Some("Storyteller EPUB Media Overlay builder".into()),
                ..LiveMetrics::default()
            },
            self.elapsed_millis(),
        );
        context
            .set_stage_percent(100, self.elapsed_millis())
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;

        Ok(StageRunOutput::new(
            StageArtifacts::BuildEpub {
                effective_alignment: PathBuf::from("effective-alignment.json"),
                candidate: PathBuf::from("readaloud.epub"),
            },
            stage_started.elapsed().as_secs(),
            self.elapsed_millis(),
        ))
    }

    fn run_validate(
        &mut self,
        context: &mut StageRunContext<'_>,
    ) -> Result<StageRunOutput, StageRunError> {
        let stage_started = Instant::now();
        self.validated_candidate = None;
        let candidate = context
            .workspace()
            .stage_dir(PipelineStage::BuildEpub)
            .join("readaloud.epub");
        let stage_dir = context.workspace().stage_dir(PipelineStage::Validate);
        reset_stage_dir(&stage_dir, PipelineStage::Validate)
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        let report_path = stage_dir.join("validation.json");
        let cancellation = context.cancellation_token();

        context.set_metrics(
            LiveMetrics {
                backend: Some("Storyteller EPUB structural validator".into()),
                ..LiveMetrics::default()
            },
            self.elapsed_millis(),
        );
        context
            .set_activity(
                "Auditing EPUB container and Media Overlays",
                self.elapsed_millis(),
            )
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        let validated = ValidatedEpub::validate(&candidate, &cancellation);
        let validated = match validated {
            Ok(validated) => validated,
            Err(error) if cancellation.is_requested() => {
                return Err(StageRunError::cancelled(error, self.elapsed_millis()));
            }
            Err(error) => return Err(StageRunError::failed(error, self.elapsed_millis())),
        };
        let summary = validated.summary();
        write_validation_report(&report_path, summary)
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        context.set_metrics(
            LiveMetrics {
                current_item: Some(summary.synchronized_segments as u64),
                total_items: Some(summary.synchronized_segments as u64),
                backend: Some("Storyteller EPUB structural validator".into()),
                ..LiveMetrics::default()
            },
            self.elapsed_millis(),
        );
        context
            .set_activity("EPUB validation passed", self.elapsed_millis())
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        context
            .set_stage_percent(100, self.elapsed_millis())
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;

        self.validated_candidate = Some(validated);
        Ok(StageRunOutput::new(
            StageArtifacts::Validate {
                report: PathBuf::from("validation.json"),
            },
            stage_started.elapsed().as_secs(),
            self.elapsed_millis(),
        ))
    }
}

impl PipelineBackend for LitePipelineBackend {
    fn plan_stage(&mut self, job: &Job, stage: PipelineStage) -> Result<StagePlan, String> {
        match stage {
            PipelineStage::Prepare => Ok(StagePlan::run(
                "Preparing source files",
                ResourceRequest::io_heavy(1),
                self.elapsed_millis(),
            )),
            PipelineStage::Analyze => Ok(StagePlan::run(
                "Analyzing book and audiobook",
                ResourceRequest::cpu_heavy(self.cpu_threads),
                self.elapsed_millis(),
            )),
            PipelineStage::Align => Ok(StagePlan::run(
                "Aligning transcript to book text",
                ResourceRequest::cpu_heavy(self.cpu_threads),
                self.elapsed_millis(),
            )),
            PipelineStage::ReviewAudio => Ok(StagePlan::run(
                "Reviewing unmatched audio",
                ResourceRequest::io_heavy(1),
                self.elapsed_millis(),
            )),
            PipelineStage::Encode => Ok(StagePlan::run(
                "Preparing output audiobook",
                match job.settings.audio.codec {
                    AudioCodec::Copy => ResourceRequest::io_heavy(1),
                    AudioCodec::Opus | AudioCodec::Aac => {
                        ResourceRequest::cpu_heavy(self.cpu_threads)
                    }
                },
                self.elapsed_millis(),
            )),
            PipelineStage::BuildEpub => Ok(StagePlan::run(
                "Building read-aloud EPUB",
                ResourceRequest::io_heavy(1),
                self.elapsed_millis(),
            )),
            PipelineStage::Validate => Ok(StagePlan::run(
                "Validating read-aloud EPUB",
                ResourceRequest::io_heavy(1),
                self.elapsed_millis(),
            )),
        }
    }

    fn run_stage(
        &mut self,
        context: &mut StageRunContext<'_>,
    ) -> Result<StageRunOutput, StageRunError> {
        match context.stage() {
            PipelineStage::Prepare => self.run_prepare(context),
            PipelineStage::Analyze => self.run_analyze(context),
            PipelineStage::Align => self.run_align(context),
            PipelineStage::ReviewAudio => self.run_review_audio(context),
            PipelineStage::Encode => self.run_encode(context),
            PipelineStage::BuildEpub => self.run_build_epub(context),
            PipelineStage::Validate => self.run_validate(context),
        }
    }

    fn finalize_stage(
        &mut self,
        context: &mut StageRunContext<'_>,
        _output: &StageRunOutput,
    ) -> Result<(), StageRunError> {
        if context.stage() != PipelineStage::Validate {
            return Ok(());
        }

        let candidate = self.validated_candidate.take().ok_or_else(|| {
            StageRunError::failed(
                "Publication requires this attempt's validated EPUB candidate.",
                self.elapsed_millis(),
            )
        })?;
        let output_path = context.job().inputs.output_path.clone();
        let intent_path = context.workspace().root().join("publication.json");
        let cancellation = context.cancellation_token();
        context
            .set_activity(
                "Publishing validated read-aloud EPUB",
                self.elapsed_millis(),
            )
            .map_err(|error| StageRunError::failed(error, self.elapsed_millis()))?;
        match publish_validated_epub(&candidate, &output_path, &intent_path, &cancellation) {
            Ok(()) => Ok(()),
            Err(error) if cancellation.is_requested() => {
                Err(StageRunError::cancelled(error, self.elapsed_millis()))
            }
            Err(error) => Err(StageRunError::failed(error, self.elapsed_millis())),
        }
    }
}

#[derive(Debug, Clone)]
struct AnalyzeRuntime {
    ffmpeg: PathBuf,
    engine: crate::TranscriptionEngine,
    model_name: String,
}

impl AnalyzeRuntime {
    fn discover(job: &Job) -> Result<Self, String> {
        storyteller_core::validate_whistle_language(
            job.settings
                .language
                .as_deref()
                .unwrap_or(storyteller_core::WHISTLE_LANGUAGE),
        )?;
        let runtime = crate::detect_runtime();
        let engine = match job.settings.transcription_backend {
            storyteller_core::TranscriptionBackend::Whistle => {
                crate::TranscriptionEngine::Whistle {
                    executable: runtime.whistle_cli.ok_or(
                        "Whistle engine is missing. Open Settings and download dependencies.",
                    )?,
                    model: runtime.whistle_model.ok_or(
                        "Whistle model is missing. Open Settings and download dependencies.",
                    )?,
                }
            }
            storyteller_core::TranscriptionBackend::WhisperCuda => {
                if !runtime.whisper.hardware_ready() {
                    return Err(runtime.whisper.gpu_text);
                }
                crate::TranscriptionEngine::WhisperCuda {
                    executable: runtime.whisper.cli.ok_or("Whisper CUDA engine is missing. Select Whisper in Settings and download its tools.")?,
                    model: runtime.whisper.model.ok_or("Whisper Turbo model is missing. Select Whisper in Settings and download its tools.")?,
                }
            }
        };
        Ok(Self {
            ffmpeg: runtime
                .ffmpeg
                .ok_or("FFmpeg is missing. Open Settings and download dependencies.")?,
            engine,
            model_name: match job.settings.transcription_backend {
                storyteller_core::TranscriptionBackend::Whistle => "Whistle",
                storyteller_core::TranscriptionBackend::WhisperCuda => "Turbo Q5",
            }
            .into(),
        })
    }
}

pub fn spawn_job_worker(job: Job) -> Result<PipelineWorkerHandle, String> {
    storyteller_core::validate_whistle_language(
        job.settings
            .language
            .as_deref()
            .unwrap_or(storyteller_core::WHISTLE_LANGUAGE),
    )?;
    let logical_cpu_threads = std::thread::available_parallelism()
        .map(|threads| threads.get())
        .unwrap_or(1);
    let scheduler = ResourceScheduler::automatic(&HardwareProfile {
        logical_cpu_threads,
        memory_gib: None,
        gpu_backend: None,
        gpu_vram_mib: None,
    })?;
    let mut runtime = RuntimeCoordinator::new(scheduler);
    runtime.register_job(job.id)?;
    let workspace = JobWorkspace::for_job(workspace_base(), job.id);

    spawn_pipeline_worker_with_runtime_preflight(
        job,
        workspace,
        runtime,
        |job, _| Ok(pipeline_environment(job)),
        LitePipelineBackend::new(logical_cpu_threads),
    )
}

pub fn job_workspace(job: &Job) -> JobWorkspace {
    JobWorkspace::for_job(workspace_base(), job.id)
}

fn pipeline_environment(job: &Job) -> PipelineEnvironment {
    let whistle_cli = resolve_executable("STORYTELLER_WHISTLE", "needle");
    let ffmpeg = resolve_executable("STORYTELLER_FFMPEG", "ffmpeg");
    let tesseract = resolve_optional_executable("STORYTELLER_TESSERACT", "tesseract");
    let model_name = job.settings.transcription_model.trim();
    let effective_transcription_model = if model_name.is_empty() {
        String::new()
    } else if job.settings.transcription_backend
        == storyteller_core::TranscriptionBackend::WhisperCuda
    {
        // Analyze accepts only this content hash. Cached, sealed output can be resumed
        // without reinstalling the model when transcription is already complete.
        format!(
            "whisper-model:sha256:{}",
            crate::whisper_runtime::WHISPER_MODEL_SHA256
        )
    } else {
        match resolve_whistle_model() {
            Ok(path) => content_identity("whistle-model", &path),
            Err(_) => format!("requested:{model_name}"),
        }
    };
    let audio_backend = match job.settings.audio.codec {
        AudioCodec::Copy => "storyteller:cancellable-file-copy-v1".into(),
        AudioCodec::Opus | AudioCodec::Aac => file_identity("ffmpeg", &ffmpeg),
    };
    let ffmpeg_identity = file_identity("ffmpeg-analyze", &ffmpeg);
    let whistle_identity = content_identity("whistle-cli", &whistle_cli);
    let transcription_backend = if job.settings.transcription_backend
        == storyteller_core::TranscriptionBackend::Whistle
    {
        format!("storyteller:chunked-whistle-v3-english-context-words|{ffmpeg_identity}|{whistle_identity}")
    } else {
        // The exact bundle and model are verified before Analyze. Never reuse Whistle output.
        format!(
            "storyteller:chunked-whisper-cuda-v2-english-context-words|{ffmpeg_identity}|{}|{}",
            crate::whisper_runtime::WHISPER_RELEASE,
            crate::whisper_runtime::WHISPER_ARCHIVE_SHA256
        )
    };
    let ocr_backend = match tesseract {
        Some(path) => format!(
            "storyteller:image-evidence-v1|{}",
            file_identity("tesseract", &path)
        ),
        None => "storyteller:image-evidence-v1|tesseract:unavailable".into(),
    };

    PipelineEnvironment {
        transcription_backend,
        alignment_backend: "storyteller:monotonic-ngram-edit-v2-block-safe".into(),
        audio_backend,
        ocr_backend,
        epub_backend: "storyteller:epub-media-overlay-v2-supplemental-edge".into(),
        effective_language: storyteller_core::WHISTLE_LANGUAGE.into(),
        effective_transcription_model,
    }
}

const REVIEW_SILENCE_THRESHOLD_DB: i16 = -38;
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
            end_ms
                .min(duration_ms)
                .saturating_sub(start_ms.min(duration_ms))
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

fn resolve_executable(environment_variable: &str, base_name: &str) -> PathBuf {
    if let Some(value) = env::var_os(environment_variable).filter(|value| !value.is_empty()) {
        return PathBuf::from(value);
    }

    let file_name = executable_file_name(base_name);
    if let Some(executable_dir) = current_executable_dir() {
        for candidate in [
            executable_dir.join("tools").join(&file_name),
            executable_dir.join(&file_name),
        ] {
            if candidate.is_file() {
                return candidate;
            }
        }
    }

    PathBuf::from(base_name)
}

fn resolve_optional_executable(environment_variable: &str, base_name: &str) -> Option<PathBuf> {
    if let Some(value) = env::var_os(environment_variable).filter(|value| !value.is_empty()) {
        let configured = PathBuf::from(value);
        return configured.is_file().then_some(configured);
    }

    let file_name = executable_file_name(base_name);
    if let Some(executable_dir) = current_executable_dir() {
        for candidate in [
            executable_dir.join("tools").join(&file_name),
            executable_dir.join(&file_name),
        ] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    if let Some(path) = env::var_os("PATH") {
        for directory in env::split_paths(&path) {
            let candidate = directory.join(&file_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn resolve_whistle_model() -> Result<PathBuf, String> {
    crate::detect_runtime()
        .whistle_model
        .ok_or_else(|| "Whistle model is unavailable.".into())
}

fn content_identity(label: &str, path: &Path) -> String {
    let cancellation = storyteller_core::CancellationToken::default();
    match storyteller_core::fingerprint_source_file(path, &cancellation, label) {
        Ok(fingerprint) => format!("{label}:{fingerprint}"),
        Err(_) => format!("{label}:unavailable:{}", path.display()),
    }
}

fn current_executable_dir() -> Option<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

fn executable_file_name(base_name: &str) -> String {
    if cfg!(windows) {
        format!("{base_name}.exe")
    } else {
        base_name.to_string()
    }
}

fn file_identity(label: &str, path: &Path) -> String {
    let Ok(metadata) = fs::metadata(path) else {
        return format!("{label}:{}", path.display());
    };
    if !metadata.is_file() {
        return format!("{label}:{}", path.display());
    }
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_secs())
        .unwrap_or(0);
    format!("{label}:{}:{}:{modified}", path.display(), metadata.len())
}

fn reset_stage_dir(stage_dir: &Path, stage: PipelineStage) -> Result<(), String> {
    if stage_dir.exists() {
        fs::remove_dir_all(stage_dir).map_err(|error| {
            format!(
                "Could not reset {} workspace {}: {error}",
                stage.label(),
                stage_dir.display()
            )
        })?;
    }
    fs::create_dir_all(stage_dir).map_err(|error| {
        format!(
            "Could not create {} workspace {}: {error}",
            stage.label(),
            stage_dir.display()
        )
    })
}

fn validate_nonempty_file(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("{label} is unavailable at {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(format!(
            "{label} is not a non-empty regular file: {}",
            path.display()
        ));
    }
    Ok(())
}

fn workspace_base() -> PathBuf {
    crate::recovery_app_root().join("jobs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovered_foreign_job_fails_before_runtime_or_worker_start() {
        let mut job = Job::new(
            storyteller_core::JobInputs {
                title: "Recovered foreign request".into(),
                epub_path: "missing.epub".into(),
                audiobook_path: "missing.m4b".into(),
                output_path: "output.epub".into(),
            },
            storyteller_core::JobSettings::default(),
        )
        .unwrap();
        // Recovery retains the original request rather than silently changing it.
        job.settings.language = Some("fr".into());
        assert!(AnalyzeRuntime::discover(&job)
            .unwrap_err()
            .contains("English-only"));
        let error = spawn_job_worker(job)
            .err()
            .expect("foreign request must fail");
        assert!(error.contains("English-only"));
        assert!(error.contains("fr"));
    }
}
