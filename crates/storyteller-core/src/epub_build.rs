use crate::{
    epub_overlay::{
        add_supplemental_package_items, annotate_xhtml_blocks, build_smil, build_supplemental_smil,
        build_supplemental_xhtml, join_archive_path, parent_archive_path, relative_archive_path,
        rewrite_package, scan_package, unique_id, uri_path, OverlaySectionSpec,
        SupplementalOverlaySpec,
    },
    read_audio_review_report, read_encoded_audio_descriptor, read_epub_corpus, AlignmentDocument,
    AlignmentStatus, AudioReviewClassification, AudioReviewDecision, CancellationToken,
};
use std::{
    collections::HashSet,
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
    let result = build_readaloud_epub_inner(
        source_epub,
        corpus_path,
        alignment_path,
        review_path,
        encoded_audio_descriptor_path,
        encoded_audio_dir,
        destination,
        cancellation,
    );
    if result.is_err() && destination.exists() {
        let _ = fs::remove_file(destination);
    }
    result
}

fn build_readaloud_epub_inner(
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
    if !review.is_complete() {
        return Err("Unmatched audio must be reviewed before the EPUB can be built.".into());
    }
    let audio = read_encoded_audio_descriptor(encoded_audio_descriptor_path)?;
    if audio.file_name.contains('/') || audio.file_name.contains('\\') {
        return Err("Encoded audio descriptor filename must not contain a path.".into());
    }
    let encoded_audio_path = encoded_audio_dir.join(&audio.file_name);
    validate_nonempty_file(&encoded_audio_path, "Encoded audiobook")?;

    let source = File::open(source_epub).map_err(|error| {
        format!(
            "Could not open source EPUB {}: {error}",
            source_epub.display()
        )
    })?;
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
    let audio_archive_path =
        join_archive_path(&resource_root, &format!("audio/{}", audio.file_name));
    let audio_manifest_href = relative_archive_path(&package_dir, &audio_archive_path);

    let mut sections = Vec::<OverlaySectionSpec>::new();
    for (section_index, section) in corpus.sections.iter().enumerate() {
        if cancellation.is_requested() {
            return Err("EPUB build was cancelled.".into());
        }
        let segments = matched_segments_for_href(&alignment, &section.href);
        if segments.is_empty() {
            continue;
        }
        if !scan.xhtml_item_ids.contains_key(&section.href) {
            return Err(format!(
                "EPUB package manifest does not contain synchronized spine document {}.",
                section.href
            ));
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
        let (annotated_xhtml, anchors) =
            annotate_xhtml_blocks(&source_xhtml, &requested_lines, section_index)?;
        let overlay_item_id = unique_id(&format!("stl-mo-{:04}", section_index + 1), &mut used_ids);
        let smil_archive_path = join_archive_path(
            &resource_root,
            &format!("overlays/overlay-{:04}.smil", section_index + 1),
        );
        let smil_manifest_href = relative_archive_path(&package_dir, &smil_archive_path);
        let smil_dir = parent_archive_path(&smil_archive_path);
        let text_href = uri_path(&relative_archive_path(&smil_dir, &section.href));
        let audio_href = uri_path(&relative_archive_path(&smil_dir, &audio_archive_path));
        let (smil, duration_ms) =
            build_smil(&segments, &anchors, &text_href, &audio_href, section_index)?;
        sections.push(OverlaySectionSpec {
            href: section.href.clone(),
            xhtml: annotated_xhtml,
            smil,
            smil_archive_path,
            smil_manifest_href,
            overlay_item_id,
            duration_ms,
            synchronized_segments: segments.len(),
        });
    }
    if sections.is_empty() {
        return Err("No EPUB spine document received synchronized Media Overlay content.".into());
    }

    let mut supplemental = Vec::<SupplementalOverlaySpec>::new();
    for (review_index, item) in review.unmatched.iter().enumerate() {
        let AudioReviewDecision::Assigned {
            destination,
            classification: Some(classification),
            ..
        } = &item.decision
        else {
            continue;
        };
        let Some(placement) = destination.supplemental else {
            continue;
        };
        if !matches!(
            classification,
            AudioReviewClassification::Introduction | AudioReviewClassification::Credits
        ) {
            return Err("Supplemental audio page has an unsupported classification.".into());
        }
        let anchor_item_id = scan.xhtml_item_ids.get(&destination.href).ok_or_else(|| {
            format!(
                "Supplemental audio anchor {} is not an EPUB manifest XHTML item.",
                destination.href
            )
        })?;
        let leaf = match classification {
            AudioReviewClassification::Introduction => "introduction",
            AudioReviewClassification::Credits => "credits",
            _ => unreachable!(),
        };
        let xhtml_archive_path = join_archive_path(
            &resource_root,
            &format!("text/{leaf}-{:04}.xhtml", review_index + 1),
        );
        let smil_archive_path = join_archive_path(
            &resource_root,
            &format!("overlays/{leaf}-{:04}.smil", review_index + 1),
        );
        let xhtml_item_id = unique_id(
            &format!("stl-extra-page-{:04}", review_index + 1),
            &mut used_ids,
        );
        let overlay_item_id = unique_id(
            &format!("stl-extra-mo-{:04}", review_index + 1),
            &mut used_ids,
        );
        let paragraph_id = format!("stl-extra-text-{}", review_index + 1);
        let xhtml =
            build_supplemental_xhtml(*classification, &item.transcript_text, &paragraph_id)?;
        let smil_dir = parent_archive_path(&smil_archive_path);
        let text_href = uri_path(&relative_archive_path(&smil_dir, &xhtml_archive_path));
        let audio_href = uri_path(&relative_archive_path(&smil_dir, &audio_archive_path));
        let (smil, duration_ms) = build_supplemental_smil(
            *classification,
            &text_href,
            &paragraph_id,
            &audio_href,
            item.audio_start_ms,
            item.audio_end_ms,
            review_index,
        )?;
        supplemental.push(SupplementalOverlaySpec {
            xhtml,
            xhtml_archive_path: xhtml_archive_path.clone(),
            xhtml_manifest_href: relative_archive_path(&package_dir, &xhtml_archive_path),
            xhtml_item_id,
            smil,
            smil_archive_path: smil_archive_path.clone(),
            smil_manifest_href: relative_archive_path(&package_dir, &smil_archive_path),
            overlay_item_id,
            anchor_item_id: anchor_item_id.clone(),
            placement,
            duration_ms,
        });
    }

    let total_duration_ms = sections
        .iter()
        .map(|section| section.duration_ms)
        .chain(supplemental.iter().map(|section| section.duration_ms))
        .try_fold(0u64, |total, duration| total.checked_add(duration))
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
    let package_rewrite = add_supplemental_package_items(&package_rewrite, &supplemental)?;

    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Could not create EPUB build directory {}: {error}",
                parent.display()
            )
        })?;
    }
    if destination.exists() {
        fs::remove_file(destination).map_err(|error| {
            format!(
                "Could not replace EPUB build candidate {}: {error}",
                destination.display()
            )
        })?;
    }
    let output = File::create(destination).map_err(|error| {
        format!(
            "Could not create EPUB build candidate {}: {error}",
            destination.display()
        )
    })?;
    let mut writer = ZipWriter::new(output);
    write_mimetype(&mut writer)?;

    let replacement_xhtml = sections
        .iter()
        .map(|section| section.href.as_str())
        .collect::<HashSet<_>>();
    for index in 0..archive.len() {
        if cancellation.is_requested() {
            return Err("EPUB build was cancelled.".into());
        }
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("Could not read source EPUB entry {index}: {error}"))?;
        let name = entry.name().to_string();
        if name == "mimetype" || name == package_path || replacement_xhtml.contains(name.as_str()) {
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

    write_text_entry(&mut writer, &package_path, &package_rewrite)?;
    for section in &sections {
        write_text_entry(&mut writer, &section.href, &section.xhtml)?;
        write_text_entry(&mut writer, &section.smil_archive_path, &section.smil)?;
    }
    for section in &supplemental {
        write_text_entry(&mut writer, &section.xhtml_archive_path, &section.xhtml)?;
        write_text_entry(&mut writer, &section.smil_archive_path, &section.smil)?;
    }

    writer
        .start_file(
            &audio_archive_path,
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .map_err(|error| format!("Could not create embedded audiobook entry: {error}"))?;
    let mut encoded = File::open(&encoded_audio_path).map_err(|error| {
        format!(
            "Could not open encoded audiobook {}: {error}",
            encoded_audio_path.display()
        )
    })?;
    copy_stream_cancellable(&mut encoded, &mut writer, cancellation, "encoded audiobook")?;
    let output = writer
        .finish()
        .map_err(|error| format!("Could not finish EPUB ZIP container: {error}"))?;
    output
        .sync_all()
        .map_err(|error| format!("Could not flush EPUB build candidate: {error}"))?;
    validate_nonempty_file(destination, "Built EPUB candidate")?;

    Ok(EpubBuildSummary {
        overlay_count: sections.len() + supplemental.len(),
        synchronized_segments: sections
            .iter()
            .map(|section| section.synchronized_segments)
            .sum::<usize>()
            + supplemental.len(),
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
        if segment.audio_end_ms <= segment.audio_start_ms {
            return Err("Matched alignment segment has an invalid audio time range.".into());
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

fn collect_archive_names<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
) -> Result<HashSet<String>, String> {
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
        return Err(format!(
            "EPUB XML entry {name} exceeds the 32 MiB build limit."
        ));
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
            return Err(format!(
                "EPUB XML entry {name} expanded beyond the build limit."
            ));
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    String::from_utf8(bytes).map_err(|error| format!("EPUB XML entry {name} is not UTF-8: {error}"))
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
        if !names
            .iter()
            .any(|name| name == &root || name.starts_with(&prefix))
        {
            return root;
        }
    }
    unreachable!()
}

fn write_mimetype<W: Write + Seek>(writer: &mut ZipWriter<W>) -> Result<(), String> {
    writer
        .start_file(
            "mimetype",
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .map_err(|error| format!("Could not write EPUB mimetype entry: {error}"))?;
    writer
        .write_all(b"application/epub+zip")
        .map_err(|error| format!("Could not write EPUB mimetype: {error}"))
}

fn write_text_entry<W: Write + Seek>(
    writer: &mut ZipWriter<W>,
    name: &str,
    text: &str,
) -> Result<(), String> {
    writer
        .start_file(
            name,
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .map_err(|error| format!("Could not create EPUB entry {name}: {error}"))?;
    writer
        .write_all(text.as_bytes())
        .map_err(|error| format!("Could not write EPUB entry {name}: {error}"))
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
        return Err(format!(
            "{label} is not a non-empty regular file: {}",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_root_avoids_existing_storyteller_tree() {
        let names = HashSet::from([
            "OPS/storyteller/audio/old.m4a".to_string(),
            "OPS/Text/chapter.xhtml".to_string(),
        ]);
        assert_eq!(unique_resource_root("OPS", &names), "OPS/storyteller-1");
    }
}
