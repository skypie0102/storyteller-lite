from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one replacement target, found {count}")
    file.write_text(text.replace(old, new, 1), encoding="utf-8")


replace_once(
    "crates/storyteller-core/src/epub_graphic.rs",
    r'''    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<smil xmlns=\"http://www.w3.org/ns/SMIL\" xmlns:epub=\"http://www.idpf.org/2007/ops\" version=\"3.0\"><body><seq>",
    );''',
    r'''    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<smil xmlns=\"http://www.w3.org/ns/SMIL\" xmlns:epub=\"http://www.idpf.org/2007/ops\" version=\"3.0\"><body><seq epub:textref=\"{}\">",
        escape_xml(text_href),
    );''',
)
replace_once(
    "crates/storyteller-core/src/epub_graphic.rs",
    '        assert!(smil.find("image-anchor").unwrap() < smil.find("text-anchor").unwrap());',
    '''        assert!(smil.find("image-anchor").unwrap() < smil.find("text-anchor").unwrap());\n        assert!(smil.contains("epub:textref=\\\"../Text/ch.xhtml\\\""));''',
)

replace_once(
    "crates/storyteller-core/src/epub_overlay.rs",
    r'''        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<smil xmlns=\"http://www.w3.org/ns/SMIL\" xmlns:epub=\"http://www.idpf.org/2007/ops\" version=\"3.0\"><body><seq epub:type=\"{}\"><par id=\"stl-extra-par-{}\"><text src=\"{}#{}\"/><audio src=\"{}\" clipBegin=\"{}\" clipEnd=\"{}\"/></par></seq></body></smil>",''',
    r'''        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<smil xmlns=\"http://www.w3.org/ns/SMIL\" xmlns:epub=\"http://www.idpf.org/2007/ops\" version=\"3.0\"><body><seq epub:type=\"{}\" epub:textref=\"{}\"><par id=\"stl-extra-par-{}\"><text src=\"{}#{}\"/><audio src=\"{}\" clipBegin=\"{}\" clipEnd=\"{}\"/></par></seq></body></smil>",''',
)
replace_once(
    "crates/storyteller-core/src/epub_overlay.rs",
    '''        epub_type,\n        sequence + 1,''',
    '''        epub_type,\n        escape_xml(text_href),\n        sequence + 1,''',
)
replace_once(
    "crates/storyteller-core/src/epub_overlay.rs",
    r'''    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<smil xmlns=\"http://www.w3.org/ns/SMIL\" xmlns:epub=\"http://www.idpf.org/2007/ops\" version=\"3.0\"><body><seq>",
    );''',
    r'''    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<smil xmlns=\"http://www.w3.org/ns/SMIL\" xmlns:epub=\"http://www.idpf.org/2007/ops\" version=\"3.0\"><body><seq epub:textref=\"{}\">",
        escape_xml(text_href),
    );''',
)

replace_once(
    "crates/storyteller-core/src/epub_validate.rs",
    '''                if name == b"par" {\n                    if in_par {''',
    '''                if name == b"seq" {\n                    let textref = attribute_value(&element, b"textref")?\n                        .ok_or("Media Overlay seq is missing epub:textref.")?;\n                    validate_textref_target(\n                        &textref,\n                        smil_dir,\n                        expected_xhtml_path,\n                        archive,\n                        xhtml_ids,\n                        cancellation,\n                    )?;\n                } else if name == b"par" {\n                    if in_par {''',
)
replace_once(
    "crates/storyteller-core/src/epub_validate.rs",
    '''fn validate_text_target<R: Read + Seek>(\n''',
    '''fn validate_textref_target<R: Read + Seek>(\n    src: &str,\n    smil_dir: &str,\n    expected_xhtml_path: &str,\n    archive: &mut ZipArchive<R>,\n    cache: &mut HashMap<String, HashSet<String>>,\n    cancellation: &CancellationToken,\n) -> Result<(), String> {\n    if src.contains('#') {\n        return validate_text_target(\n            src,\n            smil_dir,\n            expected_xhtml_path,\n            archive,\n            cache,\n            cancellation,\n        );\n    }\n    let archive_path = resolve_archive_href(smil_dir, src)?;\n    if archive_path != expected_xhtml_path {\n        return Err(format!(\n            "Media Overlay textref target {archive_path} does not match its associated XHTML item {expected_xhtml_path}."\n        ));\n    }\n    Ok(())\n}\n\nfn validate_text_target<R: Read + Seek>(\n''',
)
