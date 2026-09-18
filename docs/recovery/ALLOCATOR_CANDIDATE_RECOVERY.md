# Manual allocator destination-candidate recovery — v0.39.0

This note records how the old unmatched-audio allocator bounded EPUB image/page choices. It is useful for the reduced Lite allocator because it establishes a strong safety principle: **the UI chooses from validated candidates; it does not submit arbitrary EPUB paths.**

The old helper was edge-focused, so its exact before/after algorithm should not replace Lite's general internal-unmatched-segment review. Preserve the bounding/validation pattern, not the old edge-only scope.

## Spine-window candidate selection

For old Introduction/Credits edge audio, candidate documents were derived from the EPUB spine relative to the nearest synchronized anchor.

Historical bounds:

- up to **12 XHTML documents** per side;
- Introduction: XHTML spine documents immediately **before** the first aligned anchor;
- Credits: XHTML spine documents immediately **after** the last aligned anchor;
- only `application/xhtml+xml` manifest items were accepted as document candidates.

For a future Lite internal unmatched segment, the natural generalization is to bound candidates using the nearest accepted match before and after the segment rather than scanning the entire publication.

## Image candidate discovery

Within those bounded documents the helper inspected:

- `<img>`
- SVG `<image>`
- `<object>`
- nested SVG references
- relevant CSS `url(...)` references

It also gathered embedded textual hints from:

- `alt`
- `title`
- SVG `<text>`

Generic hints such as `cover`, `book cover`, `cover image`, `front cover`, `image`, and `illustration` were ignored as low-information text.

For Introduction candidates, the OPF item carrying the `cover-image` property was inserted as an additional image candidate when available.

Historical image bound: at most **24 unique raster-image candidates** after deduplication.

These counts are useful historical limits, not mandatory Lite constants.

## Safe path resolution

Old candidate discovery rejected resource references that were unsafe or not package-local.

It ignored/rejected references with schemes such as:

- `data:`
- `http:` / `https:`
- `mailto:`
- `tel:`
- `javascript:`

Network locations were rejected, and package-relative paths were normalized through a safe book-path resolver rather than trusting raw XHTML attributes.

Nested SVG/image references were recursively resolved only while staying inside package members.

This is a strong invariant to preserve in native Rust candidate generation.

## Image-to-document ownership

The allocator did not return a bare image filename alone. It resolved the image back to a bounded XHTML owner document and returned pairs equivalent to:

```text
{
    document_path,
    image_path,
    label: "<document filename> — <image filename>"
}
```

Only images with a resolved eligible owner document were surfaced to the UI.

Manual `image` allocations were later validated against this generated candidate set. The user could not invent an arbitrary `document_path` / `image_path` pair.

## Lite interpretation

For the reduced Rust + Slint allocator, preserve these principles:

1. **Generate candidates in core logic, not UI logic.**
2. Bound the search to EPUB order/context around the unresolved audio's neighboring accepted matches.
3. Return stable structured candidate IDs plus display metadata; do not make path strings the primary user-facing identity.
4. Validate the selected candidate again when applying the review decision.
5. Reject external, escaping, stale, or no-longer-existing resources.
6. Keep image ownership explicit: an image assignment needs both the actual package image resource and the XHTML/SVG document that can legally host the narration.
7. Use embedded text hints before invoking OCR; OCR should remain lazy and bounded to candidates that actually need it.

## Generalizing beyond old edge audio

The old helper only had first/last aligned anchors because it focused on Introduction/Credits.

Lite's current alignment can expose unmatched segments anywhere in the audiobook. A suitable generalized candidate interval is:

```text
nearest accepted match before unresolved segment
        |
        v
allowed EPUB-order candidate region
        ^
        |
nearest accepted match after unresolved segment
```

Where both neighbors exist, candidate text/image destinations should remain between them in reading order unless a deliberate supplemental-page classification (for example Introduction/Credits at a publication edge) provides a different validated rule.

Where only one neighbor exists, use a small bounded spine window on the open side rather than the entire book.

This preserves monotonicity and makes stale/incorrect manual jumps much harder.

## Historical helper functions behind this evidence

Reference-only function names recovered from the GPL helper:

- `_graphic_references`
- `_edge_graphic_candidates`
- `_edge_document_paths`
- `_edge_image_documents`
- `inspect_manual_unmatched_audio`

Do not mirror these functions mechanically in Lite; use them as behavioral evidence for native Rust APIs and tests.
