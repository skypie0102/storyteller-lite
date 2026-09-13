from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)


# The supplemental package helper has an End-event pattern that also exists in
# the pre-existing package rewrite. Scope the lifetime repair to the generated
# helper so validation cannot accidentally rewrite unrelated EPUB code.
overlay_path = Path("crates/storyteller-core/src/epub_overlay.rs")
overlay = overlay_path.read_text(encoding="utf-8")
helper_marker = "pub(crate) fn add_supplemental_package_items("
if overlay.count(helper_marker) != 1:
    raise RuntimeError("supplemental package helper: expected exactly one generated helper")
prefix, helper = overlay.split(helper_marker, 1)
helper = replace_once(
    helper,
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
    "supplemental helper end-tag state",
)
helper = replace_once(
    helper,
    '''                } else if name == b"metadata" {
                    for supplement in supplements {''',
    '''                } else if is_metadata {
                    for supplement in supplements {''',
    "supplemental helper metadata state",
)
helper = replace_once(
    helper,
    '''                if name == b"itemref" {
                    if let Some(idref) = start_itemref_after.take() {''',
    '''                if is_itemref {
                    if let Some(idref) = start_itemref_after.take() {''',
    "supplemental helper itemref state",
)
overlay_path.write_text(prefix + helper_marker + helper, encoding="utf-8", newline="\n")


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
