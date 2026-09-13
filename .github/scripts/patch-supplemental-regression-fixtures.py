from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)


path = Path("crates/storyteller-core/tests/epub_pipeline.rs")
text = path.read_text(encoding="utf-8")

text = replace_once(
    text,
    '''use storyteller_core::{
    build_readaloud_epub, publish_validated_epub, validate_readaloud_epub, AlignmentDocument,
    AlignmentSegment, AlignmentStatus, AudioReviewReport, CancellationToken, CorpusPosition,
    EncodedAudioDescriptor, EpubCorpus, EpubSection,
};''',
    '''use storyteller_core::{
    apply_audio_review_decision, build_readaloud_epub, create_audio_review_report_with_draft,
    publish_validated_epub, read_audio_review_report, validate_readaloud_epub, AlignmentDocument,
    AlignmentSegment, AlignmentStatus, AudioReviewClassification, AudioReviewDecision,
    AudioReviewDecisionSource, AudioReviewDestination, AudioReviewPolicy, AudioReviewReport,
    AudioReviewSupplementalPlacement, CancellationToken, CorpusPosition, EncodedAudioDescriptor,
    EpubCorpus, EpubSection,
};''',
    "expanded integration imports",
)

append = r'''

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
'''

if not text.endswith("\n"):
    text += "\n"
text += append
path.write_text(text, encoding="utf-8", newline="\n")
