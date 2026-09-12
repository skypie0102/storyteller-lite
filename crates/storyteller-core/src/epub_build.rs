use crate::{
    read_audio_review_report, read_encoded_audio_descriptor, read_epub_corpus, AlignmentDocument,
    AlignmentStatus, CancellationToken,
};
use quick_xml::{
    escape::unescape,
    events::{BytesEnd, BytesStart, BytesText, Event},
    Reader, Writer,
};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{Read, Seek, Write},
    path::Path,
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const COPY_BUFFER_BYTES: usize = 64 * 1024;
const MAX_XML_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpubBuildSummary {
    pub overlay_count: usize,
    pub synchronized_segments: usize,
    pub media_duration_ms: u64,
}

#[derive(Debug, Clone)]
struct OverlaySection {
    href: String,
    xhtml: String,
    smil_archive_path: String,
    smil_manifest_href: String,
    overlay_item_id: String,
    duration_ms: u64,
    synchronized_segments: usize,
}

#[derive(Debug, Clone)]
struct PackageScan {
    existing_ids: HashSet<String>,
    xhtml_item_ids: HashMap<String, String>,
}

pub fn build_readaloud_epub(
    source_epub: &Path,
    corpus_path: &Path,
    alignment_path: &Path,
    review_path: &Path,
    encoded_audio_descriptor_path: &Path,
    encoded_audio_dir: &Path,
    destination: &Path,
    cancellation: &CancellationToken,
) -> Result<EpubBuildSummary, String> {
    if cancellation.is_requested() {
        return Err("EPUB build was cancelled.".into());
    }
    let corpus = read_epub_corpus(corpus_path)?;
    let alignment = read_alignment(alignment_path)?;
    let review = read_audio_review_report(review_path)?;
    if !review.unmatched.is_empty() && !review.accepted_unmatched_exclusion {
        return Err("Unmatched audio must be reviewed before the EPUB can be built.".into());
    }
    let audio = read_encoded_audio_descriptor(encoded_audio_descriptor_path)?;
    let encoded_audio_path = encoded_audio_dir.join(&audio.file_name);
    validate_nonempty_file(&encoded_audio_path, "Encoded audiobook")?;

    let source = File::open(source_epub)
        .map_err(|error| format!("Could not open source EPUB {}: {error}", source_epub.display()))?;
    let mut archive = ZipArchive::new(source)
        .map_err(|error| format!("Could not read source EPUB ZIP container: {error}"))?;
    let archive_names = collect_archive_names(&mut archive)?;
    validate_mimetype(&mut archive)?;

    let package_path = corpus.package_path.clone();
    let package_xml = read_archive_text(&mut archive, &package_path, cancellation)?;
    let overlay_hrefs = matched_hrefs(&alignment)?;
    if overlay_hrefs.is_empty() {
        return Err("Alignment contains no matched segments to synchronize.".into());
    }
    let scan = scan_package(&package_xml, &package_path, &overlay_hrefs)?;
    let package_dir = parent_archive_path(&package_path);
    let resource_root = unique_resource_root(&package_dir, &archive_names);

    let mut used_ids = scan.existing_ids.clone();
    let audio_item_id = unique_id("stl-audio", &mut used_ids);
    let audio_archive_path = join_archive_path(
        &resource_root,
        &format!("audio/{}", audio.file_name),
    );
    let audio_manifest_href = relative_archive_path(&package_dir, &audio_archive_path);

    let mut sections = Vec::new();
    for (section_index, section) in corpus.sections.iter().enumerate() {
        if cancellation.is_requested() {
            return Err("EPUB build was cancelled.".into());
        }
        let segments = matched_segments_for_href(&alignment, &section.href);
        if segments.is_empty() {
            continue;
        }
        let source_xhtml = read_archive_text(&mut archive, &section.href, cancellation)?;
        let requested_lines = segments
            .iter()
            .map(|(_, segment)| {
                segment
                    .book_start
                    .as_ref()
                    .map(|position| position.line_index)
                    .ok_or("Matched alignment segment is missing its book start position.")
            })
            .collect::<Result<HashSet<_>, _>>()?;
        let (annotated_xhtml, anchors) = annotate_xhtml_blocks(
            &source_xhtml,
            &requested_lines,
            section_index,
        )?;
        let overlay_item_id = unique_id(&format!("stl-mo-{:04}", section_index + 1), &mut used_ids);
        let smil_archive_path = join_archive_path(
            &resource_root,
            &format!("overlays/overlay-{:04}.smil", section_index + 1),
        );
        let smil_manifest_href = relative_archive_path(&package_dir, &smil_archive_path);
        let smil_dir = parent_archive_path(&smil_archive_path);
        let text_href = uri_path(&relative_archive_path(&smil_dir, &section.href));
        let audio_href = uri_path(&relative_archive_path(&smil_dir, &audio_archive_path));
        let (smil, duration_ms) = build_smil(
            &segments,
            &anchors,
            &text_href,
            &audio_href,
            section_index,
        )?;
        sections.push(OverlaySection {
            href: section.href.clone(),
            xhtml: annotated_xhtml,
            smil_archive_path,
            smil_manifest_href,
            overlay_item_id,
            duration_ms,
            synchronized_segments: segments.len(),
        });
        sections.last_mut().expect("section was pushed").xhtml = annotated_xhtml;
        let last = sections.last().expect("section was pushed");
        if smil.is_empty() {
            return Err(format!("Generated Media Overlay is empty for {}.", last.href));
        }
        // Stash the SMIL beside the XHTML text in-memory using a sentinel separator. It is split
        // immediately before ZIP writing so OverlaySection remains a compact private structure.
        sections.last_mut().expect("section was pushed").xhtml.push_str("\u{0}STORYTELLER_SMIL\u{0}");
        sections.last_mut().expect("section was pushed").xhtml.push_str(&smil);
    }
    if sections.is_empty() {
        return Err("No EPUB spine document received synchronized Media Overlay content.".into());
    }

    for section in &sections {
        if !scan.xhtml_item_ids.contains_key(&section.href) {
            return Err(format!(
                "EPUB package manifest does not contain synchronized spine document {}.",
                section.href
            ));
        }
    }

    let total_duration_ms = sections
        .iter()
        .try_fold(0u64, |total, section| total.checked_add(section.duration_ms))
        .ok_or("Media Overlay duration overflowed.")?;
    let package_rewrite = rewrite_package(
        &package_xml,
        &package_path,
        &scan,
        &sections,
        &audio_item_id,
        &audio_manifest_href,
        &audio.media_type,
        total_duration_ms,
    )?;

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!("Could not create EPUB build directory {}: {error}", parent.display())
        })?;
    }
    if destination.exists() {
        fs::remove_file(destination).map_err(|error| {
            format!("Could not replace EPUB build candidate {}: {error}", destination.display())
        })?;
    }
    let output = File::create(destination).map_err(|error| {
        format!("Could not create EPUB build candidate {}: {error}", destination.display())
    })?;
    let mut writer = ZipWriter::new(output);
    writer
        .start_file(
            "mimetype",
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .map_err(|error| format!("Could not write EPUB mimetype entry: {error}"))?;
    writer
        .write_all(b"application/epub+zip")
        .map_err(|error| format!("Could not write EPUB mimetype: {error}"))?;

    let replacement_xhtml = sections
        .iter()
        .map(|section| (section.href.as_str(), split_section_payload(&section.xhtml).0))
        .collect::<HashMap<_, _>>();
    for index in 0..archive.len() {
        if cancellation.is_requested() {
            return Err("EPUB build was cancelled.".into());
        }
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("Could not read source EPUB entry {index}: {error}"))?;
        let name = entry.name().to_string();
        if name == "mimetype" || name == package_path || replacement_xhtml.contains_key(name.as_str()) {
            continue;
        }
        if entry.is_dir() {
            writer
                .add_directory(name, SimpleFileOptions::default())
                .map_err(|error| format!("Could not copy EPUB directory entry: {error}"))?;
            continue;
        }
        let compression = match entry.compression() {
            CompressionMethod::Stored => CompressionMethod::Stored,
            _ => CompressionMethod::Deflated,
        };
        writer
            .start_file(
                &name,
                SimpleFileOptions::default().compression_method(compression),
            )
            .map_err(|error| format!("Could not create EPUB entry {name}: {error}"))?;
        copy_stream_cancellable(&mut entry, &mut writer, cancellation, &name)?;
    }

    writer
        .start_file(
            &package_path,
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .map_err(|error| format!("Could not create rewritten package document: {error}"))?;
    writer
        .write_all(package_rewrite.as_bytes())
        .map_err(|error| format!("Could not write rewritten package document: {error}"))?;

    for section in &sections {
        let (xhtml, smil) = split_section_payload(&section.xhtml);
        writer
            .start_file(
                &section.href,
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .map_err(|error| format!("Could not create synchronized XHTML {}: {error}", section.href))?;
        writer
            .write_all(xhtml.as_bytes())
            .map_err(|error| format!("Could not write synchronized XHTML {}: {error}", section.href))?;
        writer
            .start_file(
                &section.smil_archive_path,
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .map_err(|error| format!("Could not create Media Overlay {}: {error}", section.smil_archive_path))?;
        writer
            .write_all(smil.as_bytes())
            .map_err(|error| format!("Could not write Media Overlay {}: {error}", section.smil_archive_path))?;
    }

    writer
        .start_file(
            &audio_archive_path,
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .map_err(|error| format!("Could not create embedded audiobook entry: {error}"))?;
    let mut encoded = File::open(&encoded_audio_path).map_err(|error| {
        format!("Could not open encoded audiobook {}: {error}", encoded_audio_path.display())
    })?;
    copy_stream_cancellable(&mut encoded, &mut writer, cancellation, "encoded audiobook")?;
    writer
        .finish()
        .map_err(|error| format!("Could not finish EPUB ZIP container: {error}"))?;
    validate_nonempty_file(destination, "Built EPUB candidate")?;

    Ok(EpubBuildSummary {
        overlay_count: sections.len(),
        synchronized_segments: sections.iter().map(|section| section.synchronized_segments).sum(),
        media_duration_ms: total_duration_ms,
    })
}

fn read_alignment(path: &Path) -> Result<AlignmentDocument, String> {
    let data = fs::read(path)
        .map_err(|error| format!("Could not read alignment map {}: {error}", path.display()))?;
    let alignment: AlignmentDocument = serde_json::from_slice(&data)
        .map_err(|error| format!("Could not parse alignment map {}: {error}", path.display()))?;
    if alignment.segments.len() != alignment.total_segments {
        return Err("Alignment map segment count is inconsistent.".into());
    }
    Ok(alignment)
}

fn matched_hrefs(alignment: &AlignmentDocument) -> Result<HashSet<String>, String> {
    let mut hrefs = HashSet::new();
    for segment in &alignment.segments {
        if segment.status != AlignmentStatus::Matched {
            continue;
        }
        let start = segment
            .book_start
            .as_ref()
            .ok_or("Matched alignment segment is missing a book start position.")?;
        let end = segment
            .book_end
            .as_ref()
            .ok_or("Matched alignment segment is missing a book end position.")?;
        if start.href != end.href || start.line_index != end.line_index {
            return Err("Matched alignment segment crosses an XHTML block boundary.".into());
        }
        if segment.audio_end_ms < segment.audio_start_ms {
            return Err("Alignment contains an invalid audio time range.".into());
        }
        hrefs.insert(start.href.clone());
    }
    Ok(hrefs)
}

fn matched_segments_for_href<'a>(
    alignment: &'a AlignmentDocument,
    href: &str,
) -> Vec<(usize, &'a crate::AlignmentSegment)> {
    alignment
        .segments
        .iter()
        .enumerate()
        .filter(|(_, segment)| {
            segment.status == AlignmentStatus::Matched
                && segment
                    .book_start
                    .as_ref()
                    .is_some_and(|position| position.href == href)
        })
        .collect()
}

fn collect_archive_names<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<HashSet<String>, String> {
    let mut names = HashSet::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("Could not inspect source EPUB entry {index}: {error}"))?;
        let name = entry.name().to_string();
        if !names.insert(name.clone()) {
            return Err(format!("Source EPUB contains duplicate ZIP entry {name}."));
        }
    }
    Ok(names)
}

fn validate_mimetype<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<(), String> {
    let mut entry = archive
        .by_name("mimetype")
        .map_err(|error| format!("Source EPUB is missing its mimetype entry: {error}"))?;
    let mut value = Vec::new();
    entry
        .read_to_end(&mut value)
        .map_err(|error| format!("Could not read source EPUB mimetype: {error}"))?;
    if value != b"application/epub+zip" {
        return Err("Source EPUB mimetype is not exactly application/epub+zip.".into());
    }
    Ok(())
}

fn read_archive_text<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
    cancellation: &CancellationToken,
) -> Result<String, String> {
    let mut entry = archive
        .by_name(name)
        .map_err(|error| format!("EPUB entry {name} is unavailable: {error}"))?;
    if !entry.is_file() {
        return Err(format!("EPUB entry {name} is not a regular file."));
    }
    if entry.size() > MAX_XML_BYTES as u64 {
        return Err(format!("EPUB XML entry {name} exceeds the 32 MiB build limit."));
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    loop {
        if cancellation.is_requested() {
            return Err("EPUB build was cancelled.".into());
        }
        let count = entry
            .read(&mut buffer)
            .map_err(|error| format!("Could not read EPUB entry {name}: {error}"))?;
        if count == 0 {
            break;
        }
        if bytes.len().saturating_add(count) > MAX_XML_BYTES {
            return Err(format!("EPUB XML entry {name} expanded beyond the build limit."));
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    String::from_utf8(bytes).map_err(|error| format!("EPUB XML entry {name} is not UTF-8: {error}"))
}

fn scan_package(
    xml: &str,
    package_path: &str,
    overlay_hrefs: &HashSet<String>,
) -> Result<PackageScan, String> {
    let package_dir = parent_archive_path(package_path);
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut existing_ids = HashSet::new();
    let mut xhtml_item_ids = HashMap::new();
    let mut package_version = None;
    let mut manifest_seen = false;
    let mut metadata_seen = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) | Ok(Event::Empty(element)) => {
                let name = local_name(element.name().as_ref());
                if name == b"package" {
                    package_version = attribute_value(&element, b"version")?;
                } else if name == b"manifest" {
                    manifest_seen = true;
                } else if name == b"metadata" {
                    metadata_seen = true;
                } else if name == b"item" {
                    if let Some(id) = attribute_value(&element, b"id")? {
                        existing_ids.insert(id.clone());
                        let href = attribute_value(&element, b"href")?.unwrap_or_default();
                        let media_type = attribute_value(&element, b"media-type")?.unwrap_or_default();
                        if media_type == "application/xhtml+xml" && !href.is_empty() {
                            let resolved = resolve_archive_href(&package_dir, &href)?;
                            xhtml_item_ids.insert(resolved.clone(), id);
                            if overlay_hrefs.contains(&resolved)
                                && attribute_value(&element, b"media-overlay")?.is_some()
                            {
                                return Err(format!(
                                    "Source EPUB content document {resolved} already has a media-overlay association."
                                ));
                            }
                        }
                    }
                } else if let Some(id) = attribute_value(&element, b"id")? {
                    existing_ids.insert(id);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("Could not parse EPUB package document: {error}")),
        }
    }
    let version = package_version.ok_or("EPUB package is missing its version attribute.")?;
    if !version.starts_with('3') {
        return Err(format!(
            "Storyteller Lite currently requires an EPUB 3 source package; found version {version}."
        ));
    }
    if !manifest_seen || !metadata_seen {
        return Err("EPUB package must contain metadata and manifest elements.".into());
    }
    Ok(PackageScan {
        existing_ids,
        xhtml_item_ids,
    })
}

fn rewrite_package(
    xml: &str,
    package_path: &str,
    scan: &PackageScan,
    sections: &[OverlaySection],
    audio_item_id: &str,
    audio_manifest_href: &str,
    audio_media_type: &str,
    total_duration_ms: u64,
) -> Result<String, String> {
    let package_dir = parent_archive_path(package_path);
    let overlays = sections
        .iter()
        .map(|section| (section.href.as_str(), section.overlay_item_id.as_str()))
        .collect::<HashMap<_, _>>();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::new());
    let mut wrote_manifest_additions = false;
    let mut wrote_duration_metadata = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(mut element)) if local_name(element.name().as_ref()) == b"item" => {
                maybe_add_media_overlay(&mut element, &package_dir, &overlays)?;
                writer
                    .write_event(Event::Start(element.into_owned()))
                    .map_err(|error| format!("Could not rewrite EPUB package item: {error}"))?;
            }
            Ok(Event::Empty(mut element)) if local_name(element.name().as_ref()) == b"item" => {
                maybe_add_media_overlay(&mut element, &package_dir, &overlays)?;
                writer
                    .write_event(Event::Empty(element.into_owned()))
                    .map_err(|error| format!("Could not rewrite EPUB package item: {error}"))?;
            }
            Ok(Event::End(element)) if local_name(element.name().as_ref()) == b"manifest" => {
                for section in sections {
                    write_empty_item(
                        &mut writer,
                        &section.overlay_item_id,
                        &section.smil_manifest_href,
                        "application/smil+xml",
                    )?;
                }
                write_empty_item(&mut writer, audio_item_id, audio_manifest_href, audio_media_type)?;
                wrote_manifest_additions = true;
                writer
                    .write_event(Event::End(element.into_owned()))
                    .map_err(|error| format!("Could not close EPUB manifest: {error}"))?;
            }
            Ok(Event::End(element)) if local_name(element.name().as_ref()) == b"metadata" => {
                for section in sections {
                    let refines = format!("#{}", section.overlay_item_id);
                    write_meta_duration(&mut writer, Some(&refines), section.duration_ms)?;
                }
                write_meta_duration(&mut writer, None, total_duration_ms)?;
                wrote_duration_metadata = true;
                writer
                    .write_event(Event::End(element.into_owned()))
                    .map_err(|error| format!("Could not close EPUB metadata: {error}"))?;
            }
            Ok(Event::Eof) => break,
            Ok(event) => writer
                .write_event(event.into_owned())
                .map_err(|error| format!("Could not rewrite EPUB package document: {error}"))?,
            Err(error) => return Err(format!("Could not parse EPUB package document: {error}")),
        }
    }
    if !wrote_manifest_additions || !wrote_duration_metadata {
        return Err("Could not add Media Overlay manifest or duration metadata.".into());
    }
    for section in sections {
        if !scan.xhtml_item_ids.contains_key(&section.href) {
            return Err(format!("Missing package item for synchronized XHTML {}.", section.href));
        }
    }
    String::from_utf8(writer.into_inner())
        .map_err(|error| format!("Rewritten EPUB package is not UTF-8: {error}"))
}

fn maybe_add_media_overlay(
    element: &mut BytesStart<'_>,
    package_dir: &str,
    overlays: &HashMap<&str, &str>,
) -> Result<(), String> {
    let href = attribute_value(element, b"href")?.unwrap_or_default();
    if href.is_empty() {
        return Ok(());
    }
    let resolved = resolve_archive_href(package_dir, &href)?;
    if let Some(overlay_id) = overlays.get(resolved.as_str()) {
        if attribute_value(element, b"media-overlay")?.is_some() {
            return Err(format!("EPUB package item {resolved} already has media-overlay."));
        }
        element.push_attribute(("media-overlay", *overlay_id));
    }
    Ok(())
}

fn write_empty_item(
    writer: &mut Writer<Vec<u8>>,
    id: &str,
    href: &str,
    media_type: &str,
) -> Result<(), String> {
    let mut item = BytesStart::new("item");
    item.push_attribute(("id", id));
    item.push_attribute(("href", href));
    item.push_attribute(("media-type", media_type));
    writer
        .write_event(Event::Empty(item))
        .map_err(|error| format!("Could not add EPUB manifest item: {error}"))
}

fn write_meta_duration(
    writer: &mut Writer<Vec<u8>>,
    refines: Option<&str>,
    milliseconds: u64,
) -> Result<(), String> {
    let mut meta = BytesStart::new("meta");
    meta.push_attribute(("property", "media:duration"));
    if let Some(refines) = refines {
        meta.push_attribute(("refines", refines));
    }
    writer
        .write_event(Event::Start(meta))
        .map_err(|error| format!("Could not add Media Overlay duration metadata: {error}"))?;
    let value = format_clock(milliseconds);
    writer
        .write_event(Event::Text(BytesText::new(&value)))
        .map_err(|error| format!("Could not write Media Overlay duration: {error}"))?;
    writer
        .write_event(Event::End(BytesEnd::new("meta")))
        .map_err(|error| format!("Could not close Media Overlay duration metadata: {error}"))
}

fn build_smil(
    segments: &[(usize, &crate::AlignmentSegment)],
    anchors: &HashMap<usize, String>,
    text_href: &str,
    audio_href: &str,
    section_index: usize,
) -> Result<(String, u64), String> {
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<smil xmlns=\"http://www.w3.org/ns/SMIL\" xmlns:epub=\"http://www.idpf.org/2007/ops\" version=\"3.0\"><body><seq>",
    );
    let mut duration_ms = 0u64;
    for (segment_index, segment) in segments {
        let position = segment
            .book_start
            .as_ref()
            .ok_or("Matched segment has no EPUB start position.")?;
        let anchor = anchors.get(&position.line_index).ok_or_else(|| {
            format!(
                "Could not map EPUB line {} in {} to an XHTML fragment.",
                position.line_index, position.href
            )
        })?;
        let clip_duration = segment
            .audio_end_ms
            .checked_sub(segment.audio_start_ms)
            .ok_or("Alignment audio range is invalid.")?;
        duration_ms = duration_ms
            .checked_add(clip_duration)
            .ok_or("Media Overlay duration overflowed.")?;
        xml.push_str(&format!(
            "<par id=\"stl-par-{}-{}\"><text src=\"{}#{}\"/><audio src=\"{}\" clipBegin=\"{}\" clipEnd=\"{}\"/></par>",
            section_index + 1,
            segment_index + 1,
            escape_xml(text_href),
            escape_xml(anchor),
            escape_xml(audio_href),
            format_clock(segment.audio_start_ms),
            format_clock(segment.audio_end_ms),
        ));
    }
    xml.push_str("</seq></body></smil>");
    Ok((xml, duration_ms))
}

fn annotate_xhtml_blocks(
    xml: &str,
    requested_lines: &HashSet<usize>,
    section_index: usize,
) -> Result<(String, HashMap<usize, String>), String> {
    let (line_owners, existing_ids) = map_line_owners(xml)?;
    let mut target_ids = HashMap::<usize, String>::new();
    let mut used_ids = existing_ids.values().cloned().collect::<HashSet<_>>();
    let mut line_anchors = HashMap::new();
    for line_index in requested_lines {
        let owner = line_owners.get(*line_index).copied().ok_or_else(|| {
            format!("EPUB XHTML has no visible block for corpus line {line_index}.")
        })?;
        let id = if let Some(existing) = existing_ids.get(&owner) {
            existing.clone()
        } else if let Some(generated) = target_ids.get(&owner) {
            generated.clone()
        } else {
            let base = format!("stl-mo-s{}-b{}", section_index + 1, owner + 1);
            let generated = unique_id(&base, &mut used_ids);
            target_ids.insert(owner, generated.clone());
            generated
        };
        line_anchors.insert(*line_index, id);
    }

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::new());
    let mut suppressed_depth = 0usize;
    let mut ordinal = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(mut element)) => {
                let name = local_name(element.name().as_ref());
                if suppressed_depth > 0 {
                    suppressed_depth += 1;
                } else if is_suppressed_element(name) {
                    suppressed_depth = 1;
                } else if is_anchor_container(name) {
                    let current = ordinal;
                    ordinal += 1;
                    if let Some(id) = target_ids.get(&current) {
                        if attribute_value(&element, b"id")?.is_none() {
                            element.push_attribute(("id", id.as_str()));
                        }
                    }
                }
                writer
                    .write_event(Event::Start(element.into_owned()))
                    .map_err(|error| format!("Could not rewrite EPUB XHTML: {error}"))?;
            }
            Ok(Event::End(element)) => {
                if suppressed_depth > 0 {
                    suppressed_depth -= 1;
                }
                writer
                    .write_event(Event::End(element.into_owned()))
                    .map_err(|error| format!("Could not rewrite EPUB XHTML: {error}"))?;
            }
            Ok(Event::Eof) => break,
            Ok(event) => writer
                .write_event(event.into_owned())
                .map_err(|error| format!("Could not rewrite EPUB XHTML: {error}"))?,
            Err(error) => return Err(format!("Could not parse EPUB XHTML for anchors: {error}")),
        }
    }
    let rewritten = String::from_utf8(writer.into_inner())
        .map_err(|error| format!("Rewritten EPUB XHTML is not UTF-8: {error}"))?;
    Ok((rewritten, line_anchors))
}

fn map_line_owners(xml: &str) -> Result<(Vec<usize>, HashMap<usize, String>), String> {
    #[derive(Debug)]
    struct Frame {
        name: Vec<u8>,
        ordinal: usize,
    }

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut stack = Vec::<Frame>::new();
    let mut existing_ids = HashMap::new();
    let mut line_owners = Vec::new();
    let mut line_owner = None;
    let mut line_has_text = false;
    let mut suppressed_depth = 0usize;
    let mut next_ordinal = 0usize;

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let name = local_name(element.name().as_ref());
                if suppressed_depth > 0 {
                    suppressed_depth += 1;
                    continue;
                }
                if is_suppressed_element(name) {
                    suppressed_depth = 1;
                    continue;
                }
                if is_boundary_element(name) {
                    finish_line(&mut line_owners, &mut line_owner, &mut line_has_text)?;
                }
                if is_anchor_container(name) {
                    let ordinal = next_ordinal;
                    next_ordinal += 1;
                    if let Some(id) = attribute_value(&element, b"id")?.filter(|id| !id.trim().is_empty()) {
                        existing_ids.insert(ordinal, id);
                    }
                    stack.push(Frame {
                        name: name.to_vec(),
                        ordinal,
                    });
                }
            }
            Ok(Event::Empty(element)) => {
                if suppressed_depth == 0 && is_boundary_element(local_name(element.name().as_ref())) {
                    finish_line(&mut line_owners, &mut line_owner, &mut line_has_text)?;
                }
            }
            Ok(Event::End(element)) => {
                if suppressed_depth > 0 {
                    suppressed_depth -= 1;
                    continue;
                }
                let name = local_name(element.name().as_ref());
                if is_boundary_element(name) {
                    finish_line(&mut line_owners, &mut line_owner, &mut line_has_text)?;
                }
                if is_anchor_container(name) {
                    let frame = stack.pop().ok_or("EPUB XHTML block nesting is invalid.")?;
                    if frame.name.as_slice() != name {
                        return Err("EPUB XHTML block nesting is invalid.".into());
                    }
                }
            }
            Ok(Event::Text(text)) if suppressed_depth == 0 => {
                let decoded = text
                    .decode()
                    .map_err(|error| format!("Could not decode EPUB XHTML text: {error}"))?;
                let decoded = unescape(&decoded)
                    .map_err(|error| format!("Could not unescape EPUB XHTML text: {error}"))?;
                if decoded.chars().any(|character| !character.is_whitespace()) {
                    line_has_text = true;
                    if line_owner.is_none() {
                        line_owner = stack.last().map(|frame| frame.ordinal);
                    }
                }
            }
            Ok(Event::CData(text)) if suppressed_depth == 0 => {
                let decoded = text
                    .decode()
                    .map_err(|error| format!("Could not decode EPUB XHTML CDATA: {error}"))?;
                if decoded.chars().any(|character| !character.is_whitespace()) {
                    line_has_text = true;
                    if line_owner.is_none() {
                        line_owner = stack.last().map(|frame| frame.ordinal);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("Could not parse EPUB XHTML for block map: {error}")),
        }
    }
    finish_line(&mut line_owners, &mut line_owner, &mut line_has_text)?;
    Ok((line_owners, existing_ids))
}

fn finish_line(
    lines: &mut Vec<usize>,
    owner: &mut Option<usize>,
    has_text: &mut bool,
) -> Result<(), String> {
    if *has_text {
        lines.push(owner.ok_or("Visible EPUB text is not contained in an anchorable XHTML block.")?);
    }
    *owner = None;
    *has_text = false;
    Ok(())
}

fn split_section_payload(payload: &str) -> (&str, &str) {
    payload
        .split_once("\u{0}STORYTELLER_SMIL\u{0}")
        .unwrap_or((payload, ""))
}

fn unique_resource_root(package_dir: &str, names: &HashSet<String>) -> String {
    for suffix in 0usize.. {
        let leaf = if suffix == 0 {
            "storyteller".to_string()
        } else {
            format!("storyteller-{suffix}")
        };
        let root = join_archive_path(package_dir, &leaf);
        let prefix = format!("{root}/");
        if !names.iter().any(|name| name == &root || name.starts_with(&prefix)) {
            return root;
        }
    }
    unreachable!()
}

fn unique_id(base: &str, used: &mut HashSet<String>) -> String {
    if used.insert(base.to_string()) {
        return base.to_string();
    }
    for suffix in 2usize.. {
        let candidate = format!("{base}-{suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!()
}

fn parent_archive_path(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default()
}

fn join_archive_path(base: &str, child: &str) -> String {
    if base.is_empty() {
        child.trim_start_matches('/').to_string()
    } else {
        format!("{}/{}", base.trim_end_matches('/'), child.trim_start_matches('/'))
    }
}

fn relative_archive_path(from_dir: &str, target: &str) -> String {
    let from = from_dir.split('/').filter(|part| !part.is_empty()).collect::<Vec<_>>();
    let target = target.split('/').filter(|part| !part.is_empty()).collect::<Vec<_>>();
    let mut common = 0usize;
    while common < from.len() && common < target.len() && from[common] == target[common] {
        common += 1;
    }
    let mut result = Vec::new();
    result.extend(std::iter::repeat_n("..", from.len().saturating_sub(common)));
    result.extend(target.iter().skip(common).copied());
    if result.is_empty() {
        ".".into()
    } else {
        result.join("/")
    }
}

fn resolve_archive_href(base: &str, href: &str) -> Result<String, String> {
    let href = href.split(['#', '?']).next().unwrap_or(href);
    if href.contains("://") || href.starts_with("data:") || href.starts_with('/') {
        return Err(format!("EPUB package resource uses unsupported URI {href}."));
    }
    let decoded = percent_decode(href)?;
    let combined = join_archive_path(base, &decoded);
    normalize_archive_path(&combined)
}

fn normalize_archive_path(path: &str) -> Result<String, String> {
    let mut parts = Vec::new();
    for part in path.replace('\\', "/").split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(format!("EPUB path escapes archive root: {path}"));
                }
            }
            value => parts.push(value.to_string()),
        }
    }
    if parts.is_empty() {
        return Err("EPUB archive path is empty.".into());
    }
    Ok(parts.join("/"))
}

fn percent_decode(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(format!("EPUB href has incomplete percent escape: {value}"));
            }
            let high = hex(bytes[index + 1])
                .ok_or_else(|| format!("EPUB href has invalid percent escape: {value}"))?;
            let low = hex(bytes[index + 2])
                .ok_or_else(|| format!("EPUB href has invalid percent escape: {value}"))?;
            output.push((high << 4) | low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).map_err(|error| format!("EPUB href is not UTF-8: {error}"))
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn uri_path(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'-' | b'_' | b'~') {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

fn format_clock(milliseconds: u64) -> String {
    let hours = milliseconds / 3_600_000;
    let minutes = (milliseconds % 3_600_000) / 60_000;
    let seconds = (milliseconds % 60_000) / 1000;
    let millis = milliseconds % 1000;
    format!("{hours}:{minutes:02}:{seconds:02}.{millis:03}")
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn attribute_value(element: &BytesStart<'_>, wanted: &[u8]) -> Result<Option<String>, String> {
    for attribute in element.attributes().with_checks(false) {
        let attribute = attribute.map_err(|error| format!("Invalid EPUB XML attribute: {error}"))?;
        if local_name(attribute.key.as_ref()) != wanted {
            continue;
        }
        let raw = std::str::from_utf8(attribute.value.as_ref())
            .map_err(|error| format!("EPUB XML attribute is not UTF-8: {error}"))?;
        let value = unescape(raw)
            .map_err(|error| format!("Could not unescape EPUB XML attribute: {error}"))?;
        return Ok(Some(value.into_owned()));
    }
    Ok(None)
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

fn is_suppressed_element(name: &[u8]) -> bool {
    matches!(
        name,
        b"head" | b"script" | b"style" | b"svg" | b"math" | b"noscript"
    )
}

fn is_boundary_element(name: &[u8]) -> bool {
    matches!(
        name,
        b"address"
            | b"article"
            | b"aside"
            | b"blockquote"
            | b"br"
            | b"dd"
            | b"div"
            | b"dl"
            | b"dt"
            | b"figcaption"
            | b"figure"
            | b"footer"
            | b"h1"
            | b"h2"
            | b"h3"
            | b"h4"
            | b"h5"
            | b"h6"
            | b"header"
            | b"li"
            | b"main"
            | b"nav"
            | b"ol"
            | b"p"
            | b"pre"
            | b"section"
            | b"table"
            | b"td"
            | b"th"
            | b"tr"
            | b"ul"
    )
}

fn is_anchor_container(name: &[u8]) -> bool {
    name == b"body" || (is_boundary_element(name) && name != b"br")
}

fn copy_stream_cancellable<R: Read, W: Write>(
    input: &mut R,
    output: &mut W,
    cancellation: &CancellationToken,
    label: &str,
) -> Result<(), String> {
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    loop {
        if cancellation.is_requested() {
            return Err("EPUB build was cancelled.".into());
        }
        let count = input
            .read(&mut buffer)
            .map_err(|error| format!("Could not read {label}: {error}"))?;
        if count == 0 {
            return Ok(());
        }
        output
            .write_all(&buffer[..count])
            .map_err(|error| format!("Could not write {label}: {error}"))?;
    }
}

fn validate_nonempty_file(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("{label} is unavailable at {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(format!("{label} is not a non-empty regular file: {}", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_anchor_map_reuses_existing_id_and_injects_missing_id() {
        let xml = r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p id="existing">One line.</p><p>Second <em>line</em>.</p></body></html>"#;
        let requested = HashSet::from([0usize, 1usize]);
        let (rewritten, anchors) = annotate_xhtml_blocks(xml, &requested, 0).unwrap();
        assert_eq!(anchors[&0], "existing");
        assert!(anchors[&1].starts_with("stl-mo-s1-b"));
        assert!(rewritten.contains("id=\"existing\""));
        assert!(rewritten.contains(&format!("id=\"{}\"", anchors[&1])));
    }

    #[test]
    fn relative_archive_refs_walk_between_overlay_text_and_audio() {
        assert_eq!(
            relative_archive_path("OPS/storyteller/overlays", "OPS/Text/chapter.xhtml"),
            "../../Text/chapter.xhtml"
        );
        assert_eq!(
            relative_archive_path("OPS/storyteller/overlays", "OPS/storyteller/audio/audio.m4a"),
            "../audio/audio.m4a"
        );
    }

    #[test]
    fn media_clock_preserves_milliseconds() {
        assert_eq!(format_clock(3_723_045), "1:02:03.045");
    }
}
