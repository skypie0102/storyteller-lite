use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use storyteller_core::{
    build_readaloud_epub, publish_validated_epub, validate_readaloud_epub, AlignmentDocument,
    AlignmentSegment, AlignmentStatus, AudioReviewReport, CancellationToken, CorpusPosition,
    EncodedAudioDescriptor, EpubCorpus, EpubSection,
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
    publish_validated_epub(&candidate, &published, &cancellation).unwrap();
    assert!(published.is_file());
    assert!(publish_validated_epub(&candidate, &published, &cancellation).is_err());

    let _ = fs::remove_dir_all(root);
}
