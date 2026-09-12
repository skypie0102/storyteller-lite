# Storyteller Lite UI guides

These files are the current visual/layout references for the recovered Rust + Slint Lite project. They came from user-provided mockups in the recovery conversation and are design targets, not screenshots of the current implementation.

## Assets

- `storyteller-lite-main-ui.webp` — main create/processing/queue screen.
- `storyteller-lite-manual-allocation.webp` — manual unmatched-audio review/allocation screen.

The repository copies are downscaled WebP reference images so they remain lightweight. The original uploaded PNGs are not required to implement the layout; their SHA-256 hashes are retained below for provenance.

| Source | Original SHA-256 | Repo reference SHA-256 |
| --- | --- | --- |
| Main UI mockup | `bb79350c2404cdb10480cba1f0e4d38136f318a12b61e8437f619c2b690794e3` | `ad25e75ff3b0d5c759d8c2e5a20ffb2c344b34197f49e42dee6218e739c9931b` |
| Manual allocation mockup | `71007501f381dc5ba25ec9f70660dc4c8818dd643d49652da6b14cb8911cba3b` | `35425949ffcbecbf1935304ed07c66fb6ed908873dbbe4631a3bc5b16f01eaa5` |

## Main UI intent

Treat the main mockup as the hierarchy target:

- EPUB and audiobook drop/browse areas.
- Compact output options.
- Reduced unmatched-audio policy with **Smart** as the normal/default path and **ReviewAll** as the explicit review-heavy alternative; do not resurrect the old four-mode selector.
- One current processing card with exactly one overall progress bar.
- Seven pipeline stages: Prepare → Analyze → Align → Review Audio → Encode → Build EPUB → Validate.
- Real metrics only: elapsed, ETA, speed, backend, model, match percentage, and useful current activity/context.
- Queue-first workflow with waiting/recent books and pause-after-current.
- Settings remains a separate experience and includes the simple Whisper worker-count control without manual CPU-thread tuning.

The mockup is a wide-window target, not a fixed canvas. Slint should reflow, hide secondary metadata when necessary, and use localized scrolling instead of absolute positioning.

## Manual allocation intent

The allocator mockup preserves the useful old product idea, but Lite should implement a reduced review experience rather than the old general-purpose editor.

Required Lite behavior:

- dedicated review screen for unresolved/unaligned audio segments;
- previous/next unresolved segment navigation;
- audio preview/seek for the current segment;
- transcript text, timing, and useful silence/context information;
- Smart suggestion/classification when one exists;
- EPUB candidate block/page context;
- explicit manual assignment/override to valid EPUB text/block(s);
- explicit exclusion/skip decision under a validated policy;
- durable per-segment decisions with autosave/retry restoration;
- `Apply & Next` flow and a clear path back to processing.

The first Lite implementation should preserve monotonic EPUB order by constraining assignment candidates between the nearest accepted matched neighbors when possible.

## Smart classification and lazy OCR

The recovered Lite plan explicitly retained **automatic edge handling, Smart classification, and lazy OCR**. What was cut was the old permanent OCR toggle and the full allocator/editor surface.

Accordingly:

- OCR should be internal/on-demand, not a permanent user preference.
- Only bounded candidate EPUB images/documents should be inspected/OCRed when Smart classification or the current review segment needs them.
- Embedded `alt`/`title`/SVG text should be used before expensive OCR where practical.
- High-confidence Graphic Readout narration may be suggested/assigned to a valid image page/document candidate.
- Introduction, Credits, Graphic Readout, and Extra Audio may appear as a small useful classification set when they affect destination/build behavior or make the suggestion understandable.
- The UI does not need to expose the old application's entire taxonomy/rule system just because the backend can classify a segment.
- Ambiguous Smart results stay pending for human review rather than being silently discarded.

See `docs/recovery/UNMATCHED_AUDIO_RECOVERY.md` for the recovered behavioral evidence and historical test-vector thresholds.

## Intentionally reduced from the old allocator

The following mockup/old-app affordances are not initial Lite requirements unless a later product decision explicitly restores them:

- arbitrary split/merge editing;
- a general manual trim-handle editor and restore-original toolchain;
- permanent OCR enable/disable controls;
- broad automatic `Apply to similar` rules;
- unrestricted category/destination editing;
- the old four-mode unmatched-audio selector;
- a general-purpose old-app audio editor.

This does **not** remove Smart classification, lazy OCR, conservative automatic edge handling, or image-page/Graphic Readout placement from the intended Lite backend.

The goal is a focused allocator that makes unresolved audiobook content safe and understandable without recreating the entire historical OneClick editing surface.
