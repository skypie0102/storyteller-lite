# Manual allocator output semantics recovery — v0.39.0

This note records what the old manual/automatic unmatched-audio decisions actually changed in the finished EPUB. It is derived from static inspection of the user-provided v0.39.0 installer and its bundled GPL/Sigil-derived helper source.

The source is a behavioral/reference artifact only. Do not copy its implementation wholesale into Lite without an explicit licensing decision.

## Why this matters for Lite

The reduced Lite allocator is not only a UI problem. A decision such as **Introduction**, **Credits**, or **Graphic Readout** implies a different downstream EPUB structure.

Recovered behavior shows that the old product separated:

- **classification** — what kind of unmatched audio this is;
- **destination** — page, image, or discard;
- **rendering strategy** — synchronized Media Overlay page versus standalone embedded audio player.

Lite should preserve that separation in its data model rather than encoding everything into a single enum.

## Manual allocation targets recovered

Each reviewed allocation row had at least:

- `segment_id`
- local `start` / `end`
- `category`
- `target`
- optional `document_path`
- optional `image_path`
- ordering metadata

Historical categories:

- `introduction`
- `graphic_readout`
- `credits`
- `other`
- `silence`

Historical targets:

- `page`
- `image`
- `discard`

The old helper required every unmatched segment to be covered continuously. Page/image/discard rows could subdivide one segment, but no gaps or overlaps were allowed. `discard` was valid only for silence/noise.

## Introduction / Credits — synchronized transcript page

The old helper's `transcript` rendering mode created a genuine EPUB Media Overlay page.

For an Introduction or Credits region it:

1. created a new XHTML document, normally under a Storyteller-owned `Text/` path;
2. created a companion SMIL document under a Storyteller-owned `MediaOverlays/` path;
3. added both resources to the OPF manifest;
4. associated the XHTML manifest item with the SMIL item through `media-overlay`;
5. inserted the new XHTML into the spine immediately before/after the nearest aligned anchor;
6. added a Navigation/NCX entry;
7. created stable paragraph IDs;
8. generated one or more SMIL `<par>` entries with real audio clip times;
9. marked the SMIL sequence as `frontmatter` for Introduction or `backmatter` for Credits;
10. recalculated Media Overlay duration metadata afterward.

### Text content

The page text was built from transcription units where available. Bounded OCR/image-text hints could be used to correct recognized wording or provide fallback text when the audio itself had no usable transcript.

The helper also attempted to inherit presentation/style characteristics from the nearby publisher XHTML instead of always forcing a completely unrelated visual design.

### Lite implication

This is the strongest reference for the recovered Lite planning work that mentioned native Introduction/Credits read-aloud pages with SMIL.

A clean Lite implementation should prefer native Rust generation of XHTML + SMIL tied to the current EPUB model and final validator. The old helper's exact markup/CSS is not required for compatibility.

## Introduction / Credits — standalone audio-player page

The old `player` mode used a different strategy:

1. rendered the selected unmatched ranges into a Storyteller-owned `.m4a` audio file;
2. created a new XHTML page with an `<audio controls>` player;
3. inserted that page before/after the nearest aligned spine anchor;
4. added the page/audio files to the manifest;
5. added the page to navigation.

This page was **not** a synchronized SMIL Media Overlay page. It was simply a readable spine page containing an audio player.

### Lite implication

This is useful as a fallback concept when audio is worth preserving but trustworthy text synchronization is unavailable. It should be a deliberate Lite product/interoperability choice, not an automatic port of the old `automatic_player` mode.

The recovered Lite scope removed the old four-mode selector, so users should not be asked to choose between old `player` versus `transcript` implementation modes directly.

## Graphic Readout — attach to existing image page

The old player-oriented Graphic Readout path did **not** create a separate generic Introduction/Credits page.

Given a validated EPUB `document_path` + `image_path`, it:

1. rendered the selected narration into a Storyteller-owned `.m4a` asset;
2. added that audio asset to the OPF manifest;
3. found the actual image element in the existing XHTML/SVG context;
4. wrapped the displayed image in a Storyteller-owned container;
5. inserted a small play/pause button and `<audio>` element next to the image;
6. added a small Storyteller CSS/JavaScript helper to that XHTML;
7. marked the manifest item as scripted;
8. audited the final resource/reference graph.

This behavior closely matches the supplied Lite allocator mockup note that Graphic Readouts are normally placed on the corresponding image page with a small play button overlay.

### Interoperability warning

The recovered implementation depended on scripted XHTML behavior. Reading-system support for scripting/audio controls varies.

Lite should preserve the **product behavior** — narration belongs to the relevant image/page — without assuming the exact old JavaScript overlay is the best EPUB 3 implementation. Prefer the most interoperable/native approach that passes Lite's structural audit and target-reader testing.

## Ordering behavior

Introduction/Credits pages were inserted relative to the nearest aligned spine anchors:

- Introduction before the first aligned anchor;
- Credits after the last aligned anchor;
- image-bound Graphic Readouts stayed on their existing EPUB document/image destination.

Manual image destinations were restricted to candidates produced by the helper. Arbitrary `document_path` / `image_path` pairs were rejected.

That reinforces the Lite rule that destinations must be **bounded and validated**, not free-form paths supplied by the UI.

## Existing Media Overlay boundary adjustment

When unmatched edge audio overlapped the physical audio file used by existing synchronized content, the old helper could trim existing SMIL clip boundaries so the preserved Introduction/Credits region was not double-referenced.

For trailing unmatched audio it could also physically trim/split the packaged tail and rebuild duration metadata.

Lite should preserve the invariant — one audio interval should not be ambiguously owned twice — but implement it against the new Rust EPUB/audio graph rather than copying the old helper.

## Final audit

After applying manual allocations the old helper independently checked at least:

- Storyteller-owned audio assets existed and were non-empty;
- SMIL audio references were relative and resolved to package members;
- OPF/manifest output remained internally coherent;
- Media Overlay duration metadata was rebuilt.

This aligns with Lite's existing product contract that manual decisions are not trusted merely because the review UI accepted them: Build EPUB and Validate remain responsible for verifying the resulting publication.

## Recommended Lite data model implication

Do not make `Introduction`, `Credits`, or `GraphicReadout` themselves the entire decision state.

A better conceptual model is:

```text
ReviewDecision
  Pending
  Assigned {
      destination,
      classification?,
      source = Automatic | Manual,
      rendering_hint?
  }
  Excluded {
      reason,
      source = Automatic | Manual
  }
```

Where destination can represent at least:

```text
TextBlock / TextRange
Image { document, image }
SupplementalPage { placement = Before | After anchor }
```

`rendering_hint` should remain internal/product-driven rather than restoring the old `automatic_player` / `automatic_transcript` selector.

## Suggested Lite rendering policy to validate

This is an implementation direction inferred from the recovered Lite plan + old behavior, not a recovered hard requirement:

- **Normal manually assigned transcript segment** → synchronize to an existing valid XHTML block/range.
- **Introduction/Credits with trustworthy transcript text** → generated XHTML + SMIL supplemental read-aloud page.
- **Introduction/Credits without trustworthy text** → either leave for manual decision or use a deliberately supported standalone player-page fallback.
- **High-confidence Graphic Readout** → attach narration to the validated image page/resource using the most interoperable EPUB representation available.
- **Silence/noise** → explicit exclusion only under validated Smart/manual policy.

## Files/functions that produced this evidence

Recovered helper behaviors came from functions including:

- `_apply_manual_unmatched_audio`
- `_create_unmatched_page`
- `_create_audio_player_page`
- `_attach_graphic_readout_player`
- `_adjust_existing_overlay_boundaries`
- `_audit_manual_audio_output`
- `_rebuild_media_duration_metadata`

These names are recorded for archaeology only; Lite does not need to mirror the old implementation structure.
