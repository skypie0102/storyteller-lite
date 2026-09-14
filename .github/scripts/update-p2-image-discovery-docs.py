from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one match, found {count}")
    return text.replace(old, new, 1)


roadmap_path = Path("docs/ROADMAP.md")
roadmap = roadmap_path.read_text(encoding="utf-8")
roadmap = replace_once(
    roadmap,
    """Still pending in Review Audio:\n\n- Lazy image candidate extraction/OCR and Graphic Readout classification/assignment are not implemented yet.\n- Extra Audio has a classification value but no dedicated destination/rendering behavior yet.\n- Dedicated end-to-end supplemental/Smart regression fixtures are still needed beyond the core policy tests and workspace validation gates.\n""",
    """Still pending in Review Audio:\n\n- Bounded EPUB image candidate discovery is now implemented. It reads the package/spine directly so image-only XHTML pages omitted by the text corpus remain discoverable, bounds work between neighboring matched anchors, defaults to at most 12 nearby documents and 24 images, accepts only manifest-declared image resources, skips empty/missing/oversized (>25 MiB) images, and extracts embedded `alt` / `title` / SVG `title` / `desc` / `text` hints before OCR.\n- Lazy OCR and high-confidence Graphic Readout classification/assignment are not implemented yet; discovery alone does not auto-assign narration.\n- Extra Audio has a classification value but no dedicated destination/rendering behavior yet.\n- Dedicated supplemental/Smart regression coverage is now integrated and exercises automatic + manual decisions through EPUB build and validation.\n""",
    "roadmap review pending",
)
roadmap = replace_once(
    roadmap,
    """Validated source commit: `b868eb0cae9c5916ae1048c18cd2c66bf8f6ee8c`.\n\n### Encode — implemented\n""",
    """Validated source commit: `b868eb0cae9c5916ae1048c18cd2c66bf8f6ee8c`.\n\nSupplemental/Smart end-to-end regression coverage passed Windows validation on GitHub Actions run `34746062156`. The fixture verifies Smart automatic Introduction + manual Credits draft restoration, OPF manifest/spine order, generated XHTML+SMIL, exact clip timing/duration metadata, and final independent EPUB validation.\n\nBounded image candidate discovery passed Windows validation on GitHub Actions run `34811582552`:\n\n- `cargo fmt --all -- --check`\n- `cargo clippy --workspace --all-targets -- -D warnings`\n- `cargo test --workspace`\n- `cargo build -p storyteller-ui`\n\nValidated image-discovery source commit: `20954ba2c128e7c396e870b2fb5a9f382490450e`.\n\n### Encode — implemented\n""",
    "roadmap validation insertion",
)
roadmap = replace_once(
    roadmap,
    """Still pending:\n\n- validated image-bound Graphic Readout rendering;\n- any deliberately chosen Extra Audio rendering semantics;\n- dedicated regression fixtures that exercise supplemental spine ordering/manifest/duration behavior end-to-end, beyond the workspace compile/test gates already passed.\n""",
    """Still pending:\n\n- validated image-bound Graphic Readout rendering;\n- any deliberately chosen Extra Audio rendering semantics.\n\nDedicated supplemental regression coverage is now integrated and checks spine ordering, manifest/overlay relationships, real clip timing/duration metadata, Smart/manual review mixes, and final validation.\n""",
    "roadmap build pending",
)
roadmap = replace_once(
    roadmap,
    """8. Conservative Smart edge preservation: safe anchored leading/trailing narration is automatically assigned to supplemental Introduction/Credits pages, while ReviewAll keeps those regions Pending and Smart never auto-discards audio.\n\nNext implementation sequence:\n\n1. Add dedicated regression tests for supplemental page manifest/spine order, SMIL clips, duration metadata, restart durability, and Smart/manual mixes.\n2. Add bounded EPUB image candidate discovery: nearby reading-order documents first, embedded `alt` / `title` / SVG text hints before OCR.\n3. Add lazy OCR only for bounded image candidates needed by Smart or the current unresolved segment; do not restore a permanent OCR setting.\n4. Implement high-confidence Graphic Readout classification and validated image/page assignment without allowing overlap/double allocation.\n5. Extend Build EPUB for validated image-bound Graphic Readout narration.\n6. Decide whether Extra Audio needs a distinct Lite destination/rendering rule; if not, do not grow the taxonomy merely for historical compatibility.\n7. Keep every weak/ambiguous region Pending for manual review and independently audit final output.\n""",
    """8. Conservative Smart edge preservation: safe anchored leading/trailing narration is automatically assigned to supplemental Introduction/Credits pages, while ReviewAll keeps those regions Pending and Smart never auto-discards audio.\n9. Dedicated supplemental/Smart regression coverage across draft restoration, package/spine order, generated XHTML+SMIL, timing/duration metadata, and final validation.\n10. Bounded EPUB image candidate discovery, including image-only spine documents, monotonic neighbor bounds, default 12-document / 24-image caps, manifest resource validation, a 25 MiB image ceiling, and embedded `alt` / `title` / SVG text hints before OCR.\n\nNext implementation sequence:\n\n1. Add lazy OCR only for bounded image candidates needed by Smart or the current unresolved segment; do not restore a permanent OCR setting.\n2. Add deterministic evidence scoring that prefers embedded hints and uses OCR only as fallback; keep weak/ambiguous regions Pending.\n3. Implement high-confidence Graphic Readout classification and validated image/page assignment without allowing overlap/double allocation.\n4. Extend Build EPUB for validated image-bound Graphic Readout narration.\n5. Decide whether Extra Audio needs a distinct Lite destination/rendering rule; if not, do not grow the taxonomy merely for historical compatibility.\n6. Independently audit every new destination/rendering path in Validate.\n""",
    "roadmap P2 sequence",
)
roadmap_path.write_text(roadmap, encoding="utf-8")

handoff_path = Path("docs/HANDOFF.md")
handoff = handoff_path.read_text(encoding="utf-8")
handoff = replace_once(
    handoff,
    """- P2 now includes durable per-segment review decisions, the reduced allocator foundation, native edge/silence evidence, real supplemental Introduction/Credits rendering, and behaviorally distinct Smart/ReviewAll edge handling.\n- Supplemental edge-page rendering passed Windows validation on run `34738027167`; validated source commit `cea590ada02c7942a8fda7226631f22d503e55b1` is included in recovery.\n- Smart edge preservation passed Windows validation on run `34745516558`; validated source commit `b868eb0cae9c5916ae1048c18cd2c66bf8f6ee8c` is included in recovery.\n""",
    """- P2 now includes durable per-segment review decisions, the reduced allocator foundation, native edge/silence evidence, real supplemental Introduction/Credits rendering, behaviorally distinct Smart/ReviewAll edge handling, dedicated supplemental regression coverage, and bounded EPUB image candidate discovery.\n- Supplemental edge-page rendering passed Windows validation on run `34738027167`; validated source commit `cea590ada02c7942a8fda7226631f22d503e55b1` is included in recovery.\n- Smart edge preservation passed Windows validation on run `34745516558`; validated source commit `b868eb0cae9c5916ae1048c18cd2c66bf8f6ee8c` is included in recovery.\n- Supplemental/Smart end-to-end regression coverage passed Windows validation on run `34746062156`.\n- Bounded image candidate discovery passed Windows validation on run `34811582552`; validated source commit `20954ba2c128e7c396e870b2fb5a9f382490450e` is included in recovery.\n""",
    "handoff repository truth",
)
handoff = replace_once(
    handoff,
    """- Effective alignment materialization converts validated manual text assignments into matched blocks while retaining the original automatic alignment as source truth.\n\nCurrent edge evidence and Smart behavior:\n""",
    """- Effective alignment materialization converts validated manual text assignments into matched blocks while retaining the original automatic alignment as source truth.\n- Bounded image candidate discovery reads the EPUB package/spine directly, so image-only XHTML pages remain visible even when the text corpus omits them. Candidate work is bounded between neighboring accepted alignment anchors, defaults to at most 12 nearby spine documents / 24 images, requires manifest-declared image resources, skips empty/missing/images over 25 MiB, and gathers `alt`, `title`, SVG `title`/`desc`/`text` hints before OCR.\n- Image discovery is evidence-only at this stage: it does not run OCR, classify Graphic Readout automatically, or create an image assignment.\n\nCurrent edge evidence and Smart behavior:\n""",
    "handoff allocator image discovery",
)
handoff = replace_once(
    handoff,
    """Smart edge-policy validation run `34745516558` passed the same three gates. Validated Smart source commit: `b868eb0cae9c5916ae1048c18cd2c66bf8f6ee8c`.\n\nImportant remaining validation debt: add dedicated regression fixtures for supplemental OPF manifest/spine ordering, generated XHTML+SMIL clip relationships, duration metadata, review-draft restoration, and Smart/manual mixtures. Existing workspace tests/build gates passed, but this path deserves explicit feature fixtures before P2 is considered complete.\n""",
    """Smart edge-policy validation run `34745516558` passed the same three gates. Validated Smart source commit: `b868eb0cae9c5916ae1048c18cd2c66bf8f6ee8c`.\n\nDedicated supplemental/Smart regression coverage passed Windows validation on run `34746062156`. It verifies OPF manifest/spine ordering, generated XHTML+SMIL relationships, real clip timing and duration metadata, durable review-draft restoration, a mixed automatic Introduction/manual Credits decision set, and final independent EPUB validation.\n\nBounded image candidate discovery passed Windows validation on run `34811582552` (format check, strict Clippy, full workspace tests, native Slint build). Validated image-discovery source commit: `20954ba2c128e7c396e870b2fb5a9f382490450e`.\n""",
    "handoff validation debt",
)
handoff = replace_once(
    handoff,
    """1. **Add dedicated supplemental/Smart regression fixtures** for OPF manifest/spine ordering, XHTML+SMIL clip timing, duration metadata, review-draft restoration, and mixed automatic/manual decisions.\n2. **Add bounded image candidate discovery** near the relevant reading-order edge/current segment. Consume embedded `alt`, `title`, and SVG text hints before OCR.\n3. **Add lazy OCR** only when Smart or the current unresolved review item actually needs image text evidence. Do not restore a permanent OCR setting.\n4. **Implement high-confidence Graphic Readout assignment** to a validated image/page destination, preventing overlapping/double allocation.\n5. **Extend Build EPUB for Graphic Readout** and add corresponding validation fixtures.\n6. Decide whether **Extra Audio** needs a distinct Lite destination/rendering rule. If it does not, do not broaden the taxonomy just because the old app had more modes.\n7. Keep weak/ambiguous/non-edge regions Pending unless a deterministic auditable rule is added and regression-tested.\n""",
    """1. **Add lazy OCR** only when Smart or the current unresolved review item actually needs text evidence from one of the bounded image candidates. Do not restore a permanent OCR setting.\n2. **Add deterministic image-evidence scoring** that prefers embedded `alt` / `title` / SVG text and uses OCR only as fallback. Weak/ambiguous evidence stays Pending.\n3. **Implement high-confidence Graphic Readout assignment** to a validated image/page destination, preventing overlapping/double allocation.\n4. **Extend Build EPUB for Graphic Readout** and add corresponding validation fixtures.\n5. Decide whether **Extra Audio** needs a distinct Lite destination/rendering rule. If it does not, do not broaden the taxonomy just because the old app had more modes.\n6. Keep weak/ambiguous/non-edge regions Pending unless a deterministic auditable rule is added and regression-tested.\n""",
    "handoff immediate order",
)
handoff_path.write_text(handoff, encoding="utf-8")
