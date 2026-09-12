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
- One current processing card with exactly one overall progress bar.
- Seven pipeline stages: Prepare → Analyze → Align → Review Audio → Encode → Build EPUB → Validate.
- Real metrics only: elapsed, ETA, speed, backend, model, match percentage, and useful current activity/context.
- Queue-first workflow with waiting/recent books and pause-after-current.
- Settings remains a separate experience.

The mockup is a wide-window target, not a fixed canvas. Slint should reflow, hide secondary metadata when necessary, and use localized scrolling instead of absolute positioning.

## Manual allocation intent

The allocator mockup preserves the old product idea, but Lite should initially implement a reduced version.

Required Lite behavior:

- dedicated review screen for unresolved/unaligned audio segments;
- previous/next unresolved segment navigation;
- audio preview/seek for the current segment;
- transcript text and timing;
- EPUB candidate block/page context;
- explicit manual assignment to EPUB text/block(s);
- explicit exclusion/skip decision;
- durable per-segment decisions;
- `Apply & Next` flow and a clear path back to processing.

The first Lite implementation should preserve monotonic EPUB order by constraining assignment candidates between the nearest accepted matched neighbors when possible.

The following mockup affordances are reference-only unless a later product decision explicitly restores them:

- split/merge editing;
- manual trim handles and restore-original tooling;
- permanent OCR controls;
- automatic `Apply to similar` rules;
- the full Introduction / Credits / Graphic Readout / Extra Audio classification system;
- a general-purpose old-app audio editor.

The goal is to restore the missing manual allocator, not the entire historical OneClick feature surface.
