use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use storyteller_core::{
    assign_manual_graphic_readout, build_readaloud_epub, read_audio_review_report,
    validate_readaloud_epub, AlignmentDocument, AlignmentSegment, AlignmentStatus,
    AudioReviewClassification, AudioReviewDecision, AudioReviewDecisionSource,
    AudioReviewDestination, AudioReviewItem, AudioReviewReport, CancellationToken, CorpusPosition,
    EncodedAudioDescriptor, EpubCorpus, EpubSection,
};
use uuid::Uuid;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

fn temp_root() -> PathBuf {
    std::env::temp_dir().join(format!("storyteller-graphic-build-{}", Uuid::new_v4()))
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
        br#"<?xml version="1.0" encoding="UTF-8"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="book-id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="book-id">urn:graphic-test</dc:identifier><dc:title>Graphic Test</dc:title><dc:language>en</dc:language></metadata><manifest><item id="c1" href="chapter.xhtml" media-type="application/xhtml+xml"/><item id="diagram" href="diagram.png" media-type="image/png"/></manifest><spine><itemref idref="c1"/></spine></package>"#,
    )
    .unwrap();
    zip.start_file("OPS/chapter.xhtml", deflated).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>Chapter</title></head><body><p>One line.</p><figure><img src="diagram.png" alt="Family tree showing Alice Robert Clara and Daniel"/></figure></body></html>"#,
    )
    .unwrap();
    zip.start_file("OPS/diagram.png", deflated).unwrap();
    zip.write_all(b"not-a-real-image-but-a-manifest-resource")
        .unwrap();
    zip.finish().unwrap().sync_all().unwrap();
}

fn write_inputs(root: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf, PathBuf) {
    let source = root.join("source.epub");
    write_source_epub(&source);

    let corpus_path = root.join("book-corpus.json");
    let corpus = EpubCorpus {
        package_path: "OPS/package.opf".into(),
        sections: vec![EpubSection {
            href: "OPS/chapter.xhtml".into(),
            text: "One line.".into(),
        }],
    };
    fs::write(&corpus_path, serde_json::to_vec_pretty(&corpus).unwrap()).unwrap();

    let alignment_path = root.join("alignment.json");
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
                audio_end_ms: 2200,
                transcript_text: "Family tree showing Alice Robert Clara and Daniel".into(),
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
    let review = AudioReviewReport {
        total_segments: 2,
        matched_segments: 1,
        match_percent: 50.0,
        unmatched: vec![AudioReviewItem {
            id: "graphic-segment".into(),
            alignment_index: 1,
            audio_start_ms: 1000,
            audio_end_ms: 2200,
            transcript_text: "Family tree showing Alice Robert Clara and Daniel".into(),
            suggestion: None,
            edge: None,
            silence: None,
            decision: AudioReviewDecision::Assigned {
                destination: AudioReviewDestination {
                    href: "OPS/chapter.xhtml".into(),
                    line_index: None,
                    image_href: Some("OPS/diagram.png".into()),
                    supplemental: None,
                },
                classification: Some(AudioReviewClassification::GraphicReadout),
                source: AudioReviewDecisionSource::Automatic,
            },
        }],
        accepted_unmatched_exclusion: false,
    };
    fs::write(&review_path, serde_json::to_vec_pretty(&review).unwrap()).unwrap();

    let encode_dir = root.join("encode");
    fs::create_dir_all(&encode_dir).unwrap();
    fs::write(encode_dir.join("audio.mp3"), b"fake-audio-bytes").unwrap();
    let descriptor_path = encode_dir.join("encoded-audio.json");
    let descriptor = EncodedAudioDescriptor {
        file_name: "audio.mp3".into(),
        media_type: "audio/mpeg".into(),
        codec: "copy".into(),
        bitrate_kbps: None,
    };
    fs::write(
        &descriptor_path,
        serde_json::to_vec_pretty(&descriptor).unwrap(),
    )
    .unwrap();

    (
        source,
        corpus_path,
        alignment_path,
        review_path,
        descriptor_path,
    )
}

#[test]
fn graphic_readout_shares_document_overlay_with_text_and_validates() {
    let root = temp_root();
    fs::create_dir_all(&root).unwrap();
    let (source, corpus_path, alignment_path, review_path, descriptor_path) = write_inputs(&root);
    let encode_dir = descriptor_path.parent().unwrap();
    let candidate = root.join("candidate.epub");
    let cancellation = CancellationToken::default();

    let build = build_readaloud_epub(
        &source,
        &corpus_path,
        &alignment_path,
        &review_path,
        &descriptor_path,
        encode_dir,
        &candidate,
        &cancellation,
    )
    .unwrap();
    assert_eq!(build.overlay_count, 1);
    assert_eq!(build.synchronized_segments, 2);
    assert_eq!(build.media_duration_ms, 2200);

    let validation = validate_readaloud_epub(&candidate, &cancellation).unwrap();
    assert_eq!(validation.overlay_count, 1);
    assert_eq!(validation.synchronized_segments, 2);
    assert_eq!(validation.media_duration_ms, 2200);

    let file = File::open(&candidate).unwrap();
    let mut zip = ZipArchive::new(file).unwrap();
    let package = {
        let mut entry = zip.by_name("OPS/package.opf").unwrap();
        let mut value = String::new();
        entry.read_to_string(&mut value).unwrap();
        value
    };
    assert_eq!(package.matches("media-overlay=").count(), 1);

    let chapter = {
        let mut entry = zip.by_name("OPS/chapter.xhtml").unwrap();
        let mut value = String::new();
        entry.read_to_string(&mut value).unwrap();
        value
    };
    assert!(chapter.contains("stl-mo-s1-b"));
    assert!(chapter.contains("stl-graphic-s1-r1"));
    assert!(chapter.contains("src=\"diagram.png\""));

    let smil = {
        let mut entry = zip
            .by_name("OPS/storyteller/overlays/overlay-0001.smil")
            .unwrap();
        let mut value = String::new();
        entry.read_to_string(&mut value).unwrap();
        value
    };
    let text_pos = smil.find("stl-mo-s1-b").unwrap();
    let graphic_pos = smil.find("stl-graphic-s1-r1").unwrap();
    assert!(text_pos < graphic_pos);
    assert!(smil.contains("clipBegin=\"0:00:01.000\""));
    assert!(smil.contains("clipEnd=\"0:00:02.200\""));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn manual_graphic_assignment_rediscovery_persists_and_builds() {
    let root = temp_root();
    fs::create_dir_all(&root).unwrap();
    let (source, corpus_path, alignment_path, review_path, descriptor_path) = write_inputs(&root);
    let cancellation = CancellationToken::default();

    let mut review = read_audio_review_report(&review_path).unwrap();
    review.unmatched[0].decision = AudioReviewDecision::Pending;
    fs::write(&review_path, serde_json::to_vec_pretty(&review).unwrap()).unwrap();
    let draft_path = root.join("review-draft.json");

    assign_manual_graphic_readout(
        &source,
        &alignment_path,
        &corpus_path,
        &review_path,
        &draft_path,
        "graphic-segment",
        "OPS/chapter.xhtml",
        "OPS/diagram.png",
        &cancellation,
    )
    .unwrap();

    let persisted = read_audio_review_report(&review_path).unwrap();
    let AudioReviewDecision::Assigned {
        destination,
        classification,
        source: decision_source,
    } = &persisted.unmatched[0].decision
    else {
        panic!("manual Graphic Readout assignment was not persisted");
    };
    assert_eq!(
        *classification,
        Some(AudioReviewClassification::GraphicReadout)
    );
    assert_eq!(*decision_source, AudioReviewDecisionSource::Manual);
    assert_eq!(destination.href, "OPS/chapter.xhtml");
    assert_eq!(destination.image_href.as_deref(), Some("OPS/diagram.png"));
    assert!(draft_path.is_file());

    let encode_dir = descriptor_path.parent().unwrap();
    let candidate = root.join("manual-candidate.epub");
    let build = build_readaloud_epub(
        &source,
        &corpus_path,
        &alignment_path,
        &review_path,
        &descriptor_path,
        encode_dir,
        &candidate,
        &cancellation,
    )
    .unwrap();
    assert_eq!(build.synchronized_segments, 2);

    let validation = validate_readaloud_epub(&candidate, &cancellation).unwrap();
    assert_eq!(validation.synchronized_segments, 2);

    let _ = fs::remove_dir_all(root);
}
