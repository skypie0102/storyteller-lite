use std::{
    env,
    error::Error,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

use storyteller_core::{
    build_readaloud_epub, validate_readaloud_epub, AlignmentDocument, AlignmentSegment,
    AlignmentStatus, AudioReviewClassification, AudioReviewDecision, AudioReviewDecisionSource,
    AudioReviewDestination, AudioReviewItem, AudioReviewReport, AudioReviewSupplementalPlacement,
    CancellationToken, CorpusPosition, EncodedAudioDescriptor, EpubCorpus, EpubSection,
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

const SILENT_MP3: &[u8] = &[
    0xff, 0xfb, 0x10, 0xc4, 0x00, 0x03, 0xc0, 0x00, 0x01, 0xa4, 0x00, 0x00, 0x00, 0x20,
    0x00, 0x00, 0x34, 0x80, 0x00, 0x00, 0x04, 0x4c, 0x41, 0x4d, 0x45, 0x33, 0x2e, 0x31,
    0x30, 0x30, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0xff, 0xfb, 0x12, 0xc4, 0x29, 0x83, 0xc0, 0x00,
    0x01, 0xa4, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x34, 0x80, 0x00, 0x00, 0x04, 0x55,
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
    0xff, 0xfb, 0x10, 0xc4, 0x53, 0x83, 0xc0, 0x00, 0x01, 0xa4, 0x00, 0x00, 0x00, 0x20,
    0x00, 0x00, 0x34, 0x80, 0x00, 0x00, 0x04, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
    0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55,
    0x55, 0x55, 0x55,
];

const ONE_PIXEL_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
    0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x04, 0x00, 0x00,
    0x00, 0xb5, 0x1c, 0x0c, 0x02, 0x00, 0x00, 0x00, 0x0b, 0x49, 0x44, 0x41, 0x54, 0x78,
    0xda, 0x63, 0x64, 0xf8, 0x0f, 0x00, 0x01, 0x05, 0x01, 0x01, 0x27, 0x18, 0xe3, 0x66,
    0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn main() -> Result<(), Box<dyn Error>> {
    let output_dir = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/epubcheck-fixtures"));

    if output_dir.exists() {
        fs::remove_dir_all(&output_dir)?;
    }
    fs::create_dir_all(&output_dir)?;

    export_text_fixture(&output_dir)?;
    export_supplemental_fixture(&output_dir)?;
    export_graphic_fixture(&output_dir)?;

    println!("EPUBCheck fixtures written to {}", output_dir.display());
    Ok(())
}

fn export_text_fixture(output_dir: &Path) -> Result<(), Box<dyn Error>> {
    let alignment = AlignmentDocument {
        algorithm: "epubcheck-fixture".into(),
        language: Some("en".into()),
        total_segments: 2,
        matched_segments: 2,
        match_percent: 100.0,
        segments: vec![
            matched_segment(0, 10, "One line.", 0, 0, 9),
            matched_segment(10, 20, "Second line.", 1, 10, 22),
        ],
    };
    let review = AudioReviewReport {
        total_segments: 2,
        matched_segments: 2,
        match_percent: 100.0,
        unmatched: vec![],
        accepted_unmatched_exclusion: false,
    };
    build_fixture(output_dir, "text-overlay", alignment, review)
}

fn export_supplemental_fixture(output_dir: &Path) -> Result<(), Box<dyn Error>> {
    let alignment = AlignmentDocument {
        algorithm: "epubcheck-fixture".into(),
        language: Some("en".into()),
        total_segments: 3,
        matched_segments: 1,
        match_percent: 100.0 / 3.0,
        segments: vec![
            unmatched_segment(0, 5, "Opening narration."),
            matched_segment(5, 15, "One line.", 0, 0, 9),
            unmatched_segment(15, 20, "Closing credits."),
        ],
    };

    let review = AudioReviewReport {
        total_segments: 3,
        matched_segments: 1,
        match_percent: 100.0 / 3.0,
        unmatched: vec![
            AudioReviewItem {
                id: "intro".into(),
                alignment_index: 0,
                audio_start_ms: 0,
                audio_end_ms: 5,
                transcript_text: "Opening narration.".into(),
                suggestion: None,
                edge: None,
                silence: None,
                decision: AudioReviewDecision::Assigned {
                    destination: AudioReviewDestination {
                        href: "OPS/chapter.xhtml".into(),
                        line_index: None,
                        image_href: None,
                        supplemental: Some(AudioReviewSupplementalPlacement::BeforeAnchor),
                    },
                    classification: Some(AudioReviewClassification::Introduction),
                    source: AudioReviewDecisionSource::Manual,
                },
            },
            AudioReviewItem {
                id: "credits".into(),
                alignment_index: 2,
                audio_start_ms: 15,
                audio_end_ms: 20,
                transcript_text: "Closing credits.".into(),
                suggestion: None,
                edge: None,
                silence: None,
                decision: AudioReviewDecision::Assigned {
                    destination: AudioReviewDestination {
                        href: "OPS/chapter.xhtml".into(),
                        line_index: None,
                        image_href: None,
                        supplemental: Some(AudioReviewSupplementalPlacement::AfterAnchor),
                    },
                    classification: Some(AudioReviewClassification::Credits),
                    source: AudioReviewDecisionSource::Manual,
                },
            },
        ],
        accepted_unmatched_exclusion: false,
    };

    build_fixture(output_dir, "supplemental-edges", alignment, review)
}

fn export_graphic_fixture(output_dir: &Path) -> Result<(), Box<dyn Error>> {
    let alignment = AlignmentDocument {
        algorithm: "epubcheck-fixture".into(),
        language: Some("en".into()),
        total_segments: 2,
        matched_segments: 1,
        match_percent: 50.0,
        segments: vec![
            matched_segment(0, 10, "One line.", 0, 0, 9),
            unmatched_segment(10, 20, "A small diagram."),
        ],
    };

    let review = AudioReviewReport {
        total_segments: 2,
        matched_segments: 1,
        match_percent: 50.0,
        unmatched: vec![AudioReviewItem {
            id: "graphic".into(),
            alignment_index: 1,
            audio_start_ms: 10,
            audio_end_ms: 20,
            transcript_text: "A small diagram.".into(),
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
                source: AudioReviewDecisionSource::Manual,
            },
        }],
        accepted_unmatched_exclusion: false,
    };

    build_fixture(output_dir, "graphic-readout", alignment, review)
}

fn build_fixture(
    output_dir: &Path,
    name: &str,
    alignment: AlignmentDocument,
    review: AudioReviewReport,
) -> Result<(), Box<dyn Error>> {
    let work = output_dir.join(format!("{name}-work"));
    fs::create_dir_all(&work)?;

    let source = work.join("source.epub");
    write_source_epub(&source)?;

    let corpus_path = work.join("book-corpus.json");
    let corpus = EpubCorpus {
        package_path: "OPS/package.opf".into(),
        sections: vec![EpubSection {
            href: "OPS/chapter.xhtml".into(),
            text: "One line.\nSecond line.".into(),
        }],
    };
    fs::write(&corpus_path, serde_json::to_vec_pretty(&corpus)?)?;

    let alignment_path = work.join("alignment.json");
    fs::write(&alignment_path, serde_json::to_vec_pretty(&alignment)?)?;

    let review_path = work.join("review.json");
    fs::write(&review_path, serde_json::to_vec_pretty(&review)?)?;

    let encode_dir = work.join("encode");
    fs::create_dir_all(&encode_dir)?;
    fs::write(encode_dir.join("audio.mp3"), SILENT_MP3)?;
    let descriptor_path = encode_dir.join("encoded-audio.json");
    let descriptor = EncodedAudioDescriptor {
        file_name: "audio.mp3".into(),
        media_type: "audio/mpeg".into(),
        codec: "copy".into(),
        bitrate_kbps: Some(32),
    };
    fs::write(&descriptor_path, serde_json::to_vec_pretty(&descriptor)?)?;

    let output = output_dir.join(format!("{name}.epub"));
    let cancellation = CancellationToken::default();
    build_readaloud_epub(
        &source,
        &corpus_path,
        &alignment_path,
        &review_path,
        &descriptor_path,
        &encode_dir,
        &output,
        &cancellation,
    )?;
    validate_readaloud_epub(&output, &cancellation)?;

    fs::remove_dir_all(work)?;
    println!("wrote {}", output.display());
    Ok(())
}

fn write_source_epub(path: &Path) -> Result<(), Box<dyn Error>> {
    let file = File::create(path)?;
    let mut zip = ZipWriter::new(file);
    zip.start_file(
        "mimetype",
        SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
    )?;
    zip.write_all(b"application/epub+zip")?;

    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("META-INF/container.xml", deflated)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0">
  <rootfiles>
    <rootfile full-path="OPS/package.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
    )?;

    zip.start_file("OPS/package.opf", deflated)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="book-id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="book-id">urn:storyteller:epubcheck-fixture</dc:identifier>
    <dc:title>Storyteller EPUBCheck Fixture</dc:title>
    <dc:language>en</dc:language>
    <meta property="dcterms:modified">2026-09-16T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="c1" href="chapter.xhtml" media-type="application/xhtml+xml"/>
    <item id="diagram" href="diagram.png" media-type="image/png"/>
  </manifest>
  <spine>
    <itemref idref="c1"/>
  </spine>
</package>"#,
    )?;

    zip.start_file("OPS/nav.xhtml", deflated)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="en" xml:lang="en">
  <head><title>Navigation</title></head>
  <body>
    <nav epub:type="toc" id="toc">
      <h1>Contents</h1>
      <ol><li><a href="chapter.xhtml">Chapter</a></li></ol>
    </nav>
  </body>
</html>"#,
    )?;

    zip.start_file("OPS/chapter.xhtml", deflated)?;
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" lang="en" xml:lang="en">
  <head><title>Chapter</title></head>
  <body>
    <p>One line.</p>
    <p>Second line.</p>
    <figure><img src="diagram.png" alt="A small diagram."/></figure>
  </body>
</html>"#,
    )?;

    zip.start_file("OPS/diagram.png", deflated)?;
    zip.write_all(ONE_PIXEL_PNG)?;

    zip.finish()?.sync_all()?;
    Ok(())
}

fn matched_segment(
    audio_start_ms: u64,
    audio_end_ms: u64,
    transcript_text: &str,
    line_index: usize,
    char_start: usize,
    char_end: usize,
) -> AlignmentSegment {
    AlignmentSegment {
        audio_start_ms,
        audio_end_ms,
        transcript_text: transcript_text.into(),
        status: AlignmentStatus::Matched,
        match_percent: Some(100.0),
        book_start: Some(CorpusPosition {
            href: "OPS/chapter.xhtml".into(),
            line_index,
            char_offset: char_start,
        }),
        book_end: Some(CorpusPosition {
            href: "OPS/chapter.xhtml".into(),
            line_index,
            char_offset: char_end,
        }),
    }
}

fn unmatched_segment(
    audio_start_ms: u64,
    audio_end_ms: u64,
    transcript_text: &str,
) -> AlignmentSegment {
    AlignmentSegment {
        audio_start_ms,
        audio_end_ms,
        transcript_text: transcript_text.into(),
        status: AlignmentStatus::Unmatched,
        match_percent: None,
        book_start: None,
        book_end: None,
    }
}
