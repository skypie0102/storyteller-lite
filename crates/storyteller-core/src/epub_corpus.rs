use crate::CancellationToken;
use quick_xml::{
    escape::unescape,
    events::{BytesStart, Event},
    Reader,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Read, Seek},
    path::Path,
};
use zip::ZipArchive;

const MAX_CONTAINER_BYTES: u64 = 1024 * 1024;
const MAX_PACKAGE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SPINE_DOCUMENT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_TOTAL_SPINE_BYTES: usize = 128 * 1024 * 1024;
const READ_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpubCorpus {
    pub package_path: String,
    pub sections: Vec<EpubSection>,
}

impl EpubCorpus {
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    pub fn character_count(&self) -> usize {
        self.sections
            .iter()
            .map(|section| section.text.chars().count())
            .sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpubSection {
    pub href: String,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpubCorpusSummary {
    pub section_count: usize,
    pub character_count: usize,
}

pub fn extract_epub_corpus(
    epub_path: &Path,
    destination: &Path,
    cancellation: &CancellationToken,
) -> Result<EpubCorpusSummary, String> {
    if cancellation.is_requested() {
        return Err("EPUB text extraction was cancelled.".into());
    }
    let file = File::open(epub_path)
        .map_err(|error| format!("Could not open EPUB {}: {error}", epub_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| format!("Could not read EPUB ZIP container: {error}"))?;
    if archive.is_empty() {
        return Err("EPUB ZIP container is empty.".into());
    }

    let container = read_zip_text(
        &mut archive,
        "META-INF/container.xml",
        MAX_CONTAINER_BYTES,
        cancellation,
    )?;
    let package_path = parse_container_package_path(&container)?;
    let package_xml = read_zip_text(&mut archive, &package_path, MAX_PACKAGE_BYTES, cancellation)?;
    let package = parse_package_document(&package_xml)?;
    let package_dir = parent_epub_path(&package_path);

    let mut sections = Vec::new();
    let mut total_spine_bytes = 0usize;
    for idref in package.spine {
        if cancellation.is_requested() {
            return Err("EPUB text extraction was cancelled.".into());
        }
        let item = package
            .manifest
            .get(&idref)
            .ok_or_else(|| format!("EPUB spine references missing manifest item {idref}."))?;
        if item.media_type != "application/xhtml+xml" || item.is_navigation {
            continue;
        }
        let href = resolve_epub_href(&package_dir, &item.href)?;
        let document = read_zip_text(&mut archive, &href, MAX_SPINE_DOCUMENT_BYTES, cancellation)?;
        total_spine_bytes = total_spine_bytes
            .checked_add(document.len())
            .ok_or("EPUB spine content is too large.")?;
        if total_spine_bytes > MAX_TOTAL_SPINE_BYTES {
            return Err("EPUB spine content exceeds the 128 MiB extraction limit.".into());
        }
        let text = extract_xhtml_text(&document)?;
        if !text.trim().is_empty() {
            sections.push(EpubSection { href, text });
        }
    }

    if sections.is_empty() {
        return Err("EPUB spine contains no readable XHTML text.".into());
    }
    let corpus = EpubCorpus {
        package_path,
        sections,
    };
    let summary = EpubCorpusSummary {
        section_count: corpus.section_count(),
        character_count: corpus.character_count(),
    };
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Could not create EPUB corpus destination {}: {error}",
                parent.display()
            )
        })?;
    }
    let json = serde_json::to_vec_pretty(&corpus)
        .map_err(|error| format!("Could not serialize EPUB reading-order corpus: {error}"))?;
    fs::write(destination, json).map_err(|error| {
        format!(
            "Could not write EPUB reading-order corpus {}: {error}",
            destination.display()
        )
    })?;
    Ok(summary)
}

pub fn read_epub_corpus(path: &Path) -> Result<EpubCorpus, String> {
    let data = fs::read(path)
        .map_err(|error| format!("Could not read EPUB corpus {}: {error}", path.display()))?;
    let corpus: EpubCorpus = serde_json::from_slice(&data)
        .map_err(|error| format!("Could not parse EPUB corpus {}: {error}", path.display()))?;
    if corpus.sections.is_empty() || corpus.character_count() == 0 {
        return Err(format!("EPUB corpus is empty: {}", path.display()));
    }
    Ok(corpus)
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

fn parse_container_package_path(xml: &str) -> Result<String, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) | Ok(Event::Empty(element))
                if local_name(element.name().as_ref()) == b"rootfile" =>
            {
                let path = attribute_value(&element, b"full-path")?
                    .ok_or("EPUB container rootfile is missing full-path.")?;
                return normalize_archive_path(&path, false);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("Could not parse EPUB container.xml: {error}")),
        }
    }
    Err("EPUB container.xml does not declare a package document.".into())
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
            Ok(Event::Start(element)) => match local_name(element.name().as_ref()) {
                b"manifest" => in_manifest = true,
                b"spine" => in_spine = true,
                b"item" if in_manifest => parse_manifest_item(&element, &mut manifest)?,
                b"itemref" if in_spine => parse_spine_item(&element, &mut spine)?,
                _ => {}
            },
            Ok(Event::Empty(element)) => match local_name(element.name().as_ref()) {
                b"item" if in_manifest => parse_manifest_item(&element, &mut manifest)?,
                b"itemref" if in_spine => parse_spine_item(&element, &mut spine)?,
                _ => {}
            },
            Ok(Event::End(element)) => match local_name(element.name().as_ref()) {
                b"manifest" => in_manifest = false,
                b"spine" => in_spine = false,
                _ => {}
            },
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

fn extract_xhtml_text(xml: &str) -> Result<String, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut output = String::new();
    let mut pending_space = false;
    let mut suppressed_depth = 0usize;

    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let name = element.name();
                let name = local_name(name.as_ref());
                if suppressed_depth > 0 {
                    suppressed_depth += 1;
                } else if is_suppressed_element(name) {
                    suppressed_depth = 1;
                } else if is_block_element(name) {
                    append_boundary(&mut output, &mut pending_space);
                }
            }
            Ok(Event::Empty(element)) => {
                if suppressed_depth == 0 && is_block_element(local_name(element.name().as_ref())) {
                    append_boundary(&mut output, &mut pending_space);
                }
            }
            Ok(Event::End(element)) => {
                if suppressed_depth > 0 {
                    suppressed_depth -= 1;
                } else if is_block_element(local_name(element.name().as_ref())) {
                    append_boundary(&mut output, &mut pending_space);
                }
            }
            Ok(Event::Text(text)) if suppressed_depth == 0 => {
                let decoded = text
                    .decode()
                    .map_err(|error| format!("Could not decode EPUB XHTML text: {error}"))?;
                let decoded = unescape(&decoded)
                    .map_err(|error| format!("Could not unescape EPUB XHTML text: {error}"))?;
                append_text(&mut output, &mut pending_space, &decoded);
            }
            Ok(Event::CData(text)) if suppressed_depth == 0 => {
                let decoded = text
                    .decode()
                    .map_err(|error| format!("Could not decode EPUB XHTML CDATA: {error}"))?;
                append_text(&mut output, &mut pending_space, &decoded);
            }
            Ok(Event::GeneralRef(reference)) if suppressed_depth == 0 => {
                let raw = std::str::from_utf8(reference.as_ref()).map_err(|error| {
                    format!("EPUB XHTML entity reference is not UTF-8: {error}")
                })?;
                let escaped = format!("&{raw};");
                let decoded = unescape(&escaped).map_err(|error| {
                    format!("Could not unescape EPUB XHTML entity reference: {error}")
                })?;
                append_text(&mut output, &mut pending_space, &decoded);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(format!("Could not parse EPUB XHTML: {error}")),
        }
    }
    Ok(output.trim().to_string())
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

fn read_zip_text<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
    max_bytes: u64,
    cancellation: &CancellationToken,
) -> Result<String, String> {
    if cancellation.is_requested() {
        return Err("EPUB text extraction was cancelled.".into());
    }
    let mut entry = archive
        .by_name(name)
        .map_err(|error| format!("EPUB entry {name} is unavailable: {error}"))?;
    if !entry.is_file() {
        return Err(format!("EPUB entry {name} is not a regular file."));
    }
    if entry.size() > max_bytes {
        return Err(format!(
            "EPUB entry {name} exceeds the {} MiB extraction limit.",
            max_bytes / (1024 * 1024)
        ));
    }

    let mut bytes = Vec::with_capacity(entry.size().min(max_bytes) as usize);
    let mut buffer = vec![0u8; READ_BUFFER_BYTES];
    loop {
        if cancellation.is_requested() {
            return Err("EPUB text extraction was cancelled.".into());
        }
        let count = entry
            .read(&mut buffer)
            .map_err(|error| format!("Could not read EPUB entry {name}: {error}"))?;
        if count == 0 {
            break;
        }
        if bytes.len().saturating_add(count) > max_bytes as usize {
            return Err(format!(
                "EPUB entry {name} expanded beyond its extraction limit."
            ));
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    String::from_utf8(bytes).map_err(|error| format!("EPUB entry {name} is not UTF-8: {error}"))
}

fn parent_epub_path(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default()
}

fn resolve_epub_href(base: &str, href: &str) -> Result<String, String> {
    let href = href.split(['#', '?']).next().unwrap_or(href);
    if href.contains("://") || href.starts_with("data:") {
        return Err(format!("EPUB spine resource uses an external URI: {href}"));
    }
    let decoded = percent_decode(href)?;
    let combined = if base.is_empty() {
        decoded
    } else {
        format!("{base}/{decoded}")
    };
    normalize_archive_path(&combined, true)
}

fn normalize_archive_path(path: &str, allow_parent_segments: bool) -> Result<String, String> {
    let path = path.replace('\\', "/");
    if path.starts_with('/') {
        return Err(format!("EPUB archive path must be relative: {path}"));
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." if allow_parent_segments => {
                if parts.pop().is_none() {
                    return Err(format!(
                        "EPUB archive path escapes the archive root: {path}"
                    ));
                }
            }
            ".." => return Err(format!("EPUB archive path is unsafe: {path}")),
            value => parts.push(value),
        }
    }
    if parts.is_empty() {
        return Err("EPUB archive path is empty.".into());
    }
    Ok(parts.join("/"))
}

fn percent_decode(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(format!(
                    "EPUB href has an incomplete percent escape: {value}"
                ));
            }
            let high = hex_value(bytes[index + 1])
                .ok_or_else(|| format!("EPUB href has an invalid percent escape: {value}"))?;
            let low = hex_value(bytes[index + 2])
                .ok_or_else(|| format!("EPUB href has an invalid percent escape: {value}"))?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).map_err(|error| format!("EPUB href is not valid UTF-8: {error}"))
}

fn hex_value(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
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

fn is_block_element(name: &[u8]) -> bool {
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

fn append_text(output: &mut String, pending_space: &mut bool, text: &str) {
    for character in text.chars() {
        if character.is_whitespace() {
            *pending_space = !output.is_empty();
            continue;
        }
        if *pending_space && !output.ends_with('\n') {
            output.push(' ');
        }
        output.push(character);
        *pending_space = false;
    }
}

fn append_boundary(output: &mut String, pending_space: &mut bool) {
    while output.ends_with(' ') {
        output.pop();
    }
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    *pending_space = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_finds_namespaced_rootfile() {
        let xml = r#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
        assert_eq!(
            parse_container_package_path(xml).unwrap(),
            "OPS/package.opf"
        );
    }

    #[test]
    fn package_preserves_linear_spine_and_skips_navigation() {
        let xml = r#"<package xmlns="http://www.idpf.org/2007/opf"><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="c1" href="Text/ch%201.xhtml" media-type="application/xhtml+xml"/><item id="notes" href="notes.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="nav"/><itemref idref="c1"/><itemref idref="notes" linear="no"/></spine></package>"#;
        let package = parse_package_document(xml).unwrap();
        assert_eq!(package.spine, vec!["nav", "c1"]);
        assert!(package.manifest["nav"].is_navigation);
        assert_eq!(
            resolve_epub_href("OPS", &package.manifest["c1"].href).unwrap(),
            "OPS/Text/ch 1.xhtml"
        );
    }

    #[test]
    fn xhtml_text_keeps_reading_order_and_ignores_nonprose_content() {
        let xml = r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Duplicate title</title><style>.x{}</style></head><body><h1>Chapter &amp; One</h1><p>Hello <em>world</em>.</p><script>ignore me</script><p>Second&nbsp;paragraph<br/>next line.</p></body></html>"#;
        let text = extract_xhtml_text(xml).unwrap();
        assert_eq!(
            text,
            "Chapter & One\nHello world.\nSecond paragraph\nnext line."
        );
        assert!(!text.contains("Duplicate"));
        assert!(!text.contains("ignore me"));
    }

    #[test]
    fn archive_paths_cannot_escape_root() {
        assert!(normalize_archive_path("../package.opf", false).is_err());
        assert!(resolve_epub_href("OPS", "../../outside.xhtml").is_err());
        assert_eq!(
            resolve_epub_href("OPS/package", "../Text/chapter.xhtml#frag").unwrap(),
            "OPS/Text/chapter.xhtml"
        );
    }
}
