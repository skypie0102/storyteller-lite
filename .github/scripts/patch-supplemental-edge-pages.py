from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)


# --- durable review destination model ---
audio_path = Path("crates/storyteller-core/src/audio_review.rs")
audio = audio_path.read_text(encoding="utf-8")
audio = replace_once(
    audio,
    '''#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioReviewDestination {
    pub href: String,
    #[serde(default)]
    pub line_index: Option<usize>,
    #[serde(default)]
    pub image_href: Option<String>,
}''',
    '''#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioReviewSupplementalPlacement {
    BeforeAnchor,
    AfterAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioReviewDestination {
    /// Existing EPUB content-document href used either as the direct destination or as
    /// the spine anchor for a generated supplemental page.
    pub href: String,
    #[serde(default)]
    pub line_index: Option<usize>,
    #[serde(default)]
    pub image_href: Option<String>,
    #[serde(default)]
    pub supplemental: Option<AudioReviewSupplementalPlacement>,
}''',
    "supplemental destination model",
)
old_validate = '''        AudioReviewDecision::Assigned { destination, .. } => {
            if destination.href.trim().is_empty() {
                return Err("Assigned audio review destination href cannot be blank.".into());
            }
            if destination
                .image_href
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            {
                return Err("Assigned audio review image href cannot be blank.".into());
            }
            Ok(())
        }'''
new_validate = '''        AudioReviewDecision::Assigned {
            destination,
            classification,
            ..
        } => {
            if destination.href.trim().is_empty() {
                return Err("Assigned audio review destination href cannot be blank.".into());
            }
            if destination
                .image_href
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            {
                return Err("Assigned audio review image href cannot be blank.".into());
            }
            let destination_kinds = usize::from(destination.line_index.is_some())
                + usize::from(destination.image_href.is_some())
                + usize::from(destination.supplemental.is_some());
            if destination_kinds != 1 {
                return Err(
                    "Assigned audio review destination must identify exactly one text, image, or supplemental-page target."
                        .into(),
                );
            }
            match destination.supplemental {
                Some(AudioReviewSupplementalPlacement::BeforeAnchor)
                    if *classification != Some(AudioReviewClassification::Introduction) =>
                {
                    Err("A supplemental page before the anchor must be classified as Introduction.".into())
                }
                Some(AudioReviewSupplementalPlacement::AfterAnchor)
                    if *classification != Some(AudioReviewClassification::Credits) =>
                {
                    Err("A supplemental page after the anchor must be classified as Credits.".into())
                }
                _ => Ok(()),
            }
        }'''
audio = replace_once(audio, old_validate, new_validate, "supplemental decision validation")
audio_path.write_text(audio, encoding="utf-8", newline="\n")

# Export supplemental placement.
lib_path = Path("crates/storyteller-core/src/lib.rs")
lib = lib_path.read_text(encoding="utf-8")
lib = replace_once(
    lib,
    '''    AudioReviewItem, AudioReviewPolicy, AudioReviewReport, AudioReviewSilenceEvidence,
    AudioReviewSuggestion, AudioReviewSummary,''',
    '''    AudioReviewItem, AudioReviewPolicy, AudioReviewReport, AudioReviewSilenceEvidence,
    AudioReviewSuggestion, AudioReviewSummary, AudioReviewSupplementalPlacement,''',
    "supplemental placement export",
)
lib_path.write_text(lib, encoding="utf-8", newline="\n")

# Supplemental assignments remain separate from the effective text alignment, but validate their anchor.
assign_path = Path("crates/storyteller-core/src/review_assignment.rs")
assign = assign_path.read_text(encoding="utf-8")
assign = replace_once(
    assign,
    '''            AudioReviewDecision::Assigned { destination, .. } => {
                if destination.image_href.is_some() {
                    return Err(
                        "Graphic/image audio assignments require the dedicated image rendering path, which is not implemented yet."
                            .into(),
                    );
                }
                let line_index = destination''',
    '''            AudioReviewDecision::Assigned { destination, .. } => {
                if destination.supplemental.is_some() {
                    if !corpus.sections.iter().any(|section| section.href == destination.href) {
                        return Err(format!(
                            "Supplemental audio review anchor {} is not an EPUB reading-order document.",
                            destination.href
                        ));
                    }
                    continue;
                }
                if destination.image_href.is_some() {
                    return Err(
                        "Graphic/image audio assignments require the dedicated image rendering path, which is not implemented yet."
                            .into(),
                    );
                }
                let line_index = destination''',
    "supplemental effective alignment handling",
)
# Existing unit-test destination literals are ordinary text targets.
assign = assign.replace(
    '''                    image_href: None,
                },''',
    '''                    image_href: None,
                    supplemental: None,
                },''',
)
assign_path.write_text(assign, encoding="utf-8", newline="\n")

# --- package rewrite primitives for generated spine pages ---
overlay_path = Path("crates/storyteller-core/src/epub_overlay.rs")
overlay = overlay_path.read_text(encoding="utf-8")
overlay = replace_once(
    overlay,
    '''use crate::{AlignmentSegment, AlignmentStatus};''',
    '''use crate::{
    AlignmentSegment, AlignmentStatus, AudioReviewClassification, AudioReviewSupplementalPlacement,
};''',
    "overlay supplemental imports",
)
overlay = replace_once(
    overlay,
    '''#[derive(Debug, Clone)]
pub(crate) struct PackageScan {''',
    '''#[derive(Debug, Clone)]
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
pub(crate) struct PackageScan {''',
    "supplemental overlay spec",
)
insert_before_annotate = '''pub(crate) fn annotate_xhtml_blocks('''
helpers = r'''pub(crate) fn add_supplemental_package_items(
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
                        if before.contains_key(idref.as_str()) || after.contains_key(idref.as_str()) {
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
                            .map_err(|error| format!("Could not rewrite EPUB package itemref: {error}"))?;
                        write_supplemental_itemrefs(&mut writer, after.get(idref.as_str()))?;
                        if before.contains_key(idref.as_str()) || after.contains_key(idref.as_str()) {
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
                if name == b"manifest" {
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
                } else if name == b"metadata" {
                    for supplement in supplements {
                        let refines = format!("#{}", supplement.overlay_item_id);
                        write_meta_duration(&mut writer, Some(&refines), supplement.duration_ms)?;
                    }
                }
                writer
                    .write_event(Event::End(element.into_owned()))
                    .map_err(|error| format!("Could not close EPUB package element: {error}"))?;
                if name == b"itemref" {
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
        _ => return Err("Only Introduction or Credits can use a supplemental read-aloud page.".into()),
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
        _ => return Err("Only Introduction or Credits can use a supplemental read-aloud page.".into()),
    };
    let duration = end_ms - start_ms;
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<smil xmlns=\"http://www.w3.org/ns/SMIL\" xmlns:epub=\"http://www.idpf.org/2007/ops\" version=\"3.0\"><body><seq epub:type=\"{}\"><par id=\"stl-extra-par-{}\"><text src=\"{}#{}\"/><audio src=\"{}\" clipBegin=\"{}\" clipEnd=\"{}\"/></par></seq></body></smil>",
        epub_type,
        sequence + 1,
        escape_xml(text_href),
        escape_xml(&uri_fragment(paragraph_id)),
        escape_xml(audio_href),
        format_clock(start_ms),
        format_clock(end_ms),
    );
    Ok((xml, duration))
}

'''
overlay = replace_once(overlay, insert_before_annotate, helpers + insert_before_annotate, "supplemental package helpers")
overlay_path.write_text(overlay, encoding="utf-8", newline="\n")

# --- build generated XHTML + SMIL and add it to package/spine ---
build_path = Path("crates/storyteller-core/src/epub_build.rs")
build = build_path.read_text(encoding="utf-8")
build = replace_once(
    build,
    '''        annotate_xhtml_blocks, build_smil, join_archive_path, parent_archive_path,
        relative_archive_path, rewrite_package, scan_package, unique_id, uri_path,
        OverlaySectionSpec,
    },
    read_audio_review_report, read_encoded_audio_descriptor, read_epub_corpus, AlignmentDocument,
    AlignmentStatus, CancellationToken,''',
    '''        add_supplemental_package_items, annotate_xhtml_blocks, build_smil,
        build_supplemental_smil, build_supplemental_xhtml, join_archive_path,
        parent_archive_path, relative_archive_path, rewrite_package, scan_package, unique_id,
        uri_path, OverlaySectionSpec, SupplementalOverlaySpec,
    },
    read_audio_review_report, read_encoded_audio_descriptor, read_epub_corpus, AlignmentDocument,
    AlignmentStatus, AudioReviewClassification, AudioReviewDecision, CancellationToken,''',
    "builder supplemental imports",
)
build = replace_once(
    build,
    '''    if sections.is_empty() {
        return Err("No EPUB spine document received synchronized Media Overlay content.".into());
    }

    let total_duration_ms = sections
        .iter()
        .try_fold(0u64, |total, section| {
            total.checked_add(section.duration_ms)
        })
        .ok_or("Media Overlay duration overflowed.")?;
    let package_rewrite = rewrite_package(''',
    '''    if sections.is_empty() {
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
        let xhtml_item_id = unique_id(&format!("stl-extra-page-{:04}", review_index + 1), &mut used_ids);
        let overlay_item_id = unique_id(&format!("stl-extra-mo-{:04}", review_index + 1), &mut used_ids);
        let paragraph_id = format!("stl-extra-text-{}", review_index + 1);
        let xhtml = build_supplemental_xhtml(*classification, &item.transcript_text, &paragraph_id)?;
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
    let package_rewrite = rewrite_package(''',
    "build supplemental overlays",
)
build = replace_once(
    build,
    '''        total_duration_ms,
    )?;

    if let Some(parent) = destination.parent() {''',
    '''        total_duration_ms,
    )?;
    let package_rewrite = add_supplemental_package_items(&package_rewrite, &supplemental)?;

    if let Some(parent) = destination.parent() {''',
    "supplemental package rewrite",
)
build = replace_once(
    build,
    '''    for section in &sections {
        write_text_entry(&mut writer, &section.href, &section.xhtml)?;
        write_text_entry(&mut writer, &section.smil_archive_path, &section.smil)?;
    }

    writer''',
    '''    for section in &sections {
        write_text_entry(&mut writer, &section.href, &section.xhtml)?;
        write_text_entry(&mut writer, &section.smil_archive_path, &section.smil)?;
    }
    for section in &supplemental {
        write_text_entry(&mut writer, &section.xhtml_archive_path, &section.xhtml)?;
        write_text_entry(&mut writer, &section.smil_archive_path, &section.smil)?;
    }

    writer''',
    "write supplemental entries",
)
build = replace_once(
    build,
    '''    Ok(EpubBuildSummary {
        overlay_count: sections.len(),
        synchronized_segments: sections
            .iter()
            .map(|section| section.synchronized_segments)
            .sum(),
        media_duration_ms: total_duration_ms,
    })''',
    '''    Ok(EpubBuildSummary {
        overlay_count: sections.len() + supplemental.len(),
        synchronized_segments: sections
            .iter()
            .map(|section| section.synchronized_segments)
            .sum::<usize>()
            + supplemental.len(),
        media_duration_ms: total_duration_ms,
    })''',
    "supplemental build summary",
)
build_path.write_text(build, encoding="utf-8", newline="\n")

# --- allocator action for edge segments ---
review_path = Path("crates/storyteller-ui/src/review_ui.rs")
review = review_path.read_text(encoding="utf-8")
review = replace_once(
    review,
    '''    AudioReviewEdge, AudioReviewItem, Job, JobQueue, JobStatus, PipelineStage,
    DEFAULT_REVIEW_CANDIDATE_LIMIT,''',
    '''    AlignmentStatus, AudioReviewClassification, AudioReviewEdge, AudioReviewItem,
    AudioReviewSupplementalPlacement, Job, JobQueue, JobStatus, PipelineStage,
    DEFAULT_REVIEW_CANDIDATE_LIMIT,''',
    "review UI supplemental imports",
)
review = replace_once(
    review,
    '''                                image_href: None,
                            },''',
    '''                                image_href: None,
                                supplemental: None,
                            },''',
    "text destination supplemental field",
)
insert_handler = '''    {
        let controller = Rc::clone(&controller);
        let queue = Rc::clone(&queue);
        let ui_weak = ui.as_weak();
        ui.on_review_exclude(move || {'''
new_handler = '''    {
        let controller = Rc::clone(&controller);
        let queue = Rc::clone(&queue);
        let ui_weak = ui.as_weak();
        ui.on_review_preserve_edge(move || {
            let Some(ui) = ui_weak.upgrade() else {
                return;
            };
            let mut controller = controller.borrow_mut();
            controller.stop_preview();
            let result = review_job(&queue.borrow())
                .ok_or_else(|| "No book is waiting for audio review.".to_string())
                .and_then(|job| {
                    let report = worker_bridge::load_audio_review_report(job)?;
                    let item = report
                        .unmatched
                        .get(controller.selected_index)
                        .ok_or_else(|| "Selected review segment no longer exists.".to_string())?;
                    let (anchor_href, placement, classification) = edge_page_destination(job, item)?;
                    apply_audio_review_decision(
                        &worker_bridge::audio_review_path(job),
                        &worker_bridge::audio_review_draft_path(job),
                        &item.id,
                        AudioReviewDecision::Assigned {
                            destination: AudioReviewDestination {
                                href: anchor_href,
                                line_index: None,
                                image_href: None,
                                supplemental: Some(placement),
                            },
                            classification: Some(classification),
                            source: AudioReviewDecisionSource::Manual,
                        },
                    )
                });
            match result {
                Ok(()) => {
                    ui.set_status_text("Edge narration will be preserved on a supplemental read-aloud page.".into());
                    select_next_pending(&queue.borrow(), &mut controller);
                }
                Err(error) => ui.set_status_text(error.into()),
            }
            refresh_for_ui(&ui, &queue.borrow(), &mut controller);
        });
    }

''' + insert_handler
review = replace_once(review, insert_handler, new_handler, "preserve edge callback")
review = replace_once(
    review,
    '''    ui.set_review_complete(report.is_complete());
    ui.set_review_allocator_status_text(if report.pending_count() == 0 {''',
    '''    ui.set_review_complete(report.is_complete());
    let preserve_label = match item.edge {
        Some(AudioReviewEdge::Introduction) => "Preserve as Introduction",
        Some(AudioReviewEdge::Credits) => "Preserve as Credits",
        None => "",
    };
    ui.set_review_can_preserve_edge(item.edge.is_some());
    ui.set_review_preserve_edge_text(preserve_label.into());
    ui.set_review_allocator_status_text(if report.pending_count() == 0 {''',
    "edge preserve UI state",
)
review = replace_once(
    review,
    '''    ui.set_review_can_next(false);
    ui.set_review_complete(false);''',
    '''    ui.set_review_can_next(false);
    ui.set_review_can_preserve_edge(false);
    ui.set_review_preserve_edge_text("".into());
    ui.set_review_complete(false);''',
    "clear edge preserve UI",
)
load_anchor = '''fn load_candidates(
    job: &Job,'''
edge_helper = '''fn edge_page_destination(
    job: &Job,
    item: &AudioReviewItem,
) -> Result<(String, AudioReviewSupplementalPlacement, AudioReviewClassification), String> {
    let workspace = worker_bridge::job_workspace(job);
    let alignment_path = workspace
        .stage_dir(PipelineStage::Align)
        .join("alignment.json");
    let data = std::fs::read(&alignment_path).map_err(|error| {
        format!("Could not read alignment map {}: {error}", alignment_path.display())
    })?;
    let alignment: AlignmentDocument = serde_json::from_slice(&data).map_err(|error| {
        format!("Could not parse alignment map {}: {error}", alignment_path.display())
    })?;
    match item.edge {
        Some(AudioReviewEdge::Introduction) => {
            let href = alignment
                .segments
                .iter()
                .find(|segment| segment.status == AlignmentStatus::Matched)
                .and_then(|segment| segment.book_start.as_ref())
                .map(|position| position.href.clone())
                .ok_or_else(|| "Introduction has no matched EPUB anchor.".to_string())?;
            Ok((
                href,
                AudioReviewSupplementalPlacement::BeforeAnchor,
                AudioReviewClassification::Introduction,
            ))
        }
        Some(AudioReviewEdge::Credits) => {
            let href = alignment
                .segments
                .iter()
                .rev()
                .find(|segment| segment.status == AlignmentStatus::Matched)
                .and_then(|segment| segment.book_end.as_ref())
                .map(|position| position.href.clone())
                .ok_or_else(|| "Credits have no matched EPUB anchor.".to_string())?;
            Ok((
                href,
                AudioReviewSupplementalPlacement::AfterAnchor,
                AudioReviewClassification::Credits,
            ))
        }
        None => Err("Selected segment is not a leading or trailing edge candidate.".into()),
    }
}

'''
review = replace_once(review, load_anchor, edge_helper + load_anchor, "edge page destination helper")
review = replace_once(
    review,
    '''        AudioReviewDecision::Assigned { destination, .. } => match destination.line_index {
            Some(line) => format!("Assigned — {} line {}", destination.href, line + 1),
            None => format!("Assigned — {}", destination.href),
        },''',
    '''        AudioReviewDecision::Assigned { destination, classification, .. } => {
            if destination.supplemental.is_some() {
                format!("Assigned — {:?} supplemental page anchored at {}", classification, destination.href)
            } else {
                match destination.line_index {
                    Some(line) => format!("Assigned — {} line {}", destination.href, line + 1),
                    None => format!("Assigned — {}", destination.href),
                }
            }
        },''',
    "supplemental decision display",
)
review_path.write_text(review, encoding="utf-8", newline="\n")

# Slint callback/properties/button.
slint_path = Path("crates/storyteller-ui/ui/app-window.slint")
slint = slint_path.read_text(encoding="utf-8")
slint = replace_once(
    slint,
    '''    in property <bool> review-can-next: false;
    in property <bool> review-complete: false;''',
    '''    in property <bool> review-can-next: false;
    in property <bool> review-can-preserve-edge: false;
    in property <string> review-preserve-edge-text: "";
    in property <bool> review-complete: false;''',
    "supplemental UI properties",
)
slint = replace_once(
    slint,
    '''    callback review-assign(string, int);
    callback review-exclude();''',
    '''    callback review-assign(string, int);
    callback review-preserve-edge();
    callback review-exclude();''',
    "supplemental UI callback",
)
slint = replace_once(
    slint,
    '''                                Rectangle { horizontal-stretch: 1; }
                                Button { text: "Exclude segment"; clicked => { root.review-exclude(); } }''',
    '''                                Rectangle { horizontal-stretch: 1; }
                                if root.review-can-preserve-edge : Button {
                                    text: root.review-preserve-edge-text;
                                    primary: true;
                                    clicked => { root.review-preserve-edge(); }
                                }
                                Button { text: "Exclude segment"; clicked => { root.review-exclude(); } }''',
    "supplemental edge button",
)
slint_path.write_text(slint, encoding="utf-8", newline="\n")

# Build behavior changed semantically: do not reuse an older Build EPUB checkpoint.
backend_path = Path("crates/storyteller-ui/src/pipeline_backend.rs")
backend = backend_path.read_text(encoding="utf-8")
backend = replace_once(
    backend,
    '''        epub_backend: "storyteller:epub-media-overlay-v1-block".into(),''',
    '''        epub_backend: "storyteller:epub-media-overlay-v2-supplemental-edge".into(),''',
    "builder fingerprint version",
)
backend_path.write_text(backend, encoding="utf-8", newline="\n")
