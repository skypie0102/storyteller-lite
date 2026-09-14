use crate::{
    epub_overlay::{parent_archive_path, resolve_archive_href},
    AlignmentSegment, AlignmentStatus, AudioReviewClassification, AudioReviewDecision,
    AudioReviewReport,
};
use quick_xml::{
    escape::unescape,
    events::{BytesStart, Event},
    Reader, Writer,
};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GraphicReadoutSpec {
    pub review_index: usize,
    pub document_href: String,
    pub image_href: String,
    pub audio_start_ms: u64,
    pub audio_end_ms: u64,
}

pub(crate) fn collect_graphic_readouts(
    review: &AudioReviewReport,
) -> Result<Vec<GraphicReadoutSpec>, String> {
    let mut result = Vec::new();
    let mut seen_targets = HashSet::<(String, String)>::new();
    for (review_index, item) in review.unmatched.iter().enumerate() {
        let AudioReviewDecision::Assigned {
            destination,
            classification,
            ..
        } = &item.decision
        else {
            continue;
        };
        let Some(image_href) = destination.image_href.as_deref() else {
            if *classification == Some(AudioReviewClassification::GraphicReadout) {
                return Err("Graphic Readout assignment is missing its image destination.".into());
            }
            continue;
        };
        if *classification != Some(AudioReviewClassification::GraphicReadout) {
            return Err("Image audio assignment must be classified as Graphic Readout.".into());
        }
        if destination.line_index.is_some() || destination.supplemental.is_some() {
            return Err(
                "Graphic Readout assignment must identify only an existing EPUB image target."
                    .into(),
            );
        }
        if item.audio_end_ms <= item.audio_start_ms {
            return Err("Graphic Readout audio segment has an invalid duration.".into());
        }
        let target = (destination.href.clone(), image_href.to_string());
        if !seen_targets.insert(target.clone()) {
            return Err(format!(
                "Graphic Readout image {} in {} is assigned more than once.",
                target.1, target.0
            ));
        }
        result.push(GraphicReadoutSpec {
            review_index,
            document_href: destination.href.clone(),
            image_href: image_href.to_string(),
            audio_start_ms: item.audio_start_ms,
            audio_end_ms: item.audio_end_ms,
        });
    }
    Ok(result)
}

pub(crate) fn annotate_graphic_targets(
    xml: &str,
    document_href: &str,
    specs: &[GraphicReadoutSpec],
    section_index: usize,
) -> Result<(String, HashMap<usize, String>), String> {
    if specs.is_empty() {
        return Ok((xml.to_string(), HashMap::new()));
    }
    if specs.iter().any(|spec| spec.document_href != document_href) {
        return Err("Graphic Readout target set mixes EPUB content documents.".into());
    }

    let mut used_ids = collect_ids(xml)?;
    let document_dir = parent_archive_path(document_href);
    let wanted = specs
        .iter()
        .map(|spec| (spec.image_href.as_str(), spec.review_index))
        .collect::<HashMap<_, _>>();
    let mut anchors = HashMap::<usize, String>::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::new());

    loop {
        match reader.read_event() {
            Ok(Event::Start(mut element)) => {
                maybe_anchor_image_element(
                    &mut element,
                    &document_dir,
                    &wanted,
                    &mut used_ids,
                    &mut anchors,
                    section_index,
                )?;
                writer
                    .write_event(Event::Start(element.into_owned()))
                    .map_err(|error| {
                        format!("Could not rewrite EPUB Graphic Readout XHTML: {error}")
                    })?;
            }
            Ok(Event::Empty(mut element)) => {
                maybe_anchor_image_element(
                    &mut element,
                    &document_dir,
                    &wanted,
                    &mut used_ids,
                    &mut anchors,
                    section_index,
                )?;
                writer
                    .write_event(Event::Empty(element.into_owned()))
                    .map_err(|error| {
                        format!("Could not rewrite EPUB Graphic Readout XHTML: {error}")
                    })?;
            }
            Ok(Event::Eof) => break,
            Ok(event) => writer.write_event(event.into_owned()).map_err(|error| {
                format!("Could not rewrite EPUB Graphic Readout XHTML: {error}")
            })?,
            Err(error) => {
                return Err(format!(
                    "Could not parse EPUB XHTML for Graphic Readout anchors: {error}"
                ))
            }
        }
    }

    for spec in specs {
        if !anchors.contains_key(&spec.review_index) {
            return Err(format!(
                "Graphic Readout image {} was not found in EPUB document {}.",
                spec.image_href, document_href
            ));
        }
    }
    let rewritten = String::from_utf8(writer.into_inner())
        .map_err(|error| format!("Rewritten Graphic Readout XHTML is not UTF-8: {error}"))?;
    Ok((rewritten, anchors))
}

pub(crate) fn build_combined_smil(
    segments: &[(usize, &AlignmentSegment)],
    text_anchors: &HashMap<usize, String>,
    graphics: &[GraphicReadoutSpec],
    graphic_anchors: &HashMap<usize, String>,
    text_href: &str,
    audio_href: &str,
    section_index: usize,
) -> Result<(String, u64, usize), String> {
    #[derive(Debug)]
    struct Cue {
        start_ms: u64,
        end_ms: u64,
        id: String,
        anchor: String,
    }

    let mut cues = Vec::<Cue>::new();
    for (segment_index, segment) in segments {
        if segment.status != AlignmentStatus::Matched {
            continue;
        }
        let position = segment
            .book_start
            .as_ref()
            .ok_or("Matched segment has no EPUB start position.")?;
        let anchor = text_anchors.get(&position.line_index).ok_or_else(|| {
            format!(
                "Could not map EPUB line {} in {} to an XHTML fragment.",
                position.line_index, position.href
            )
        })?;
        if segment.audio_end_ms <= segment.audio_start_ms {
            return Err("Matched alignment segment has an invalid audio duration.".into());
        }
        cues.push(Cue {
            start_ms: segment.audio_start_ms,
            end_ms: segment.audio_end_ms,
            id: format!("stl-par-{}-{}", section_index + 1, segment_index + 1),
            anchor: anchor.clone(),
        });
    }
    for graphic in graphics {
        let anchor = graphic_anchors
            .get(&graphic.review_index)
            .ok_or("Graphic Readout is missing its XHTML image anchor.")?;
        if graphic.audio_end_ms <= graphic.audio_start_ms {
            return Err("Graphic Readout audio segment has an invalid duration.".into());
        }
        cues.push(Cue {
            start_ms: graphic.audio_start_ms,
            end_ms: graphic.audio_end_ms,
            id: format!(
                "stl-graphic-par-{}-{}",
                section_index + 1,
                graphic.review_index + 1
            ),
            anchor: anchor.clone(),
        });
    }
    if cues.is_empty() {
        return Err("Media Overlay section contains no synchronization cues.".into());
    }
    cues.sort_by(|left, right| {
        left.start_ms
            .cmp(&right.start_ms)
            .then_with(|| left.end_ms.cmp(&right.end_ms))
            .then_with(|| left.id.cmp(&right.id))
    });
    for window in cues.windows(2) {
        if window[1].start_ms < window[0].end_ms {
            return Err(format!(
                "Media Overlay cues {} and {} overlap in audio time.",
                window[0].id, window[1].id
            ));
        }
    }

    let mut duration_ms = 0u64;
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<smil xmlns=\"http://www.w3.org/ns/SMIL\" xmlns:epub=\"http://www.idpf.org/2007/ops\" version=\"3.0\"><body><seq>",
    );
    for cue in &cues {
        let clip_duration = cue.end_ms - cue.start_ms;
        duration_ms = duration_ms
            .checked_add(clip_duration)
            .ok_or("Media Overlay duration overflowed.")?;
        xml.push_str(&format!(
            "<par id=\"{}\"><text src=\"{}#{}\"/><audio src=\"{}\" clipBegin=\"{}\" clipEnd=\"{}\"/></par>",
            escape_xml(&cue.id),
            escape_xml(text_href),
            escape_xml(&uri_fragment(&cue.anchor)),
            escape_xml(audio_href),
            format_clock(cue.start_ms),
            format_clock(cue.end_ms),
        ));
    }
    xml.push_str("</seq></body></smil>");
    Ok((xml, duration_ms, cues.len()))
}

fn maybe_anchor_image_element(
    element: &mut BytesStart<'_>,
    document_dir: &str,
    wanted: &HashMap<&str, usize>,
    used_ids: &mut HashSet<String>,
    anchors: &mut HashMap<usize, String>,
    section_index: usize,
) -> Result<(), String> {
    let qname = element.name();
    let name = local_name(qname.as_ref());
    let href = match name {
        b"img" => attribute_value(element, b"src")?,
        b"image" => attribute_value(element, b"href")?,
        _ => None,
    };
    let Some(href) = href else {
        return Ok(());
    };
    let Ok(resolved) = resolve_archive_href(document_dir, &href) else {
        return Ok(());
    };
    let Some(review_index) = wanted.get(resolved.as_str()).copied() else {
        return Ok(());
    };
    if anchors.contains_key(&review_index) {
        return Ok(());
    }

    let id = match attribute_value(element, b"id")?.filter(|value| !value.trim().is_empty()) {
        Some(id) => id,
        None => {
            let base = format!("stl-graphic-s{}-r{}", section_index + 1, review_index + 1);
            let generated = unique_id(&base, used_ids);
            element.push_attribute(("id", generated.as_str()));
            generated
        }
    };
    anchors.insert(review_index, id);
    Ok(())
}

fn collect_ids(xml: &str) -> Result<HashSet<String>, String> {
    let mut ids = HashSet::new();
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) | Ok(Event::Empty(element)) => {
                if let Some(id) = attribute_value(&element, b"id")?.filter(|id| !id.is_empty()) {
                    ids.insert(id);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("Could not inspect EPUB XHTML ids: {error}")),
        }
    }
    Ok(ids)
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

fn format_clock(milliseconds: u64) -> String {
    let hours = milliseconds / 3_600_000;
    let minutes = (milliseconds % 3_600_000) / 60_000;
    let seconds = (milliseconds % 60_000) / 1000;
    let millis = milliseconds % 1000;
    format!("{hours}:{minutes:02}:{seconds:02}.{millis:03}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AudioReviewDecisionSource, AudioReviewDestination, AudioReviewItem, CorpusPosition,
    };

    fn graphic_item(id: &str, image_href: &str, start: u64, end: u64) -> AudioReviewItem {
        AudioReviewItem {
            id: id.into(),
            alignment_index: 0,
            audio_start_ms: start,
            audio_end_ms: end,
            transcript_text: "diagram readout".into(),
            suggestion: None,
            edge: None,
            silence: None,
            decision: AudioReviewDecision::Assigned {
                destination: AudioReviewDestination {
                    href: "OPS/Text/ch.xhtml".into(),
                    line_index: None,
                    image_href: Some(image_href.into()),
                    supplemental: None,
                },
                classification: Some(AudioReviewClassification::GraphicReadout),
                source: AudioReviewDecisionSource::Automatic,
            },
        }
    }

    #[test]
    fn duplicate_graphic_target_is_rejected() {
        let report = AudioReviewReport {
            total_segments: 2,
            matched_segments: 0,
            match_percent: 0.0,
            unmatched: vec![
                graphic_item("one", "OPS/Images/a.png", 1000, 2000),
                graphic_item("two", "OPS/Images/a.png", 2000, 3000),
            ],
            accepted_unmatched_exclusion: false,
        };
        assert!(collect_graphic_readouts(&report)
            .unwrap_err()
            .contains("assigned more than once"));
    }

    #[test]
    fn xhtml_image_target_gets_durable_anchor() {
        let specs = vec![GraphicReadoutSpec {
            review_index: 3,
            document_href: "OPS/Text/ch.xhtml".into(),
            image_href: "OPS/Images/diagram.png".into(),
            audio_start_ms: 1000,
            audio_end_ms: 2000,
        }];
        let xml = "<html xmlns=\"http://www.w3.org/1999/xhtml\"><body><p>Text</p><img src=\"../Images/diagram.png\" alt=\"Diagram\"/></body></html>";
        let (rewritten, anchors) =
            annotate_graphic_targets(xml, "OPS/Text/ch.xhtml", &specs, 2).unwrap();
        let anchor = anchors.get(&3).unwrap();
        assert!(rewritten.contains(&format!("id=\"{anchor}\"")));
        assert!(rewritten.contains("src=\"../Images/diagram.png\""));
    }

    #[test]
    fn svg_image_target_reuses_existing_anchor() {
        let specs = vec![GraphicReadoutSpec {
            review_index: 0,
            document_href: "OPS/Text/ch.xhtml".into(),
            image_href: "OPS/Images/map.png".into(),
            audio_start_ms: 1000,
            audio_end_ms: 2000,
        }];
        let xml = "<html xmlns=\"http://www.w3.org/1999/xhtml\"><body><svg xmlns=\"http://www.w3.org/2000/svg\"><image id=\"map-anchor\" href=\"../Images/map.png\"/></svg></body></html>";
        let (rewritten, anchors) =
            annotate_graphic_targets(xml, "OPS/Text/ch.xhtml", &specs, 0).unwrap();
        assert_eq!(anchors.get(&0).map(String::as_str), Some("map-anchor"));
        assert_eq!(rewritten.matches("id=\"map-anchor\"").count(), 1);
    }

    #[test]
    fn combined_smil_orders_text_and_graphic_by_audio_time() {
        let segment = AlignmentSegment {
            audio_start_ms: 2000,
            audio_end_ms: 3000,
            transcript_text: "matched".into(),
            status: AlignmentStatus::Matched,
            match_percent: Some(100.0),
            book_start: Some(CorpusPosition {
                href: "OPS/Text/ch.xhtml".into(),
                line_index: 0,
                char_offset: 0,
            }),
            book_end: Some(CorpusPosition {
                href: "OPS/Text/ch.xhtml".into(),
                line_index: 0,
                char_offset: 7,
            }),
        };
        let segments = vec![(1usize, &segment)];
        let text_anchors = HashMap::from([(0usize, "text-anchor".to_string())]);
        let graphics = vec![GraphicReadoutSpec {
            review_index: 0,
            document_href: "OPS/Text/ch.xhtml".into(),
            image_href: "OPS/Images/diagram.png".into(),
            audio_start_ms: 1000,
            audio_end_ms: 2000,
        }];
        let graphic_anchors = HashMap::from([(0usize, "image-anchor".to_string())]);
        let (smil, duration, count) = build_combined_smil(
            &segments,
            &text_anchors,
            &graphics,
            &graphic_anchors,
            "../Text/ch.xhtml",
            "../audio/book.m4a",
            0,
        )
        .unwrap();
        assert_eq!(duration, 2000);
        assert_eq!(count, 2);
        assert!(smil.find("image-anchor").unwrap() < smil.find("text-anchor").unwrap());
    }

    #[test]
    fn combined_smil_rejects_overlapping_audio_cues() {
        let segment = AlignmentSegment {
            audio_start_ms: 1500,
            audio_end_ms: 2500,
            transcript_text: "matched".into(),
            status: AlignmentStatus::Matched,
            match_percent: Some(100.0),
            book_start: Some(CorpusPosition {
                href: "OPS/Text/ch.xhtml".into(),
                line_index: 0,
                char_offset: 0,
            }),
            book_end: Some(CorpusPosition {
                href: "OPS/Text/ch.xhtml".into(),
                line_index: 0,
                char_offset: 7,
            }),
        };
        let segments = vec![(1usize, &segment)];
        let text_anchors = HashMap::from([(0usize, "text-anchor".to_string())]);
        let graphics = vec![GraphicReadoutSpec {
            review_index: 0,
            document_href: "OPS/Text/ch.xhtml".into(),
            image_href: "OPS/Images/diagram.png".into(),
            audio_start_ms: 1000,
            audio_end_ms: 2000,
        }];
        let graphic_anchors = HashMap::from([(0usize, "image-anchor".to_string())]);
        assert!(build_combined_smil(
            &segments,
            &text_anchors,
            &graphics,
            &graphic_anchors,
            "../Text/ch.xhtml",
            "../audio/book.m4a",
            0,
        )
        .unwrap_err()
        .contains("overlap"));
    }
}
