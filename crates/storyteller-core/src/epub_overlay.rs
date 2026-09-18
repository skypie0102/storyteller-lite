use crate::{
    AlignmentSegment, AlignmentStatus, AudioReviewClassification, AudioReviewSupplementalPlacement,
};
use quick_xml::{
    escape::unescape,
    events::{BytesEnd, BytesStart, BytesText, Event},
    Reader, Writer,
};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub(crate) struct OverlaySectionSpec {
    pub href: String,
    pub xhtml: String,
    pub smil: String,
    pub smil_archive_path: String,
    pub smil_manifest_href: String,
    pub overlay_item_id: String,
    pub duration_ms: u64,
    pub synchronized_segments: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct SupplementalOverlaySpec {
    pub xhtml: String,
    pub xhtml_archive_path: String,
    pub xhtml_manifest_href: String,
    pub xhtml_item_id: String,
    pub smil: String,
    pub smil_archive_path: String,
    pub smil_manifest_href: String,
    pub overlay_item_id: String,
    pub anchor_item_id: String,
    pub placement: AudioReviewSupplementalPlacement,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct PackageScan {
    pub existing_ids: HashSet<String>,
    pub xhtml_item_ids: HashMap<String, String>,
}

pub(crate) fn scan_package(
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
    let mut existing_media_overlay = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) | Ok(Event::Empty(element)) => {
                let qname = element.name();
                let name = local_name(qname.as_ref());
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
                        let media_type =
                            attribute_value(&element, b"media-type")?.unwrap_or_default();
                        if media_type == "application/smil+xml"
                            || attribute_value(&element, b"media-overlay")?.is_some()
                        {
                            existing_media_overlay = true;
                        }
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
                } else if name == b"meta" {
                    if attribute_value(&element, b"property")?.as_deref() == Some("media:duration")
                    {
                        existing_media_overlay = true;
                    }
                    if let Some(id) = attribute_value(&element, b"id")? {
                        existing_ids.insert(id);
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
    if existing_media_overlay {
        return Err(
            "Source EPUB already contains Media Overlay metadata. Rebuilding an existing synchronized publication is not supported yet."
                .into(),
        );
    }

    Ok(PackageScan {
        existing_ids,
        xhtml_item_ids,
    })
}

pub(crate) fn rewrite_package(
    xml: &str,
    package_path: &str,
    scan: &PackageScan,
    sections: &[OverlaySectionSpec],
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
            Ok(Event::Start(mut element)) => {
                let qname = element.name();
                if local_name(qname.as_ref()) == b"item" {
                    maybe_add_media_overlay(&mut element, &package_dir, &overlays)?;
                }
                writer
                    .write_event(Event::Start(element.into_owned()))
                    .map_err(|error| format!("Could not rewrite EPUB package element: {error}"))?;
            }
            Ok(Event::Empty(mut element)) => {
                let qname = element.name();
                if local_name(qname.as_ref()) == b"item" {
                    maybe_add_media_overlay(&mut element, &package_dir, &overlays)?;
                }
                writer
                    .write_event(Event::Empty(element.into_owned()))
                    .map_err(|error| format!("Could not rewrite EPUB package element: {error}"))?;
            }
            Ok(Event::End(element)) => {
                let qname = element.name();
                let name = local_name(qname.as_ref());
                if name == b"manifest" {
                    for section in sections {
                        write_empty_item(
                            &mut writer,
                            &section.overlay_item_id,
                            &section.smil_manifest_href,
                            "application/smil+xml",
                        )?;
                    }
                    write_empty_item(
                        &mut writer,
                        audio_item_id,
                        audio_manifest_href,
                        audio_media_type,
                    )?;
                    wrote_manifest_additions = true;
                } else if name == b"metadata" {
                    for section in sections {
                        let refines = format!("#{}", section.overlay_item_id);
                        write_meta_duration(&mut writer, Some(&refines), section.duration_ms)?;
                    }
                    write_meta_duration(&mut writer, None, total_duration_ms)?;
                    wrote_duration_metadata = true;
                }
                writer
                    .write_event(Event::End(element.into_owned()))
                    .map_err(|error| format!("Could not close EPUB package element: {error}"))?;
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
            return Err(format!(
                "Missing package item for synchronized XHTML {}.",
                section.href
            ));
        }
    }
    String::from_utf8(writer.into_inner())
        .map_err(|error| format!("Rewritten EPUB package is not UTF-8: {error}"))
}

pub(crate) fn add_supplemental_package_items(
    xml: &str,
    supplements: &[SupplementalOverlaySpec],
) -> Result<String, String> {
    if supplements.is_empty() {
        return Ok(xml.to_string());
    }
    let mut before = HashMap::<&str, Vec<&SupplementalOverlaySpec>>::new();
    let mut after = HashMap::<&str, Vec<&SupplementalOverlaySpec>>::new();
    for supplement in supplements {
        match supplement.placement {
            AudioReviewSupplementalPlacement::BeforeAnchor => before
                .entry(supplement.anchor_item_id.as_str())
                .or_default()
                .push(supplement),
            AudioReviewSupplementalPlacement::AfterAnchor => after
                .entry(supplement.anchor_item_id.as_str())
                .or_default()
                .push(supplement),
        }
    }

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::new());
    let mut seen_anchors = HashSet::<String>::new();
    let mut start_itemref_after = None::<String>;
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let qname = element.name();
                if local_name(qname.as_ref()) == b"itemref" {
                    if let Some(idref) = attribute_value(&element, b"idref")? {
                        write_supplemental_itemrefs(&mut writer, before.get(idref.as_str()))?;
                        if before.contains_key(idref.as_str()) || after.contains_key(idref.as_str())
                        {
                            seen_anchors.insert(idref.clone());
                        }
                        if after.contains_key(idref.as_str()) {
                            start_itemref_after = Some(idref);
                        }
                    }
                }
                writer
                    .write_event(Event::Start(element.into_owned()))
                    .map_err(|error| format!("Could not rewrite EPUB package element: {error}"))?;
            }
            Ok(Event::Empty(element)) => {
                let qname = element.name();
                if local_name(qname.as_ref()) == b"itemref" {
                    if let Some(idref) = attribute_value(&element, b"idref")? {
                        write_supplemental_itemrefs(&mut writer, before.get(idref.as_str()))?;
                        writer
                            .write_event(Event::Empty(element.into_owned()))
                            .map_err(|error| {
                                format!("Could not rewrite EPUB package itemref: {error}")
                            })?;
                        write_supplemental_itemrefs(&mut writer, after.get(idref.as_str()))?;
                        if before.contains_key(idref.as_str()) || after.contains_key(idref.as_str())
                        {
                            seen_anchors.insert(idref);
                        }
                        continue;
                    }
                }
                writer
                    .write_event(Event::Empty(element.into_owned()))
                    .map_err(|error| format!("Could not rewrite EPUB package element: {error}"))?;
            }
            Ok(Event::End(element)) => {
                let qname = element.name();
                let name = local_name(qname.as_ref());
                let is_manifest = name == b"manifest";
                let is_metadata = name == b"metadata";
                let is_itemref = name == b"itemref";
                if is_manifest {
                    for supplement in supplements {
                        write_xhtml_overlay_item(
                            &mut writer,
                            &supplement.xhtml_item_id,
                            &supplement.xhtml_manifest_href,
                            &supplement.overlay_item_id,
                        )?;
                        write_empty_item(
                            &mut writer,
                            &supplement.overlay_item_id,
                            &supplement.smil_manifest_href,
                            "application/smil+xml",
                        )?;
                    }
                } else if is_metadata {
                    for supplement in supplements {
                        let refines = format!("#{}", supplement.overlay_item_id);
                        write_meta_duration(&mut writer, Some(&refines), supplement.duration_ms)?;
                    }
                }
                writer
                    .write_event(Event::End(element.into_owned()))
                    .map_err(|error| format!("Could not close EPUB package element: {error}"))?;
                if is_itemref {
                    if let Some(idref) = start_itemref_after.take() {
                        write_supplemental_itemrefs(&mut writer, after.get(idref.as_str()))?;
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(event) => writer
                .write_event(event.into_owned())
                .map_err(|error| format!("Could not rewrite EPUB package document: {error}"))?,
            Err(error) => return Err(format!("Could not parse EPUB package document: {error}")),
        }
    }
    for supplement in supplements {
        if !seen_anchors.contains(&supplement.anchor_item_id) {
            return Err(format!(
                "Supplemental page anchor {} is not present in the EPUB spine.",
                supplement.anchor_item_id
            ));
        }
    }
    String::from_utf8(writer.into_inner())
        .map_err(|error| format!("Rewritten EPUB package is not UTF-8: {error}"))
}

fn write_supplemental_itemrefs(
    writer: &mut Writer<Vec<u8>>,
    specs: Option<&Vec<&SupplementalOverlaySpec>>,
) -> Result<(), String> {
    let Some(specs) = specs else {
        return Ok(());
    };
    for spec in specs {
        let mut itemref = BytesStart::new("itemref");
        itemref.push_attribute(("idref", spec.xhtml_item_id.as_str()));
        writer
            .write_event(Event::Empty(itemref))
            .map_err(|error| format!("Could not add supplemental EPUB spine item: {error}"))?;
    }
    Ok(())
}

fn write_xhtml_overlay_item(
    writer: &mut Writer<Vec<u8>>,
    id: &str,
    href: &str,
    overlay_id: &str,
) -> Result<(), String> {
    let mut item = BytesStart::new("item");
    item.push_attribute(("id", id));
    item.push_attribute(("href", href));
    item.push_attribute(("media-type", "application/xhtml+xml"));
    item.push_attribute(("media-overlay", overlay_id));
    writer
        .write_event(Event::Empty(item))
        .map_err(|error| format!("Could not add supplemental XHTML manifest item: {error}"))
}

pub(crate) fn build_supplemental_xhtml(
    classification: AudioReviewClassification,
    transcript: &str,
    paragraph_id: &str,
) -> Result<String, String> {
    let (title, epub_type) = match classification {
        AudioReviewClassification::Introduction => ("Introduction", "frontmatter"),
        AudioReviewClassification::Credits => ("Credits", "backmatter"),
        _ => {
            return Err(
                "Only Introduction or Credits can use a supplemental read-aloud page.".into(),
            )
        }
    };
    if transcript.trim().is_empty() {
        return Err("Supplemental read-aloud page requires transcript text.".into());
    }
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" lang=\"en\"><head><title>{}</title><meta charset=\"utf-8\"/></head><body epub:type=\"{}\"><section><h1>{}</h1><p id=\"{}\">{}</p></section></body></html>",
        title,
        epub_type,
        title,
        escape_xml(paragraph_id),
        escape_xml(transcript.trim()),
    ))
}

pub(crate) fn build_supplemental_smil(
    classification: AudioReviewClassification,
    text_href: &str,
    paragraph_id: &str,
    audio_href: &str,
    start_ms: u64,
    end_ms: u64,
    sequence: usize,
) -> Result<(String, u64), String> {
    if end_ms <= start_ms {
        return Err("Supplemental audio segment has an invalid duration.".into());
    }
    let epub_type = match classification {
        AudioReviewClassification::Introduction => "frontmatter",
        AudioReviewClassification::Credits => "backmatter",
        _ => {
            return Err(
                "Only Introduction or Credits can use a supplemental read-aloud page.".into(),
            )
        }
    };
    let duration = end_ms - start_ms;
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<smil xmlns=\"http://www.w3.org/ns/SMIL\" xmlns:epub=\"http://www.idpf.org/2007/ops\" version=\"3.0\"><body><seq epub:type=\"{}\" epub:textref=\"{}\"><par id=\"stl-extra-par-{}\"><text src=\"{}#{}\"/><audio src=\"{}\" clipBegin=\"{}\" clipEnd=\"{}\"/></par></seq></body></smil>",
        epub_type,
        escape_xml(text_href),
        sequence + 1,
        escape_xml(text_href),
        escape_xml(&uri_fragment(paragraph_id)),
        escape_xml(audio_href),
        format_clock(start_ms),
        format_clock(end_ms),
    );
    Ok((xml, duration))
}

pub(crate) fn annotate_xhtml_blocks(
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
                let qname = element.name();
                let name = local_name(qname.as_ref());
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
                suppressed_depth = suppressed_depth.saturating_sub(1);
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

pub(crate) fn build_smil(
    segments: &[(usize, &AlignmentSegment)],
    anchors: &HashMap<usize, String>,
    text_href: &str,
    audio_href: &str,
    section_index: usize,
) -> Result<(String, u64), String> {
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<smil xmlns=\"http://www.w3.org/ns/SMIL\" xmlns:epub=\"http://www.idpf.org/2007/ops\" version=\"3.0\"><body><seq epub:textref=\"{}\">",
        escape_xml(text_href),
    );
    let mut duration_ms = 0u64;

    for (segment_index, segment) in segments {
        if segment.status != AlignmentStatus::Matched {
            continue;
        }
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
        if clip_duration == 0 {
            return Err("Matched alignment segment has zero audio duration.".into());
        }
        duration_ms = duration_ms
            .checked_add(clip_duration)
            .ok_or("Media Overlay duration overflowed.")?;
        let fragment = uri_fragment(anchor);
        xml.push_str(&format!(
            "<par id=\"stl-par-{}-{}\"><text src=\"{}#{}\"/><audio src=\"{}\" clipBegin=\"{}\" clipEnd=\"{}\"/></par>",
            section_index + 1,
            segment_index + 1,
            escape_xml(text_href),
            escape_xml(&fragment),
            escape_xml(audio_href),
            format_clock(segment.audio_start_ms),
            format_clock(segment.audio_end_ms),
        ));
    }
    xml.push_str("</seq></body></smil>");
    Ok((xml, duration_ms))
}

pub(crate) fn unique_id(base: &str, used: &mut HashSet<String>) -> String {
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

pub(crate) fn parent_archive_path(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default()
}

pub(crate) fn join_archive_path(base: &str, child: &str) -> String {
    if base.is_empty() {
        child.trim_start_matches('/').to_string()
    } else {
        format!(
            "{}/{}",
            base.trim_end_matches('/'),
            child.trim_start_matches('/')
        )
    }
}

pub(crate) fn relative_archive_path(from_dir: &str, target: &str) -> String {
    let from = from_dir
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let target = target
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let mut common = 0usize;
    while common < from.len() && common < target.len() && from[common] == target[common] {
        common += 1;
    }
    let mut result = Vec::new();
    for _ in common..from.len() {
        result.push("..");
    }
    result.extend(target.iter().skip(common).copied());
    if result.is_empty() {
        ".".into()
    } else {
        result.join("/")
    }
}

pub(crate) fn resolve_archive_href(base: &str, href: &str) -> Result<String, String> {
    let href = href.split(['#', '?']).next().unwrap_or(href);
    if href.contains("://") || href.starts_with("data:") || href.starts_with('/') {
        return Err(format!(
            "EPUB package resource uses unsupported URI {href}."
        ));
    }
    let decoded = percent_decode(href)?;
    normalize_archive_path(&join_archive_path(base, &decoded))
}

pub(crate) fn uri_path(value: &str) -> String {
    percent_encode(value, true)
}

pub(crate) fn format_clock(milliseconds: u64) -> String {
    let hours = milliseconds / 3_600_000;
    let minutes = (milliseconds % 3_600_000) / 60_000;
    let seconds = (milliseconds % 60_000) / 1000;
    let millis = milliseconds % 1000;
    format!("{hours}:{minutes:02}:{seconds:02}.{millis:03}")
}

fn maybe_add_media_overlay(
    element: &mut BytesStart<'_>,
    package_dir: &str,
    overlays: &HashMap<&str, &str>,
) -> Result<(), String> {
    if attribute_value(element, b"media-type")?.as_deref() != Some("application/xhtml+xml") {
        return Ok(());
    }
    let href = attribute_value(element, b"href")?.unwrap_or_default();
    if href.is_empty() {
        return Ok(());
    }
    let resolved = resolve_archive_href(package_dir, &href)?;
    if let Some(overlay_id) = overlays.get(resolved.as_str()) {
        if attribute_value(element, b"media-overlay")?.is_some() {
            return Err(format!(
                "EPUB package item {resolved} already has media-overlay."
            ));
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
                let qname = element.name();
                let name = local_name(qname.as_ref());
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
                    if let Some(id) =
                        attribute_value(&element, b"id")?.filter(|id| !id.trim().is_empty())
                    {
                        existing_ids.insert(ordinal, id);
                    }
                    stack.push(Frame {
                        name: name.to_vec(),
                        ordinal,
                    });
                }
            }
            Ok(Event::Empty(element)) => {
                if suppressed_depth == 0 {
                    let qname = element.name();
                    if is_boundary_element(local_name(qname.as_ref())) {
                        finish_line(&mut line_owners, &mut line_owner, &mut line_has_text)?;
                    }
                }
            }
            Ok(Event::End(element)) => {
                if suppressed_depth > 0 {
                    suppressed_depth -= 1;
                    continue;
                }
                let qname = element.name();
                let name = local_name(qname.as_ref());
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
        lines
            .push(owner.ok_or("Visible EPUB text is not contained in an anchorable XHTML block.")?);
    }
    *owner = None;
    *has_text = false;
    Ok(())
}

fn normalize_archive_path(path: &str) -> Result<String, String> {
    let normalized = path.replace('\\', "/");
    let mut parts = Vec::<String>::new();
    for part in normalized.split('/') {
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

fn uri_fragment(value: &str) -> String {
    percent_encode(value, false)
}

fn percent_encode(value: &str, preserve_slash: bool) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'.' | b'-' | b'_' | b'~')
            || (preserve_slash && byte == b'/')
        {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
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
            relative_archive_path(
                "OPS/storyteller/overlays",
                "OPS/storyteller/audio/audio.m4a"
            ),
            "../audio/audio.m4a"
        );
    }

    #[test]
    fn media_clock_preserves_milliseconds() {
        assert_eq!(format_clock(3_723_045), "1:02:03.045");
    }
}
