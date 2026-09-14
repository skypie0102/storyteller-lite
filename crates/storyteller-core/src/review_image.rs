use crate::{AlignmentDocument, AlignmentStatus, CancellationToken, EpubCorpus};
use quick_xml::{
    escape::unescape,
    events::{BytesStart, Event},
    Reader,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{Read, Seek},
    path::Path,
};
use zip::{result::ZipError, ZipArchive};

pub const DEFAULT_REVIEW_IMAGE_DOCUMENT_LIMIT: usize = 12;
pub const DEFAULT_REVIEW_IMAGE_LIMIT: usize = 24;

const MAX_PACKAGE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_XHTML_BYTES: u64 = 32 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 25 * 1024 * 1024;
const READ_BUFFER_BYTES: usize = 64 * 1024;
const MAX_HINT_CHARS: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioReviewImageCandidate {
    pub document_href: String,
    pub image_href: String,
    pub media_type: String,
    pub byte_size: u64,
    pub document_spine_index: usize,
    pub image_ordinal: usize,
    pub embedded_text: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManifestItem {
    href: String,
    media_type: String,
    is_navigation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PackageDocument {
    manifest: HashMap<String, ManifestItem>,
    spine: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RawImageReference {
    href: String,
    image_ordinal: usize,
    embedded_text: Vec<String>,
}

#[derive(Debug, Default)]
struct SvgContext {
    images: Vec<(String, usize, Vec<String>)>,
    hints: Vec<String>,
}

pub fn review_image_candidates(
    epub_path: &Path,
    alignment: &AlignmentDocument,
    corpus: &EpubCorpus,
    alignment_index: usize,
    document_limit: usize,
    image_limit: usize,
    cancellation: &CancellationToken,
) -> Result<Vec<AudioReviewImageCandidate>, String> {
    if document_limit == 0 {
        return Err("Audio review image document limit must be greater than zero.".into());
    }
    if image_limit == 0 {
        return Err("Audio review image limit must be greater than zero.".into());
    }
    if cancellation.is_requested() {
        return Err("EPUB image candidate discovery was cancelled.".into());
    }
    let segment = alignment
        .segments
        .get(alignment_index)
        .ok_or_else(|| format!("Alignment segment {alignment_index} does not exist."))?;
    if segment.status != AlignmentStatus::Unmatched {
        return Err(format!(
            "Alignment segment {alignment_index} is already matched and does not need image review."
        ));
    }
    if corpus.package_path.trim().is_empty() {
        return Err("EPUB corpus does not identify its package document.".into());
    }

    let file = File::open(epub_path)
        .map_err(|error| format!("Could not open EPUB {}: {error}", epub_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| format!("Could not read EPUB ZIP container: {error}"))?;
    let package_xml = read_zip_text(
        &mut archive,
        &corpus.package_path,
        MAX_PACKAGE_BYTES,
        cancellation,
    )?;
    let package = parse_package_document(&package_xml)?;
    let package_dir = parent_archive_path(&corpus.package_path);

    let mut spine_documents = Vec::<String>::new();
    for idref in &package.spine {
        let item = package
            .manifest
            .get(idref)
            .ok_or_else(|| format!("EPUB spine references missing manifest item {idref}."))?;
        if item.media_type != "application/xhtml+xml" || item.is_navigation {
            continue;
        }
        spine_documents.push(resolve_archive_href(&package_dir, &item.href)?);
    }
    if spine_documents.is_empty() {
        return Err("EPUB spine contains no XHTML documents for image review.".into());
    }
    let spine_positions = spine_documents
        .iter()
        .enumerate()
        .map(|(index, href)| (href.clone(), index))
        .collect::<HashMap<_, _>>();

    let mut image_media_types = HashMap::<String, String>::new();
    for item in package.manifest.values() {
        if !item.media_type.starts_with("image/") {
            continue;
        }
        if let Ok(href) = resolve_archive_href(&package_dir, &item.href) {
            image_media_types.insert(href, item.media_type.clone());
        }
    }

    let previous = nearest_previous_anchor(
        alignment,
        alignment_index,
        &spine_positions,
    )?;
    let next = nearest_next_anchor(alignment, alignment_index, &spine_positions)?;
    let last_spine_index = spine_documents.len() - 1;
    let (minimum, maximum, focus) = match (previous, next) {
        (Some((previous_segment, previous_spine)), Some((next_segment, next_spine))) => {
            if previous_spine > next_spine {
                return Err("Neighboring alignment anchors are not monotonic in EPUB spine order.".into());
            }
            let segment_span = next_segment.saturating_sub(previous_segment).max(1);
            let segment_offset = alignment_index.saturating_sub(previous_segment);
            let spine_span = next_spine - previous_spine;
            let focus = previous_spine
                + spine_span.saturating_mul(segment_offset) / segment_span;
            (previous_spine, next_spine, focus)
        }
        (Some((_, previous_spine)), None) => (previous_spine, last_spine_index, previous_spine),
        (None, Some((_, next_spine))) => (0, next_spine, next_spine),
        (None, None) => (0, last_spine_index, 0),
    };

    let mut document_indexes = (minimum..=maximum).collect::<Vec<_>>();
    document_indexes.sort_by(|left, right| {
        left.abs_diff(focus)
            .cmp(&right.abs_diff(focus))
            .then_with(|| left.cmp(right))
    });
    document_indexes.truncate(document_limit.min(document_indexes.len()));

    let mut candidates = Vec::new();
    let mut seen = HashSet::<(String, String)>::new();
    for document_spine_index in document_indexes {
        if cancellation.is_requested() {
            return Err("EPUB image candidate discovery was cancelled.".into());
        }
        let document_href = &spine_documents[document_spine_index];
        let document_xml = read_zip_text(
            &mut archive,
            document_href,
            MAX_XHTML_BYTES,
            cancellation,
        )?;
        let document_dir = parent_archive_path(document_href);
        for reference in extract_image_references(&document_xml)? {
            if candidates.len() >= image_limit {
                return Ok(candidates);
            }
            let Some(image_href) = resolve_optional_resource_href(&document_dir, &reference.href)? else {
                continue;
            };
            let Some(media_type) = image_media_types.get(&image_href) else {
                continue;
            };
            if !seen.insert((document_href.clone(), image_href.clone())) {
                continue;
            }
            let Some(byte_size) = image_entry_size(&mut archive, &image_href)? else {
                continue;
            };
            candidates.push(AudioReviewImageCandidate {
                document_href: document_href.clone(),
                image_href,
                media_type: media_type.clone(),
                byte_size,
                document_spine_index,
                image_ordinal: reference.image_ordinal,
                embedded_text: reference.embedded_text,
            });
        }
    }
    Ok(candidates)
}

fn nearest_previous_anchor(
    alignment: &AlignmentDocument,
    alignment_index: usize,
    spine_positions: &HashMap<String, usize>,
) -> Result<Option<(usize, usize)>, String> {
    for (segment_index, segment) in alignment.segments[..alignment_index].iter().enumerate().rev() {
        if segment.status != AlignmentStatus::Matched {
            continue;
        }
        let position = segment
            .book_end
            .as_ref()
            .ok_or("Matched alignment segment is missing its book end position.")?;
        let spine_index = spine_positions.get(&position.href).copied().ok_or_else(|| {
            format!(
                "Matched EPUB document {} is not present in the package spine.",
                position.href
            )
        })?;
        return Ok(Some((segment_index, spine_index)));
    }
    Ok(None)
}

fn nearest_next_anchor(
    alignment: &AlignmentDocument,
    alignment_index: usize,
    spine_positions: &HashMap<String, usize>,
) -> Result<Option<(usize, usize)>, String> {
    for (offset, segment) in alignment.segments[alignment_index + 1..].iter().enumerate() {
        if segment.status != AlignmentStatus::Matched {
            continue;
        }
        let position = segment
            .book_start
            .as_ref()
            .ok_or("Matched alignment segment is missing its book start position.")?;
        let spine_index = spine_positions.get(&position.href).copied().ok_or_else(|| {
            format!(
                "Matched EPUB document {} is not present in the package spine.",
                position.href
            )
        })?;
        return Ok(Some((alignment_index + 1 + offset, spine_index)));
    }
    Ok(None)
}

fn parse_package_document(xml: &str) -> Result<PackageDocument, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut manifest = HashMap::new();
    let mut spine = Vec::new();
    let mut in_manifest = false;
    let mut in_spine = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let name = element.name();
                match local_name(name.as_ref()) {
                    b"manifest" => in_manifest = true,
                    b"spine" => in_spine = true,
                    b"item" if in_manifest => parse_manifest_item(&element, &mut manifest)?,
                    b"itemref" if in_spine => parse_spine_item(&element, &mut spine)?,
                    _ => {}
                }
            }
            Ok(Event::Empty(element)) => {
                let name = element.name();
                match local_name(name.as_ref()) {
                    b"item" if in_manifest => parse_manifest_item(&element, &mut manifest)?,
                    b"itemref" if in_spine => parse_spine_item(&element, &mut spine)?,
                    _ => {}
                }
            }
            Ok(Event::End(element)) => {
                let name = element.name();
                match local_name(name.as_ref()) {
                    b"manifest" => in_manifest = false,
                    b"spine" => in_spine = false,
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("Could not parse EPUB package document: {error}")),
        }
    }
    if manifest.is_empty() {
        return Err("EPUB package manifest is empty.".into());
    }
    if spine.is_empty() {
        return Err("EPUB package spine is empty.".into());
    }
    Ok(PackageDocument { manifest, spine })
}

fn parse_manifest_item(
    element: &BytesStart<'_>,
    manifest: &mut HashMap<String, ManifestItem>,
) -> Result<(), String> {
    let Some(id) = attribute_value(element, b"id")? else {
        return Ok(());
    };
    let Some(href) = attribute_value(element, b"href")? else {
        return Ok(());
    };
    let media_type = attribute_value(element, b"media-type")?.unwrap_or_default();
    let properties = attribute_value(element, b"properties")?.unwrap_or_default();
    manifest.insert(
        id,
        ManifestItem {
            href,
            media_type,
            is_navigation: properties.split_whitespace().any(|value| value == "nav"),
        },
    );
    Ok(())
}

fn parse_spine_item(element: &BytesStart<'_>, spine: &mut Vec<String>) -> Result<(), String> {
    if attribute_value(element, b"linear")?
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case("no"))
    {
        return Ok(());
    }
    if let Some(idref) = attribute_value(element, b"idref")? {
        spine.push(idref);
    }
    Ok(())
}

fn extract_image_references(xml: &str) -> Result<Vec<RawImageReference>, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut references = Vec::new();
    let mut svg_stack = Vec::<SvgContext>::new();
    let mut active_svg_hint_depth = None::<usize>;
    let mut depth = 0usize;
    let mut next_ordinal = 0usize;

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                depth += 1;
                let name = element.name();
                let name = local_name(name.as_ref());
                if name == b"svg" {
                    svg_stack.push(SvgContext::default());
                } else if name == b"img" {
                    if let Some(src) = attribute_value(&element, b"src")? {
                        let mut hints = Vec::new();
                        push_attribute_hint(&mut hints, &element, b"alt")?;
                        push_attribute_hint(&mut hints, &element, b"title")?;
                        references.push(RawImageReference {
                            href: src,
                            image_ordinal: next_ordinal,
                            embedded_text: hints,
                        });
                        next_ordinal += 1;
                    }
                } else if name == b"image" && !svg_stack.is_empty() {
                    if let Some(href) = attribute_value(&element, b"href")? {
                        let mut hints = Vec::new();
                        push_attribute_hint(&mut hints, &element, b"title")?;
                        svg_stack.last_mut().unwrap().images.push((
                            href,
                            next_ordinal,
                            hints,
                        ));
                        next_ordinal += 1;
                    }
                } else if !svg_stack.is_empty()
                    && matches!(name, b"title" | b"desc" | b"text")
                    && active_svg_hint_depth.is_none()
                {
                    active_svg_hint_depth = Some(depth);
                }
            }
            Ok(Event::Empty(element)) => {
                let name = element.name();
                let name = local_name(name.as_ref());
                if name == b"img" {
                    if let Some(src) = attribute_value(&element, b"src")? {
                        let mut hints = Vec::new();
                        push_attribute_hint(&mut hints, &element, b"alt")?;
                        push_attribute_hint(&mut hints, &element, b"title")?;
                        references.push(RawImageReference {
                            href: src,
                            image_ordinal: next_ordinal,
                            embedded_text: hints,
                        });
                        next_ordinal += 1;
                    }
                } else if name == b"image" && !svg_stack.is_empty() {
                    if let Some(href) = attribute_value(&element, b"href")? {
                        let mut hints = Vec::new();
                        push_attribute_hint(&mut hints, &element, b"title")?;
                        svg_stack.last_mut().unwrap().images.push((
                            href,
                            next_ordinal,
                            hints,
                        ));
                        next_ordinal += 1;
                    }
                }
            }
            Ok(Event::Text(text)) if active_svg_hint_depth.is_some() && !svg_stack.is_empty() => {
                let decoded = text
                    .decode()
                    .map_err(|error| format!("Could not decode EPUB SVG text hint: {error}"))?;
                let decoded = unescape(&decoded)
                    .map_err(|error| format!("Could not unescape EPUB SVG text hint: {error}"))?;
                push_hint(&mut svg_stack.last_mut().unwrap().hints, &decoded);
            }
            Ok(Event::CData(text)) if active_svg_hint_depth.is_some() && !svg_stack.is_empty() => {
                let decoded = text
                    .decode()
                    .map_err(|error| format!("Could not decode EPUB SVG CDATA hint: {error}"))?;
                push_hint(&mut svg_stack.last_mut().unwrap().hints, &decoded);
            }
            Ok(Event::End(element)) => {
                let name = element.name();
                let name = local_name(name.as_ref());
                if active_svg_hint_depth == Some(depth) {
                    active_svg_hint_depth = None;
                }
                if name == b"svg" {
                    let context = svg_stack
                        .pop()
                        .ok_or("EPUB XHTML closed an SVG element without opening one.")?;
                    for (href, image_ordinal, mut hints) in context.images {
                        for hint in &context.hints {
                            push_hint(&mut hints, hint);
                        }
                        references.push(RawImageReference {
                            href,
                            image_ordinal,
                            embedded_text: hints,
                        });
                    }
                }
                depth = depth.saturating_sub(1);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("Could not parse EPUB XHTML for images: {error}")),
        }
    }
    references.sort_by_key(|reference| reference.image_ordinal);
    Ok(references)
}

fn push_attribute_hint(
    hints: &mut Vec<String>,
    element: &BytesStart<'_>,
    name: &[u8],
) -> Result<(), String> {
    if let Some(value) = attribute_value(element, name)? {
        push_hint(hints, &value);
    }
    Ok(())
}

fn push_hint(hints: &mut Vec<String>, value: &str) {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() || normalized.chars().count() > MAX_HINT_CHARS {
        return;
    }
    if hints
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&normalized))
    {
        return;
    }
    hints.push(normalized);
}

fn image_entry_size<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    href: &str,
) -> Result<Option<u64>, String> {
    match archive.by_name(href) {
        Ok(entry) => {
            if !entry.is_file() || entry.size() == 0 || entry.size() > MAX_IMAGE_BYTES {
                Ok(None)
            } else {
                Ok(Some(entry.size()))
            }
        }
        Err(ZipError::FileNotFound) => Ok(None),
        Err(error) => Err(format!("Could not inspect EPUB image {href}: {error}")),
    }
}

fn resolve_optional_resource_href(base: &str, href: &str) -> Result<Option<String>, String> {
    let href = href.split(['#', '?']).next().unwrap_or(href).trim();
    if href.is_empty()
        || href.starts_with('/')
        || href.starts_with("data:")
        || href.contains("://")
    {
        return Ok(None);
    }
    resolve_archive_href(base, href).map(Some)
}

fn resolve_archive_href(base: &str, href: &str) -> Result<String, String> {
    let href = href.split(['#', '?']).next().unwrap_or(href);
    if href.contains("://") || href.starts_with("data:") || href.starts_with('/') {
        return Err(format!("EPUB resource uses unsupported URI {href}."));
    }
    let decoded = percent_decode(href)?;
    let combined = if base.is_empty() {
        decoded
    } else {
        format!("{base}/{decoded}")
    };
    normalize_archive_path(&combined)
}

fn normalize_archive_path(path: &str) -> Result<String, String> {
    let path = path.replace('\\', "/");
    if path.starts_with('/') {
        return Err(format!("EPUB archive path must be relative: {path}"));
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(format!("EPUB archive path escapes its root: {path}"));
                }
            }
            value => parts.push(value),
        }
    }
    if parts.is_empty() {
        return Err(format!("EPUB archive path is empty after normalization: {path}"));
    }
    Ok(parts.join("/"))
}

fn percent_decode(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return Err(format!("EPUB URI contains an incomplete percent escape: {value}"));
        }
        let high = hex_value(bytes[index + 1])
            .ok_or_else(|| format!("EPUB URI contains an invalid percent escape: {value}"))?;
        let low = hex_value(bytes[index + 2])
            .ok_or_else(|| format!("EPUB URI contains an invalid percent escape: {value}"))?;
        decoded.push((high << 4) | low);
        index += 3;
    }
    String::from_utf8(decoded).map_err(|error| format!("EPUB URI is not UTF-8: {error}"))
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn parent_archive_path(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default()
}

fn attribute_value(element: &BytesStart<'_>, wanted: &[u8]) -> Result<Option<String>, String> {
    for attribute in element.attributes().with_checks(false) {
        let attribute =
            attribute.map_err(|error| format!("Invalid EPUB XML attribute: {error}"))?;
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

fn read_zip_text<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
    max_bytes: u64,
    cancellation: &CancellationToken,
) -> Result<String, String> {
    if cancellation.is_requested() {
        return Err("EPUB image candidate discovery was cancelled.".into());
    }
    let mut entry = archive
        .by_name(name)
        .map_err(|error| format!("EPUB entry {name} is unavailable: {error}"))?;
    if !entry.is_file() {
        return Err(format!("EPUB entry {name} is not a regular file."));
    }
    if entry.size() > max_bytes {
        return Err(format!("EPUB entry {name} exceeds its image-review extraction limit."));
    }
    let mut bytes = Vec::with_capacity(entry.size().min(max_bytes) as usize);
    let mut buffer = vec![0u8; READ_BUFFER_BYTES];
    loop {
        if cancellation.is_requested() {
            return Err("EPUB image candidate discovery was cancelled.".into());
        }
        let count = entry
            .read(&mut buffer)
            .map_err(|error| format!("Could not read EPUB entry {name}: {error}"))?;
        if count == 0 {
            break;
        }
        if bytes.len().saturating_add(count) > max_bytes as usize {
            return Err(format!("EPUB entry {name} expanded beyond its image-review extraction limit."));
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    String::from_utf8(bytes).map_err(|error| format!("EPUB entry {name} is not UTF-8: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AlignmentSegment, CorpusPosition, EpubSection};
    use std::{fs, io::Write, path::PathBuf};
    use uuid::Uuid;
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    fn temp_epub() -> PathBuf {
        let root = std::env::temp_dir().join(format!("storyteller-review-images-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("book.epub");
        let file = File::create(&path).unwrap();
        let mut zip = ZipWriter::new(file);
        let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("mimetype", SimpleFileOptions::default().compression_method(CompressionMethod::Stored)).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        zip.start_file("OPS/package.opf", deflated).unwrap();
        zip.write_all(br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata/><manifest><item id="c1" href="Text/ch1.xhtml" media-type="application/xhtml+xml"/><item id="fig" href="Text/figure.xhtml" media-type="application/xhtml+xml"/><item id="c3" href="Text/ch3.xhtml" media-type="application/xhtml+xml"/><item id="diagram" href="Images/diagram.png" media-type="image/png"/><item id="map" href="Images/map.png" media-type="image/png"/></manifest><spine><itemref idref="c1"/><itemref idref="fig"/><itemref idref="c3"/></spine></package>"#).unwrap();
        zip.start_file("OPS/Text/ch1.xhtml", deflated).unwrap();
        zip.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Opening text.</p></body></html>"#).unwrap();
        zip.start_file("OPS/Text/figure.xhtml", deflated).unwrap();
        zip.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><img src="../Images/diagram.png" alt="Family tree" title="Figure two"/></body></html>"#).unwrap();
        zip.start_file("OPS/Text/ch3.xhtml", deflated).unwrap();
        zip.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Closing text.</p><svg xmlns="http://www.w3.org/2000/svg"><title>Map title</title><desc>Map description</desc><text>North wing</text><image href="../Images/map.png"/></svg></body></html>"#).unwrap();
        zip.start_file("OPS/Images/diagram.png", deflated).unwrap();
        zip.write_all(b"diagram-bytes").unwrap();
        zip.start_file("OPS/Images/map.png", deflated).unwrap();
        zip.write_all(b"map-bytes").unwrap();
        zip.finish().unwrap();
        path
    }

    fn corpus() -> EpubCorpus {
        EpubCorpus {
            package_path: "OPS/package.opf".into(),
            sections: vec![
                EpubSection {
                    href: "OPS/Text/ch1.xhtml".into(),
                    text: "Opening text.".into(),
                },
                EpubSection {
                    href: "OPS/Text/ch3.xhtml".into(),
                    text: "Closing text.".into(),
                },
            ],
        }
    }

    fn matched_segment(start: u64, end: u64, text: &str, href: &str) -> AlignmentSegment {
        let position = CorpusPosition {
            href: href.into(),
            line_index: 0,
            char_offset: 0,
        };
        AlignmentSegment {
            audio_start_ms: start,
            audio_end_ms: end,
            transcript_text: text.into(),
            status: AlignmentStatus::Matched,
            match_percent: Some(100.0),
            book_start: Some(position.clone()),
            book_end: Some(position),
        }
    }

    fn alignment() -> AlignmentDocument {
        AlignmentDocument {
            algorithm: "test".into(),
            language: Some("en".into()),
            total_segments: 3,
            matched_segments: 2,
            match_percent: 66.666,
            segments: vec![
                matched_segment(0, 1000, "opening", "OPS/Text/ch1.xhtml"),
                AlignmentSegment {
                    audio_start_ms: 1000,
                    audio_end_ms: 2000,
                    transcript_text: "family tree diagram".into(),
                    status: AlignmentStatus::Unmatched,
                    match_percent: None,
                    book_start: None,
                    book_end: None,
                },
                matched_segment(2000, 3000, "closing", "OPS/Text/ch3.xhtml"),
            ],
        }
    }

    #[test]
    fn discovery_finds_image_only_spine_document_nearest_the_review_segment() {
        let epub = temp_epub();
        let candidates = review_image_candidates(
            &epub,
            &alignment(),
            &corpus(),
            1,
            1,
            24,
            &CancellationToken::default(),
        )
        .unwrap();
        assert_eq!(candidates.len(), 1);
        let candidate = &candidates[0];
        assert_eq!(candidate.document_href, "OPS/Text/figure.xhtml");
        assert_eq!(candidate.image_href, "OPS/Images/diagram.png");
        assert_eq!(candidate.media_type, "image/png");
        assert_eq!(candidate.document_spine_index, 1);
        assert_eq!(candidate.image_ordinal, 0);
        assert_eq!(candidate.embedded_text, vec!["Family tree", "Figure two"]);
        assert!(candidate.byte_size > 0);
        let _ = fs::remove_dir_all(epub.parent().unwrap());
    }

    #[test]
    fn discovery_collects_svg_text_hints_before_ocr() {
        let epub = temp_epub();
        let candidates = review_image_candidates(
            &epub,
            &alignment(),
            &corpus(),
            1,
            3,
            2,
            &CancellationToken::default(),
        )
        .unwrap();
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].image_href, "OPS/Images/diagram.png");
        let map = candidates
            .iter()
            .find(|candidate| candidate.image_href == "OPS/Images/map.png")
            .unwrap();
        assert_eq!(
            map.embedded_text,
            vec!["Map title", "Map description", "North wing"]
        );
        let _ = fs::remove_dir_all(epub.parent().unwrap());
    }

    #[test]
    fn discovery_honors_cancellation_and_limits() {
        let epub = temp_epub();
        let cancellation = CancellationToken::default();
        cancellation.cancel();
        assert!(review_image_candidates(
            &epub,
            &alignment(),
            &corpus(),
            1,
            12,
            24,
            &cancellation,
        )
        .unwrap_err()
        .contains("cancelled"));
        assert!(review_image_candidates(
            &epub,
            &alignment(),
            &corpus(),
            1,
            0,
            24,
            &CancellationToken::default(),
        )
        .is_err());
        let _ = fs::remove_dir_all(epub.parent().unwrap());
    }
}
