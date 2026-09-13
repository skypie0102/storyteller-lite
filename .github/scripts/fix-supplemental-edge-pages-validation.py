from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)


# The supplemental package rewrite needs the element-name tests to outlive
# moving the quick-xml end event into the writer.
overlay_path = Path("crates/storyteller-core/src/epub_overlay.rs")
overlay = overlay_path.read_text(encoding="utf-8")
overlay = replace_once(
    overlay,
    '''            Ok(Event::End(element)) => {
                let qname = element.name();
                let name = local_name(qname.as_ref());
                if name == b"manifest" {''',
    '''            Ok(Event::End(element)) => {
                let qname = element.name();
                let name = local_name(qname.as_ref());
                let is_manifest = name == b"manifest";
                let is_metadata = name == b"metadata";
                let is_itemref = name == b"itemref";
                if is_manifest {''',
    "owned package end-tag state",
)
overlay = replace_once(
    overlay,
    '''                } else if name == b"metadata" {
                    for supplement in supplements {''',
    '''                } else if is_metadata {
                    for supplement in supplements {''',
    "metadata end-tag state",
)
overlay = replace_once(
    overlay,
    '''                if name == b"itemref" {
                    if let Some(idref) = start_itemref_after.take() {''',
    '''                if is_itemref {
                    if let Some(idref) = start_itemref_after.take() {''',
    "itemref end-tag state",
)
overlay_path.write_text(overlay, encoding="utf-8", newline="\n")


# Existing review-assignment unit tests construct ordinary text/image
# destinations directly. They must explicitly opt out of supplemental pages.
assign_path = Path("crates/storyteller-core/src/review_assignment.rs")
assign = assign_path.read_text(encoding="utf-8")
assign = replace_once(
    assign,
    '''                href: "OPS/ch1.xhtml".into(),
                line_index: Some(1),
                image_href: None,
            },''',
    '''                href: "OPS/ch1.xhtml".into(),
                line_index: Some(1),
                image_href: None,
                supplemental: None,
            },''',
    "matched text assignment fixture",
)
assign = replace_once(
    assign,
    '''                href: "OPS/ch2.xhtml".into(),
                line_index: Some(1),
                image_href: None,
            },''',
    '''                href: "OPS/ch2.xhtml".into(),
                line_index: Some(1),
                image_href: None,
                supplemental: None,
            },''',
    "out-of-window text assignment fixture",
)
assign = replace_once(
    assign,
    '''                href: "OPS/ch1.xhtml".into(),
                line_index: None,
                image_href: Some("OPS/image.png".into()),
            },''',
    '''                href: "OPS/ch1.xhtml".into(),
                line_index: None,
                image_href: Some("OPS/image.png".into()),
                supplemental: None,
            },''',
    "image assignment fixture",
)
assign_path.write_text(assign, encoding="utf-8", newline="\n")
