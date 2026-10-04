use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use storyteller_core::{
    apply_audio_review_decision, build_readaloud_epub, create_audio_review_report_with_draft,
    publish_validated_epub, read_audio_review_report, validate_readaloud_epub, AlignmentDocument,
    AlignmentSegment, AlignmentStatus, AudioReviewClassification, AudioReviewDecision,
    AudioReviewDecisionSource, AudioReviewDestination, AudioReviewPolicy, AudioReviewReport,
    AudioReviewSupplementalPlacement, CancellationToken, CorpusPosition, EncodedAudioDescriptor,
    EpubCorpus, EpubSection, ValidatedEpub,
};
use uuid::Uuid;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

fn temp_root() -> PathBuf {
    std::env::temp_dir().join(format!("storyteller-epub-pipeline-{}", Uuid::new_v4()))
}

fn write_source_epub(path: &Path) {
    let file = File::create(path).unwrap();
    let mut zip = ZipWriter::new(file);
    zip.start_file(
        "mimetype",
        SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
    )
    .unwrap();
    zip.write_all(b"application/epub+zip").unwrap();

    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file("META-INF/container.xml", deflated).unwrap();
    zip.write_all(
        br#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
    )
    .unwrap();
    zip.start_file("OPS/package.opf", deflated).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="book-id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="book-id">urn:test</dc:identifier><dc:title>Test Book</dc:title><dc:language>en</dc:language></metadata><manifest><item id="c1" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#,
    )
    .unwrap();
    zip.start_file("OPS/chapter.xhtml", deflated).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>Chapter</title></head><body><p>One line.</p><p>Second line.</p></body></html>"#,
    )
    .unwrap();
    zip.finish().unwrap().sync_all().unwrap();
}

#[test]
fn whistle_word_times_survive_chunk_merge_alignment_and_epub_validation() {
    use storyteller_core::{
        align_transcript_to_corpus, create_audio_review_report, extract_epub_corpus,
        merge_chunk_transcripts, parse_whistle_transcript, write_transcript, TranscriptionChunk,
    };
    let root = temp_root();
    fs::create_dir_all(&root).unwrap();
    let source = root.join("source.epub");
    write_source_epub(&source);
    let cancellation = CancellationToken::default();
    let corpus_path = root.join("corpus.json");
    extract_epub_corpus(&source, &corpus_path, &cancellation).unwrap();
    let first = parse_whistle_transcript(
        r#"{"text":"One line.","language":"en","words":[
        {"word":"One","start":0.1,"end":0.5,"probability":0.9},
        {"word":"line.","start":0.5,"end":1.2,"probability":0.9}] }"#,
        25_000,
    )
    .unwrap();
    let second = parse_whistle_transcript(
        r#"{"text":"Second line.","language":"en","words":[
        {"word":"Second","start":0.2,"end":0.8,"probability":0.9},
        {"word":"line.","start":0.8,"end":1.7,"probability":0.9}] }"#,
        25_000,
    )
    .unwrap();
    let transcript = merge_chunk_transcripts(
        50_000,
        &[
            (
                TranscriptionChunk {
                    index: 0,
                    start_ms: 0,
                    end_ms: 25_000,
                },
                first,
            ),
            (
                TranscriptionChunk {
                    index: 1,
                    start_ms: 25_000,
                    end_ms: 50_000,
                },
                second,
            ),
        ],
    )
    .unwrap();
    assert_eq!(transcript.segments[1].start_ms, 25_200);
    assert_eq!(transcript.segments[1].end_ms, 26_700);
    let transcript_path = root.join("transcript.json");
    write_transcript(&transcript_path, &transcript).unwrap();
    let alignment_path = root.join("alignment.json");
    let aligned = align_transcript_to_corpus(
        &corpus_path,
        &transcript_path,
        &alignment_path,
        &cancellation,
        &mut |_| Ok(()),
    )
    .unwrap();
    assert_eq!(aligned.matched_segments, 2);
    let review_path = root.join("review.json");
    create_audio_review_report(&alignment_path, &review_path).unwrap();
    let encode_dir = root.join("encode");
    fs::create_dir_all(&encode_dir).unwrap();
    // This regression validates EPUB structure and timing, not audio decoding.
    fs::write(encode_dir.join("audio.mp3"), b"fixture-audio").unwrap();
    let descriptor_path = encode_dir.join("encoded-audio.json");
    fs::write(
        &descriptor_path,
        serde_json::to_vec(&EncodedAudioDescriptor {
            file_name: "audio.mp3".into(),
            media_type: "audio/mpeg".into(),
            codec: "copy".into(),
            bitrate_kbps: None,
        })
        .unwrap(),
    )
    .unwrap();
    let candidate = root.join("candidate.epub");
    let build = build_readaloud_epub(
        &source,
        &corpus_path,
        &alignment_path,
        &review_path,
        &descriptor_path,
        &encode_dir,
        &candidate,
        &cancellation,
    )
    .unwrap();
    assert_eq!(build.synchronized_segments, 2);
    let validated = validate_readaloud_epub(&candidate, &cancellation).unwrap();
    assert_eq!(validated.synchronized_segments, 2);
    assert_eq!(validated.media_duration_ms, 2_600);
    let mut archive = ZipArchive::new(File::open(&candidate).unwrap()).unwrap();
    let mut smil = String::new();
    archive
        .by_name("OPS/storyteller/overlays/overlay-0001.smil")
        .unwrap()
        .read_to_string(&mut smil)
        .unwrap();
    assert!(smil.contains("clipBegin=\"0:00:25.200\""));
    assert!(smil.contains("clipEnd=\"0:00:26.700\""));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn build_validate_and_publish_media_overlay_epub() {
    let root = temp_root();
    fs::create_dir_all(&root).unwrap();
    let source = root.join("source.epub");
    write_source_epub(&source);

    let corpus_path = root.join("book-corpus.json");
    let corpus = EpubCorpus {
        package_path: "OPS/package.opf".into(),
        sections: vec![EpubSection {
            href: "OPS/chapter.xhtml".into(),
            text: "One line.\nSecond line.".into(),
        }],
    };
    fs::write(&corpus_path, serde_json::to_vec_pretty(&corpus).unwrap()).unwrap();

    let alignment_path = root.join("alignment.json");
    let alignment = AlignmentDocument {
        algorithm: "test".into(),
        language: Some("en".into()),
        total_segments: 2,
        matched_segments: 2,
        match_percent: 100.0,
        segments: vec![
            AlignmentSegment {
                audio_start_ms: 0,
                audio_end_ms: 1000,
                transcript_text: "One line.".into(),
                status: AlignmentStatus::Matched,
                match_percent: Some(100.0),
                book_start: Some(CorpusPosition {
                    href: "OPS/chapter.xhtml".into(),
                    line_index: 0,
                    char_offset: 0,
                }),
                book_end: Some(CorpusPosition {
                    href: "OPS/chapter.xhtml".into(),
                    line_index: 0,
                    char_offset: 9,
                }),
            },
            AlignmentSegment {
                audio_start_ms: 1000,
                audio_end_ms: 2500,
                transcript_text: "Second line.".into(),
                status: AlignmentStatus::Matched,
                match_percent: Some(100.0),
                book_start: Some(CorpusPosition {
                    href: "OPS/chapter.xhtml".into(),
                    line_index: 1,
                    char_offset: 10,
                }),
                book_end: Some(CorpusPosition {
                    href: "OPS/chapter.xhtml".into(),
                    line_index: 1,
                    char_offset: 22,
                }),
            },
        ],
    };
    fs::write(
        &alignment_path,
        serde_json::to_vec_pretty(&alignment).unwrap(),
    )
    .unwrap();

    let review_path = root.join("review.json");
    let review = AudioReviewReport {
        total_segments: 2,
        matched_segments: 2,
        match_percent: 100.0,
        unmatched: vec![],
        accepted_unmatched_exclusion: false,
    };
    fs::write(&review_path, serde_json::to_vec_pretty(&review).unwrap()).unwrap();

    let encode_dir = root.join("encode");
    fs::create_dir_all(&encode_dir).unwrap();
    fs::write(encode_dir.join("audio.mp3"), b"fake-audio-bytes").unwrap();
    let descriptor = EncodedAudioDescriptor {
        file_name: "audio.mp3".into(),
        media_type: "audio/mpeg".into(),
        codec: "copy".into(),
        bitrate_kbps: None,
    };
    let descriptor_path = encode_dir.join("encoded-audio.json");
    fs::write(
        &descriptor_path,
        serde_json::to_vec_pretty(&descriptor).unwrap(),
    )
    .unwrap();

    let candidate = root.join("candidate.epub");
    let cancellation = CancellationToken::default();
    let build = build_readaloud_epub(
        &source,
        &corpus_path,
        &alignment_path,
        &review_path,
        &descriptor_path,
        &encode_dir,
        &candidate,
        &cancellation,
    )
    .unwrap();
    assert_eq!(build.overlay_count, 1);
    assert_eq!(build.synchronized_segments, 2);
    assert_eq!(build.media_duration_ms, 2500);

    let validation = validate_readaloud_epub(&candidate, &cancellation).unwrap();
    assert_eq!(validation.overlay_count, 1);
    assert_eq!(validation.synchronized_segments, 2);
    assert_eq!(validation.media_duration_ms, 2500);

    let file = File::open(&candidate).unwrap();
    let mut zip = ZipArchive::new(file).unwrap();
    let first = zip.by_index(0).unwrap();
    assert_eq!(first.name(), "mimetype");
    assert_eq!(first.compression(), CompressionMethod::Stored);
    drop(first);

    let mut package = String::new();
    zip.by_name("OPS/package.opf")
        .unwrap()
        .read_to_string(&mut package)
        .unwrap();
    assert!(package.contains("media-overlay="));
    assert!(package.contains("media:duration"));

    let mut chapter = String::new();
    zip.by_name("OPS/chapter.xhtml")
        .unwrap()
        .read_to_string(&mut chapter)
        .unwrap();
    assert!(chapter.contains("stl-mo-s1-b"));
    assert!(zip
        .by_name("OPS/storyteller/overlays/overlay-0001.smil")
        .is_ok());
    assert!(zip.by_name("OPS/storyteller/audio/audio.mp3").is_ok());

    let published = root.join("Test Book (readaloud).epub");
    let validated = ValidatedEpub::validate(&candidate, &cancellation).unwrap();
    let intent = root.join("resume-workspace").join("publication.json");
    publish_validated_epub(&validated, &published, &intent, &cancellation).unwrap();
    assert!(published.is_file());
    // A matching job-owned intent plus matching bytes recovers a completed commit.
    publish_validated_epub(&validated, &published, &intent, &cancellation).unwrap();
    assert!(publish_validated_epub(
        &validated,
        &published,
        &root.join("unrelated-intent.json"),
        &cancellation
    )
    .is_err());
    assert_cached_publication_is_rechecked(&root, &published);
    fs::write(&candidate, b"modified after validation").unwrap();
    let refused = root.join("Refused.epub");
    assert!(publish_validated_epub(
        &validated,
        &refused,
        &root.join("refused-intent.json"),
        &cancellation
    )
    .unwrap_err()
    .contains("changed after validation"));
    assert!(!refused.exists());

    let _ = fs::remove_dir_all(root);
}

/// Exercise the worker resume path with a real built EPUB and synthetic earlier
/// stage files. Every checkpoint is present, including Validate, when it relaunches.
fn assert_cached_publication_is_rechecked(root: &Path, published: &Path) {
    use storyteller_core::{
        spawn_pipeline_worker, write_validation_report, AudioEncoding, HardwareProfile, Job,
        JobInputs, JobSettings, JobStatus, JobWorkspace, PipelineBackend, PipelineRunState,
        PipelineStage, ResourceRequest, ResourceScheduler, ResumeContext, RuntimeCoordinator,
        StageArtifacts, StagePlan, StageRunContext, StageRunError, StageRunOutput, StageStatus,
    };
    struct PublishBackend {
        validated: Option<ValidatedEpub>,
    }
    impl PipelineBackend for PublishBackend {
        fn plan_stage(&mut self, _job: &Job, stage: PipelineStage) -> Result<StagePlan, String> {
            assert_eq!(stage, PipelineStage::Validate);
            Ok(StagePlan::run(
                "Recheck publication",
                ResourceRequest::io_heavy(1),
                0,
            ))
        }
        fn run_stage(
            &mut self,
            context: &mut StageRunContext<'_>,
        ) -> Result<StageRunOutput, StageRunError> {
            let token = ValidatedEpub::validate(
                &context
                    .workspace()
                    .stage_dir(PipelineStage::BuildEpub)
                    .join("readaloud.epub"),
                &context.cancellation_token(),
            )
            .map_err(|error| StageRunError::failed(error, 0))?;
            write_validation_report(
                &context
                    .workspace()
                    .stage_dir(PipelineStage::Validate)
                    .join("validation.json"),
                token.summary(),
            )
            .map_err(|error| StageRunError::failed(error, 0))?;
            self.validated = Some(token);
            Ok(StageRunOutput::new(
                StageArtifacts::Validate {
                    report: "validation.json".into(),
                },
                1,
                1,
            ))
        }
        fn finalize_stage(
            &mut self,
            context: &mut StageRunContext<'_>,
            _output: &StageRunOutput,
        ) -> Result<(), StageRunError> {
            publish_validated_epub(
                self.validated.as_ref().unwrap(),
                &context.job().inputs.output_path,
                &context.workspace().root().join("publication.json"),
                &context.cancellation_token(),
            )
            .map_err(|error| StageRunError::failed(error, 0))
        }
    }
    let workspace = JobWorkspace::new(root.join("resume-workspace"));
    let mut job = Job::new(
        JobInputs {
            title: "Resume publication".into(),
            epub_path: root.join("source.epub"),
            audiobook_path: root.join("encode/audio.mp3"),
            output_path: published.to_owned(),
        },
        JobSettings {
            audio: AudioEncoding::copy(),
            ..JobSettings::default()
        },
    )
    .unwrap();
    job.start().unwrap();
    let resume = ResumeContext {
        epub_source: "fixture:epub".into(),
        audiobook_source: "fixture:audio".into(),
        transcription_backend: "fixture:whistle".into(),
        alignment_backend: "fixture:align".into(),
        audio_backend: "fixture:audio".into(),
        ocr_backend: "fixture:ocr".into(),
        epub_backend: "fixture:epub".into(),
        effective_language: "en".into(),
        effective_transcription_model: "whistle".into(),
        settings: job.settings.clone(),
    };
    let artifacts = [
        StageArtifacts::Prepare {
            epub: "source.epub".into(),
            audiobook: "audio.mp3".into(),
        },
        StageArtifacts::Analyze {
            corpus: "corpus.json".into(),
            plan: "plan.json".into(),
            transcript: "transcript.json".into(),
        },
        StageArtifacts::Align {
            alignment: "alignment.json".into(),
        },
        StageArtifacts::ReviewAudio {
            report: "review.json".into(),
        },
        StageArtifacts::Encode {
            audio: "audio.mp3".into(),
            descriptor: "descriptor.json".into(),
        },
        StageArtifacts::BuildEpub {
            candidate: "readaloud.epub".into(),
            effective_alignment: "effective.json".into(),
        },
        StageArtifacts::Validate {
            report: "validation.json".into(),
        },
    ];
    for output in artifacts {
        let stage = output.stage();
        let dir = workspace.stage_dir(stage);
        fs::create_dir_all(&dir).unwrap();
        for relative in output.paths() {
            if stage == PipelineStage::BuildEpub && relative == Path::new("readaloud.epub") {
                fs::copy(root.join("candidate.epub"), dir.join(relative)).unwrap();
            } else {
                fs::write(dir.join(relative), b"sealed earlier fixture stage").unwrap();
            }
        }
        workspace
            .capture_stage_artifacts(stage, &output, &CancellationToken::default())
            .unwrap();
        job.progress
            .start_stage(stage, "Fixture completed")
            .unwrap();
        job.progress.complete_stage(stage, 1).unwrap();
        job.checkpoint_completed_stage(stage, &resume).unwrap();
    }
    let run = || {
        let scheduler = ResourceScheduler::automatic(&HardwareProfile {
            logical_cpu_threads: 2,
            memory_gib: None,
            gpu_backend: None,
            gpu_vram_mib: None,
        })
        .unwrap();
        let mut runtime = RuntimeCoordinator::new(scheduler);
        runtime.register_job(job.id).unwrap();
        spawn_pipeline_worker(
            job.clone(),
            workspace.clone(),
            runtime,
            resume.clone(),
            PublishBackend { validated: None },
        )
        .unwrap()
        .join()
        .unwrap()
    };
    let before = fs::metadata(published).unwrap().modified().unwrap();
    let result = run();
    assert_eq!(result.run_result.unwrap(), PipelineRunState::Completed);
    assert_eq!(result.job.status, JobStatus::Completed);
    assert!(result.job.progress.stages()[..6]
        .iter()
        .all(|stage| stage.status == StageStatus::Cached));
    assert_eq!(
        result.job.progress.stages()[6].status,
        StageStatus::Completed
    );
    assert_eq!(fs::metadata(published).unwrap().modified().unwrap(), before);
    fs::remove_file(published).unwrap();
    assert_eq!(run().run_result.unwrap(), PipelineRunState::Completed);
    validate_readaloud_epub(published, &CancellationToken::default()).unwrap();
    fs::write(published, b"externally changed output").unwrap();
    assert!(matches!(
        run().run_result.unwrap(),
        PipelineRunState::Failed(_)
    ));
    assert_eq!(fs::read(published).unwrap(), b"externally changed output");
}

#[test]
fn smart_and_manual_supplemental_pages_preserve_spine_order_and_timing() {
    let root = temp_root();
    fs::create_dir_all(&root).unwrap();
    let source = root.join("source.epub");
    write_source_epub(&source);

    let corpus_path = root.join("book-corpus.json");
    let corpus = EpubCorpus {
        package_path: "OPS/package.opf".into(),
        sections: vec![EpubSection {
            href: "OPS/chapter.xhtml".into(),
            text: "One line.\nSecond line.".into(),
        }],
    };
    fs::write(&corpus_path, serde_json::to_vec_pretty(&corpus).unwrap()).unwrap();

    let alignment_path = root.join("alignment.json");
    let alignment = AlignmentDocument {
        algorithm: "test".into(),
        language: Some("en".into()),
        total_segments: 3,
        matched_segments: 1,
        match_percent: 100.0 / 3.0,
        segments: vec![
            AlignmentSegment {
                audio_start_ms: 0,
                audio_end_ms: 900,
                transcript_text: "Opening narration.".into(),
                status: AlignmentStatus::Unmatched,
                match_percent: None,
                book_start: None,
                book_end: None,
            },
            AlignmentSegment {
                audio_start_ms: 900,
                audio_end_ms: 2500,
                transcript_text: "One line.".into(),
                status: AlignmentStatus::Matched,
                match_percent: Some(100.0),
                book_start: Some(CorpusPosition {
                    href: "OPS/chapter.xhtml".into(),
                    line_index: 0,
                    char_offset: 0,
                }),
                book_end: Some(CorpusPosition {
                    href: "OPS/chapter.xhtml".into(),
                    line_index: 0,
                    char_offset: 9,
                }),
            },
            AlignmentSegment {
                audio_start_ms: 2500,
                audio_end_ms: 4000,
                transcript_text: "Closing credits.".into(),
                status: AlignmentStatus::Unmatched,
                match_percent: None,
                book_start: None,
                book_end: None,
            },
        ],
    };
    fs::write(
        &alignment_path,
        serde_json::to_vec_pretty(&alignment).unwrap(),
    )
    .unwrap();

    let review_path = root.join("review.json");
    let draft_path = root.join("review-draft.json");
    let summary = create_audio_review_report_with_draft(
        &alignment_path,
        &review_path,
        Some(&draft_path),
        AudioReviewPolicy::Smart,
    )
    .unwrap();
    assert_eq!(summary.unmatched_segments, 2);
    assert_eq!(summary.pending_segments, 0);

    let initial_review = read_audio_review_report(&review_path).unwrap();
    assert!(matches!(
        initial_review.unmatched[0].decision,
        AudioReviewDecision::Assigned {
            source: AudioReviewDecisionSource::Automatic,
            classification: Some(AudioReviewClassification::Introduction),
            ..
        }
    ));
    let credits_id = initial_review.unmatched[1].id.clone();
    apply_audio_review_decision(
        &review_path,
        &draft_path,
        &credits_id,
        AudioReviewDecision::Assigned {
            destination: AudioReviewDestination {
                href: "OPS/chapter.xhtml".into(),
                line_index: None,
                image_href: None,
                supplemental: Some(AudioReviewSupplementalPlacement::AfterAnchor),
            },
            classification: Some(AudioReviewClassification::Credits),
            source: AudioReviewDecisionSource::Manual,
        },
    )
    .unwrap();

    create_audio_review_report_with_draft(
        &alignment_path,
        &review_path,
        Some(&draft_path),
        AudioReviewPolicy::Smart,
    )
    .unwrap();
    let restored_review = read_audio_review_report(&review_path).unwrap();
    assert!(matches!(
        restored_review.unmatched[0].decision,
        AudioReviewDecision::Assigned {
            source: AudioReviewDecisionSource::Automatic,
            classification: Some(AudioReviewClassification::Introduction),
            ..
        }
    ));
    assert!(matches!(
        restored_review.unmatched[1].decision,
        AudioReviewDecision::Assigned {
            source: AudioReviewDecisionSource::Manual,
            classification: Some(AudioReviewClassification::Credits),
            ..
        }
    ));

    let encode_dir = root.join("encode");
    fs::create_dir_all(&encode_dir).unwrap();
    fs::write(encode_dir.join("audio.mp3"), b"fake-audio-bytes").unwrap();
    let descriptor = EncodedAudioDescriptor {
        file_name: "audio.mp3".into(),
        media_type: "audio/mpeg".into(),
        codec: "copy".into(),
        bitrate_kbps: None,
    };
    let descriptor_path = encode_dir.join("encoded-audio.json");
    fs::write(
        &descriptor_path,
        serde_json::to_vec_pretty(&descriptor).unwrap(),
    )
    .unwrap();

    let candidate = root.join("candidate.epub");
    let cancellation = CancellationToken::default();
    let build = build_readaloud_epub(
        &source,
        &corpus_path,
        &alignment_path,
        &review_path,
        &descriptor_path,
        &encode_dir,
        &candidate,
        &cancellation,
    )
    .unwrap();
    assert_eq!(build.overlay_count, 3);
    assert_eq!(build.synchronized_segments, 3);
    assert_eq!(build.media_duration_ms, 4000);

    let validation = validate_readaloud_epub(&candidate, &cancellation).unwrap();
    assert_eq!(validation.overlay_count, 3);
    assert_eq!(validation.synchronized_segments, 3);
    assert_eq!(validation.media_duration_ms, 4000);

    let file = File::open(&candidate).unwrap();
    let mut zip = ZipArchive::new(file).unwrap();
    let package = {
        let mut entry = zip.by_name("OPS/package.opf").unwrap();
        let mut value = String::new();
        entry.read_to_string(&mut value).unwrap();
        value
    };

    let intro_spine = package
        .find("idref=\"stl-extra-page-0001\"")
        .expect("Introduction must be inserted into the spine");
    let chapter_spine = package
        .find("idref=\"c1\"")
        .expect("Original chapter must remain in the spine");
    let credits_spine = package
        .find("idref=\"stl-extra-page-0002\"")
        .expect("Credits must be inserted into the spine");
    assert!(intro_spine < chapter_spine);
    assert!(chapter_spine < credits_spine);

    assert!(package.contains("href=\"storyteller/text/introduction-0001.xhtml\""));
    assert!(package.contains("href=\"storyteller/text/credits-0002.xhtml\""));
    assert!(package.contains("media-overlay=\"stl-extra-mo-0001\""));
    assert!(package.contains("media-overlay=\"stl-extra-mo-0002\""));
    assert!(package.contains("refines=\"#stl-extra-mo-0001\""));
    assert!(package.contains("refines=\"#stl-extra-mo-0002\""));
    assert!(package.contains("0:00:00.900"));
    assert!(package.contains("0:00:01.500"));
    assert!(package.contains("0:00:04.000"));

    let introduction = {
        let mut entry = zip
            .by_name("OPS/storyteller/text/introduction-0001.xhtml")
            .unwrap();
        let mut value = String::new();
        entry.read_to_string(&mut value).unwrap();
        value
    };
    assert!(introduction.contains("<title>Introduction</title>"));
    assert!(introduction.contains("Opening narration."));

    let credits = {
        let mut entry = zip
            .by_name("OPS/storyteller/text/credits-0002.xhtml")
            .unwrap();
        let mut value = String::new();
        entry.read_to_string(&mut value).unwrap();
        value
    };
    assert!(credits.contains("<title>Credits</title>"));
    assert!(credits.contains("Closing credits."));

    let introduction_smil = {
        let mut entry = zip
            .by_name("OPS/storyteller/overlays/introduction-0001.smil")
            .unwrap();
        let mut value = String::new();
        entry.read_to_string(&mut value).unwrap();
        value
    };
    assert!(introduction_smil.contains("epub:type=\"frontmatter\""));
    assert!(introduction_smil.contains("clipBegin=\"0:00:00.000\""));
    assert!(introduction_smil.contains("clipEnd=\"0:00:00.900\""));

    let credits_smil = {
        let mut entry = zip
            .by_name("OPS/storyteller/overlays/credits-0002.smil")
            .unwrap();
        let mut value = String::new();
        entry.read_to_string(&mut value).unwrap();
        value
    };
    assert!(credits_smil.contains("epub:type=\"backmatter\""));
    assert!(credits_smil.contains("clipBegin=\"0:00:02.500\""));
    assert!(credits_smil.contains("clipEnd=\"0:00:04.000\""));

    let _ = fs::remove_dir_all(root);
}
