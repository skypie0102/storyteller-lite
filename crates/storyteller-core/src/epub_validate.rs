use crate::{
    copy_file_cancellable,
    epub_overlay::{parent_archive_path, resolve_archive_href},
    CancellationToken,
};
use quick_xml::{
    escape::unescape,
    events::{BytesStart, Event},
    Reader,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{Read, Seek},
    path::{Path, PathBuf},
};
use zip::{CompressionMethod, ZipArchive};

const MAX_XML_BYTES: usize = 32 * 1024 * 1024;
const READ_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpubValidationSummary {
    pub overlay_count: usize,
    pub synchronized_segments: usize,
    pub media_duration_ms: u64,
}

#[derive(Debug, Clone)]
struct ManifestItem {
    id: String,
    archive_path: String,
    media_type: String,
    media_overlay: Option<String>,
}

#[derive(Debug)]
struct PackageAudit {
    items: HashMap<String, ManifestItem>,
    total_duration_ms: u64,
    refined_duration_ms: HashMap<String, u64>,
}

pub fn validate_readaloud_epub(
    path: &Path,
    cancellation: &CancellationToken,
) -> Result<EpubValidationSummary, String> {
    if cancellation.is_requested() {
        return Err("EPUB validation was cancelled.".into());
    }
    let file = File::open(path)
        .map_err(|error| format!("Could not open built EPUB {}: {error}", path.display()))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| format!("Built EPUB is not a readable ZIP container: {error}"))?;
    if archive.is_empty() {
        return Err("Built EPUB ZIP container is empty.".into());
    }
    validate_first_mimetype(&mut archive)?;
    validate_unique_entries(&mut archive)?;

    let container_xml = read_archive_text(
        &mut archive,
        "META-INF/container.xml",
        cancellation,
    )?;
    let package_path = parse_container_package_path(&container_xml)?;
    let package_xml = read_archive_text(&mut archive, &package_path, cancellation)?;
    let package = audit_package(&package_xml, &package_path)?;

    let overlay_items = package
        .items
        .values()
        .filter(|item| item.media_type == "application/xhtml+xml" && item.media_overlay.is_some())
        .cloned()
        .collect::<Vec<_>>();
    if overlay_items.is_empty() {
        return Err("Built EPUB package has no synchronized XHTML manifest items.".into());
    }

    let mut xhtml_ids = HashMap::<String, HashSet<String>>::new();
    let mut synchronized_segments = 0usize;
    let mut computed_duration_ms = 0u64;
    let mut seen_overlay_ids = HashSet::new();

    for xhtml_item in overlay_items {
        if cancellation.is_requested() {
            return Err("EPUB validation was cancelled.".into());
        }
        let overlay_id = xhtml_item
            .media_overlay
            .as_deref()
            .ok_or("Synchronized XHTML item is missing media-overlay.")?;
        if !seen_overlay_ids.insert(overlay_id.to_string()) {
            return Err(format!("Media Overlay item {overlay_id} is associated more than once."));
        }
        let smil_item = package.items.get(overlay_id).ok_or_else(|| {
            format!("Media Overlay manifest item {overlay_id} does not exist.")
        })?;
        if smil_item.media_type != "application/smil+xml" {
            return Err(format!(
                "Media Overlay item {overlay_id} has media type {} instead of application/smil+xml.",
                smil_item.media_type
            ));
        }
        ensure_nonempty_archive_entry(&mut archive, &xhtml_item.archive_path)?;
        ensure_nonempty_archive_entry(&mut archive, &smil_item.archive_path)?;

        let smil_xml = read_archive_text(&mut archive, &smil_item.archive_path, cancellation)?;
        let smil_dir = parent_archive_path(&smil_item.archive_path);
        let audit = audit_smil(
            &smil_xml,
            &smil_dir,
            &mut archive,
            &mut xhtml_ids,
            cancellation,
        )?;
        if audit.segment_count == 0 {
            return Err(format!("Media Overlay {} contains no synchronized par elements.", smil_item.archive_path));
        }
        let declared = package.refined_duration_ms.get(overlay_id).copied().ok_or_else(|| {
            format!("Media Overlay item {overlay_id} is missing refined media:duration metadata.")
        })?;
        if declared != audit.duration_ms {
            return Err(format!(
                "Media Overlay {overlay_id} duration metadata ({declared} ms) does not match its clips ({} ms).",
                audit.duration_ms
            ));
        }
        synchronized_segments = synchronized_segments
            .checked_add(audit.segment_count)
            .ok_or("Synchronized segment count overflowed.")?;
        computed_duration_ms = computed_duration_ms
            .checked_add(audit.duration_ms)
            .ok_or("Media Overlay duration overflowed.")?;
    }

    if package.total_duration_ms != computed_duration_ms {
        return Err(format!(
            "Total media:duration metadata ({} ms) does not match Media Overlay clips ({computed_duration_ms} ms).",
            package.total_duration_ms
        ));
    }

    Ok(EpubValidationSummary {
        overlay_count: seen_overlay_ids.len(),
        synchronized_segments,
        media_duration_ms: computed_duration_ms,
    })
}

pub fn write_validation_report(
    path: &Path,
    summary: EpubValidationSummary,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!("Could not create validation report directory {}: {error}", parent.display())
        })?;
    }
    let json = serde_json::to_vec_pretty(&summary)
        .map_err(|error| format!("Could not serialize EPUB validation report: {error}"))?;
    fs::write(path, json)
        .map_err(|error| format!("Could not write EPUB validation report {}: {error}", path.display()))
}

pub fn publish_validated_epub(
    candidate: &Path,
    destination: &Path,
    cancellation: &CancellationToken,
) -> Result<(), String> {
    if candidate == destination {
        return Err("Validated EPUB candidate and publication destination must differ.".into());
    }
    if destination.exists() {
        return Err(format!(
            "Output EPUB already exists and will not be overwritten: {}",
            destination.display()
        ));
    }
    let parent = destination
        .parent()
        .ok_or("Output EPUB path has no parent directory.")?;
    fs::create_dir_all(parent).map_err(|error| {
        format!("Could not create output directory {}: {error}", parent.display())
    })?;
    let temp = publication_temp_path(destination)?;
    if temp.exists() {
        fs::remove_file(&temp).map_err(|error| {
            format!("Could not remove stale publication temporary file {}: {error}", temp.display())
        })?;
    }
    let result = (|| {
        copy_file_cancellable(candidate, &temp, cancellation)?;
        if cancellation.is_requested() {
            return Err("EPUB publication was cancelled.".into());
        }
        fs::rename(&temp, destination).map_err(|error| {
            format!(
                "Could not publish validated EPUB from {} to {}: {error}",
                temp.display(),
                destination.display()
            )
        })
    })();
    if result.is_err() && temp.exists() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[derive(Debug, Clone, Copy)]
struct SmilAudit {
    segment_count: usize,
    duration_ms: u64,
}

fn audit_smil<R: Read + Seek>(
    xml: &str,
    smil_dir: &str,
    archive: &mut ZipArchive<R>,
    xhtml_ids: &mut HashMap<String, HashSet<String>>,
    cancellation: &CancellationToken,
) -> Result<SmilAudit, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut in_par = false;
    let mut text_src = None::<String>;
    let mut audio_src = None::<String>;
    let mut clip_begin = None::<u64>;
    let mut clip_end = None::<u64>;
    let mut segment_count = 0usize;
    let mut duration_ms = 0u64;

    loop {
        if cancellation.is_requested() {
            return Err("EPUB validation was cancelled.".into());
        }
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let qname = element.name();
                let name = local_name(qname.as_ref());
                if name == b"par" {
                    if in_par {
                        return Err("Media Overlay contains nested par elements.".into());
                    }
                    in_par = true;
                    text_src = None;
                    audio_src = None;
                    clip_begin = None;
                    clip_end = None;
                } else if name == b"text" && in_par {
                    text_src = attribute_value(&element, b"src")?;
                } else if name == b"audio" && in_par {
                    audio_src = attribute_value(&element, b"src")?;
                    clip_begin = attribute_value(&element, b"clipBegin")?
                        .as_deref()
                        .map(parse_clock)
                        .transpose()?;
                    clip_end = attribute_value(&element, b"clipEnd")?
                        .as_deref()
                        .map(parse_clock)
                        .transpose()?;
                }
            }
            Ok(Event::Empty(element)) => {
                let qname = element.name();
                let name = local_name(qname.as_ref());
                if name == b"text" && in_par {
                    text_src = attribute_value(&element, b"src")?;
                } else if name == b"audio" && in_par {
                    audio_src = attribute_value(&element, b"src")?;
                    clip_begin = attribute_value(&element, b"clipBegin")?
                        .as_deref()
                        .map(parse_clock)
                        .transpose()?;
                    clip_end = attribute_value(&element, b"clipEnd")?
                        .as_deref()
                        .map(parse_clock)
                        .transpose()?;
                }
            }
            Ok(Event::End(element)) => {
                let qname = element.name();
                if local_name(qname.as_ref()) == b"par" {
                    if !in_par {
                        return Err("Media Overlay par nesting is invalid.".into());
                    }
                    let text_src = text_src.take().ok_or("Media Overlay par is missing text src.")?;
                    let audio_src = audio_src.take().ok_or("Media Overlay par is missing audio src.")?;
                    let begin = clip_begin.take().ok_or("Media Overlay par is missing clipBegin.")?;
                    let end = clip_end.take().ok_or("Media Overlay par is missing clipEnd.")?;
                    if end <= begin {
                        return Err("Media Overlay par has an invalid audio clip range.".into());
                    }
                    validate_text_target(&text_src, smil_dir, archive, xhtml_ids, cancellation)?;
                    validate_audio_target(&audio_src, smil_dir, archive)?;
                    duration_ms = duration_ms
                        .checked_add(end - begin)
                        .ok_or("Media Overlay duration overflowed.")?;
                    segment_count = segment_count
                        .checked_add(1)
                        .ok_or("Media Overlay segment count overflowed.")?;
                    in_par = false;
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("Could not parse Media Overlay SMIL: {error}")),
        }
    }
    if in_par {
        return Err("Media Overlay ended inside a par element.".into());
    }
    Ok(SmilAudit {
        segment_count,
        duration_ms,
    })
}

fn validate_text_target<R: Read + Seek>(
    src: &str,
    smil_dir: &str,
    archive: &mut ZipArchive<R>,
    cache: &mut HashMap<String, HashSet<String>>,
    cancellation: &CancellationToken,
) -> Result<(), String> {
    let (path_part, fragment) = src
        .split_once('#')
        .ok_or("Media Overlay text src is missing a fragment identifier.")?;
    let archive_path = resolve_archive_href(smil_dir, path_part)?;
    let fragment = percent_decode(fragment)?;
    if fragment.trim().is_empty() {
        return Err("Media Overlay text src has an empty fragment identifier.".into());
    }
    if !cache.contains_key(&archive_path) {
        let xml = read_archive_text(archive, &archive_path, cancellation)?;
        cache.insert(archive_path.clone(), collect_ids(&xml)?);
    }
    let ids = cache
        .get(&archive_path)
        .ok_or("Could not cache synchronized XHTML IDs.")?;
    if !ids.contains(&fragment) {
        return Err(format!(
            "Media Overlay text target {archive_path}#{fragment} does not exist."
        ));
    }
    Ok(())
}

fn validate_audio_target<R: Read + Seek>(
    src: &str,
    smil_dir: &str,
    archive: &mut ZipArchive<R>,
) -> Result<(), String> {
    let archive_path = resolve_archive_href(smil_dir, src)?;
    ensure_nonempty_archive_entry(archive, &archive_path)
}

fn collect_ids(xml: &str) -> Result<HashSet<String>, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut ids = HashSet::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) | Ok(Event::Empty(element)) => {
                if let Some(id) = attribute_value(&element, b"id")?.filter(|id| !id.trim().is_empty()) {
                    if !ids.insert(id.clone()) {
                        return Err(format!("Synchronized XHTML contains duplicate id {id}."));
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("Could not parse synchronized XHTML: {error}")),
        }
    }
    Ok(ids)
}

fn audit_package(xml: &str, package_path: &str) -> Result<PackageAudit, String> {
    let package_dir = parent_archive_path(package_path);
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut items = HashMap::<String, ManifestItem>::new();
    let mut current_duration = None::<Option<String>>;
    let mut duration_text = String::new();
    let mut total_duration_ms = None;
    let mut refined_duration_ms = HashMap::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let qname = element.name();
                let name = local_name(qname.as_ref());
                if name == b"item" {
                    insert_manifest_item(&element, &package_dir, &mut items)?;
                } else if name == b"meta"
                    && attribute_value(&element, b"property")?.as_deref() == Some("media:duration")
                {
                    let refines = attribute_value(&element, b"refines")?
                        .map(|value| value.trim_start_matches('#').to_string());
                    current_duration = Some(refines);
                    duration_text.clear();
                }
            }
            Ok(Event::Empty(element)) => {
                let qname = element.name();
                if local_name(qname.as_ref()) == b"item" {
                    insert_manifest_item(&element, &package_dir, &mut items)?;
                }
            }
            Ok(Event::Text(text)) if current_duration.is_some() => {
                let decoded = text
                    .decode()
                    .map_err(|error| format!("Could not decode package metadata text: {error}"))?;
                duration_text.push_str(&decoded);
            }
            Ok(Event::End(element)) => {
                let qname = element.name();
                if local_name(qname.as_ref()) == b"meta" {
                    if let Some(refines) = current_duration.take() {
                        let duration = parse_clock(duration_text.trim())?;
                        if let Some(id) = refines {
                            if refined_duration_ms.insert(id.clone(), duration).is_some() {
                                return Err(format!("Duplicate media:duration metadata for #{id}."));
                            }
                        } else if total_duration_ms.replace(duration).is_some() {
                            return Err("EPUB package contains duplicate total media:duration metadata.".into());
                        }
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("Could not parse built EPUB package: {error}")),
        }
    }
    if items.is_empty() {
        return Err("Built EPUB package manifest is empty.".into());
    }
    Ok(PackageAudit {
        items,
        total_duration_ms: total_duration_ms
            .ok_or("Built EPUB package is missing total media:duration metadata.")?,
        refined_duration_ms,
    })
}

fn insert_manifest_item(
    element: &BytesStart<'_>,
    package_dir: &str,
    items: &mut HashMap<String, ManifestItem>,
) -> Result<(), String> {
    let id = attribute_value(element, b"id")?.ok_or("EPUB manifest item is missing id.")?;
    let href = attribute_value(element, b"href")?.ok_or("EPUB manifest item is missing href.")?;
    let media_type = attribute_value(element, b"media-type")?
        .ok_or("EPUB manifest item is missing media-type.")?;
    let archive_path = resolve_archive_href(package_dir, &href)?;
    let item = ManifestItem {
        id: id.clone(),
        archive_path,
        media_type,
        media_overlay: attribute_value(element, b"media-overlay")?,
    };
    if items.insert(id.clone(), item).is_some() {
        return Err(format!("EPUB manifest contains duplicate id {id}."));
    }
    Ok(())
}

fn parse_container_package_path(xml: &str) -> Result<String, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) | Ok(Event::Empty(element)) => {
                let qname = element.name();
                if local_name(qname.as_ref()) == b"rootfile" {
                    let path = attribute_value(&element, b"full-path")?
                        .ok_or("EPUB container rootfile is missing full-path.")?;
                    return resolve_archive_href("", &path);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("Could not parse EPUB container.xml: {error}")),
        }
    }
    Err("EPUB container.xml does not declare a package document.".into())
}

fn validate_first_mimetype<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<(), String> {
    let mut first = archive
        .by_index(0)
        .map_err(|error| format!("Could not read first EPUB ZIP entry: {error}"))?;
    if first.name() != "mimetype" {
        return Err("EPUB mimetype entry is not the first ZIP entry.".into());
    }
    if first.compression() != CompressionMethod::Stored {
        return Err("EPUB mimetype entry must be stored without compression.".into());
    }
    let mut value = Vec::new();
    first
        .read_to_end(&mut value)
        .map_err(|error| format!("Could not read EPUB mimetype entry: {error}"))?;
    if value != b"application/epub+zip" {
        return Err("EPUB mimetype entry does not equal application/epub+zip.".into());
    }
    Ok(())
}

fn validate_unique_entries<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<(), String> {
    let mut names = HashSet::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("Could not inspect EPUB ZIP entry {index}: {error}"))?;
        let name = entry.name().to_string();
        if !names.insert(name.clone()) {
            return Err(format!("EPUB ZIP contains duplicate entry {name}."));
        }
    }
    Ok(())
}

fn ensure_nonempty_archive_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<(), String> {
    let entry = archive
        .by_name(name)
        .map_err(|error| format!("EPUB resource {name} is unavailable: {error}"))?;
    if !entry.is_file() || entry.size() == 0 {
        return Err(format!("EPUB resource {name} is not a non-empty regular file."));
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
        .map_err(|error| format!("EPUB XML resource {name} is unavailable: {error}"))?;
    if !entry.is_file() || entry.size() == 0 {
        return Err(format!("EPUB XML resource {name} is not a non-empty regular file."));
    }
    if entry.size() > MAX_XML_BYTES as u64 {
        return Err(format!("EPUB XML resource {name} exceeds the 32 MiB validation limit."));
    }
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    let mut buffer = vec![0u8; READ_BUFFER_BYTES];
    loop {
        if cancellation.is_requested() {
            return Err("EPUB validation was cancelled.".into());
        }
        let count = entry
            .read(&mut buffer)
            .map_err(|error| format!("Could not read EPUB XML resource {name}: {error}"))?;
        if count == 0 {
            break;
        }
        if bytes.len().saturating_add(count) > MAX_XML_BYTES {
            return Err(format!("EPUB XML resource {name} expanded beyond the validation limit."));
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    String::from_utf8(bytes).map_err(|error| format!("EPUB XML resource {name} is not UTF-8: {error}"))
}

fn parse_clock(value: &str) -> Result<u64, String> {
    let mut parts = value.split(':').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err(format!("Invalid Media Overlay clock value: {value}"));
    }
    let seconds_part = parts.pop().expect("clock has three parts");
    let minutes = parts
        .pop()
        .expect("clock has minutes")
        .parse::<u64>()
        .map_err(|_| format!("Invalid Media Overlay clock value: {value}"))?;
    let hours = parts
        .pop()
        .expect("clock has hours")
        .parse::<u64>()
        .map_err(|_| format!("Invalid Media Overlay clock value: {value}"))?;
    if minutes >= 60 {
        return Err(format!("Invalid Media Overlay clock value: {value}"));
    }
    let (seconds, millis) = if let Some((seconds, fraction)) = seconds_part.split_once('.') {
        let seconds = seconds
            .parse::<u64>()
            .map_err(|_| format!("Invalid Media Overlay clock value: {value}"))?;
        let mut fraction = fraction.chars().take(3).collect::<String>();
        while fraction.len() < 3 {
            fraction.push('0');
        }
        let millis = fraction
            .parse::<u64>()
            .map_err(|_| format!("Invalid Media Overlay clock value: {value}"))?;
        (seconds, millis)
    } else {
        let seconds = seconds_part
            .parse::<u64>()
            .map_err(|_| format!("Invalid Media Overlay clock value: {value}"))?;
        (seconds, 0)
    };
    if seconds >= 60 {
        return Err(format!("Invalid Media Overlay clock value: {value}"));
    }
    hours
        .checked_mul(3_600_000)
        .and_then(|value| value.checked_add(minutes * 60_000))
        .and_then(|value| value.checked_add(seconds * 1000))
        .and_then(|value| value.checked_add(millis))
        .ok_or_else(|| format!("Media Overlay clock value is too large: {value}"))
}

fn percent_decode(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(format!("URI has incomplete percent escape: {value}"));
            }
            let high = hex(bytes[index + 1])
                .ok_or_else(|| format!("URI has invalid percent escape: {value}"))?;
            let low = hex(bytes[index + 2])
                .ok_or_else(|| format!("URI has invalid percent escape: {value}"))?;
            output.push((high << 4) | low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).map_err(|error| format!("URI fragment is not UTF-8: {error}"))
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn publication_temp_path(destination: &Path) -> Result<PathBuf, String> {
    let file_name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("Output EPUB filename is not valid UTF-8.")?;
    Ok(destination.with_file_name(format!(".{file_name}.storyteller.tmp")))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_parser_accepts_builder_clock_shape() {
        assert_eq!(parse_clock("1:02:03.045").unwrap(), 3_723_045);
        assert_eq!(parse_clock("0:00:00.250").unwrap(), 250);
        assert!(parse_clock("0:61:00.000").is_err());
    }

    #[test]
    fn publication_temp_stays_next_to_output() {
        let output = Path::new("books/Novel (readaloud).epub");
        assert_eq!(
            publication_temp_path(output).unwrap(),
            PathBuf::from("books/.Novel (readaloud).epub.storyteller.tmp")
        );
    }
}
