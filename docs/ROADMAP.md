# Storyteller Lite roadmap

> **Canonical recovery roadmap.** Read `docs/HANDOFF.md` before substantial work. Live Rust + Slint source on `recovery/rust-slint` is technical truth; this file records current product scope and pending work.

Storyteller Lite is the Rust + Slint successor to Storyteller OneClick. The legacy app is a behavioral reference where useful, but Lite is intentionally smaller and is not a line-for-line port.

## Product contracts

- Queue-first workflow with one foreground book pipeline at a time.
- Fixed seven-stage flow: Prepare → Analyze → Align → Review Audio → Encode → Build EPUB → Validate.
- Analyze may internally run bounded Whisper chunks concurrently.
- Failed books remain Failed/retryable and the queue continues unless explicitly paused.
- Long-running work never runs on the Slint UI thread.
- Exactly one weighted overall progress bar, backed only by real structured measurements.
- Source files are never intentionally overwritten; output defaults next to the source EPUB.
- Resume reuses only a validated contiguous prefix of checkpoints; stale semantics invalidate downstream stages.
- Human review decisions are explicit and durable.
- Smart decisions are conservative, auditable, and reversible before publication.
- Weak or ambiguous evidence stays unresolved instead of being forced.
- Publication happens only after independent structural validation.
- EPUB destinations are bounded/revalidated; arbitrary paths from UI state are never authoritative.
- GitHub-hosted Actions are intentionally opt-in/manual-only. Do not create temporary validation PRs/workflows or dispatch hosted runners unless the user explicitly asks.

## Scope kept in Lite

Keep queue-first processing, native Rust + Slint, automatic CPU-thread selection, one simple Whisper worker count (1–4, default 1), Smart/ReviewAll unmatched-audio policy, conservative edge handling, bounded/lazy OCR, reduced manual allocation, and only the destination/classification types that materially change output behavior.

Do not restore manual CPU allocation, word-level synchronization, the old activity-console-first UX, Runtime Health, process-now, broad runtime-path controls, EPUB standardization/CSS editor toggles, permanent OCR controls, or the legacy general-purpose split/merge/trim/rules allocator.

## Current implementation status

### P0/P1 foundations — complete

The Rust workspace, queue/progress/resume/scheduler/worker architecture, cancellation, source fingerprinting/staging, and seven-stage runner are implemented. Queue auto-advance after failures matches the recovered product behavior.

Analyze uses deterministic bounded audio chunks rather than a permanent full-book PCM artifact. Chapter boundaries are preferred, synthetic boundaries are silence-refined, at most the selected 1–4 Whisper processes run concurrently, available logical CPU threads are divided across active workers, and chunk-local timestamps are merged into one validated global transcript. Temporary PCM chunks are removed after use.

Windows validation for P1: run `34732299289`.

### Align — implemented

The conservative monotonic block-safe aligner uses Whisper timestamps as timing authority, accepts strong monotonic EPUB matches, prevents accepted matches from crossing normalized XHTML block boundaries, and leaves weak evidence unmatched.

### P2 Review Audio — substantially complete

Implemented:

1. Durable per-segment Pending / Assigned / Excluded decisions with Automatic / Manual provenance and restart-safe `review-draft.json` persistence.
2. Reduced Smart / ReviewAll policy wiring.
3. Native leading/trailing edge identity and bounded silence evidence.
4. Monotonic bounded EPUB text candidates plus manual text assignment/exclusion.
5. Reduced allocator navigation, preview/seek, transcript/timing/context, assign/exclude, and completion gating.
6. Effective reviewed text alignment materialization while retaining original automatic alignment as source truth.
7. Manual Introduction/Credits preservation through generated supplemental XHTML + SMIL pages.
8. Conservative Smart edge preservation: anchored leading/trailing narration becomes supplemental Introduction/Credits in Smart; ReviewAll leaves it Pending; Smart never auto-discards unmatched audio.
9. Dedicated supplemental/Smart regression coverage through final validation.
10. Bounded EPUB image candidate discovery, including image-only spine documents, monotonic neighboring bounds, default 12-document / 24-image caps, manifest resource validation, 25 MiB ceiling, and embedded `alt` / `title` / SVG text hints.
11. Lazy per-candidate image text evidence with embedded hints first and optional per-call Tesseract fallback; no permanent OCR setting.
12. Deterministic transcript-to-image evidence scoring with conservative phrase/coverage/similarity/distinctive-word thresholds and ambiguity margin.
13. Smart high-confidence Graphic Readout assignment for pending non-edge items only, with manual decisions preserved, ReviewAll kept non-automatic, and duplicate target ownership rejected.
14. Graphic Readout EPUB rendering to the existing image target using native Media Overlay cues. Text and image narration on the same XHTML share one SMIL sequence; cues are audio-time ordered and overlap/duplicate targets are rejected.
15. End-to-end Graphic Readout fixture covering mixed text/image overlays and final structural validation.

Validation history:

- supplemental rendering: `34738027167`;
- Smart edge preservation: `34745516558`;
- supplemental/Smart regression: `34746062156`;
- bounded image discovery: `34811582552`;
- lazy image evidence: `34815433905`;
- deterministic image scoring: `34820045285`;
- Smart Graphic Readout assignment + rendering: `34826426511`.

The Graphic Readout slice is integrated in recovery as commit `9673dc7a6f41493e13b24724b3123659fcbaa271`.

### Encode — implemented

Copy mode performs cancellable byte-preserving copy only for safely mappable EPUB audio types. Opus/AAC use FFmpeg structured progress. `encoded-audio.json` records the packaged audio identity.

### Build EPUB — text, supplemental edge pages, and Graphic Readout implemented

For supported EPUB 3 sources without pre-existing Media Overlays, the builder preserves unrelated resources, creates Media Overlay/package relationships, injects deterministic XHTML/image anchors as needed, writes real audiobook clip times, and embeds the encoded audio.

Build consumes:

- effective reviewed text alignment;
- supplemental Introduction/Credits review destinations;
- Graphic Readout image destinations directly from review data.

Graphic Readout can share an XHTML/SMIL sequence with normal text cues or create a section overlay for an image-only page. Duplicate image assignment and overlapping audio ownership fail explicitly.

### Validate — implemented

Validate independently reopens the candidate EPUB and audits package/SMIL/text-or-image/audio relationships, clip ranges, duration consistency, duplicates, resource resolution, and mimetype rules before publication. New rendering paths must extend regression coverage rather than weakening this audit.

## P2 remaining work

### Manual Graphic Readout allocator — active feature branch

Branch: `feature/manual-graphic-readout` at `3835be6832182b9b3c7b684cb499c1f71888511f`.

Goal: unresolved/ambiguous **non-edge** narration may be manually attached to a nearby discovered image, but only through the same bounded candidate model used by Smart. The UI must never authorize a free-form `(document_href, image_href)` pair.

Work in progress currently includes:

- a core manual assignment API that reruns bounded image discovery before persisting the decision;
- duplicate-image and edge-region rejection;
- focused pure validation tests;
- candidate-list UI work that mixes a small number of clearly labeled Graphic Readout rows with existing text candidates and revalidates the selected image on click;
- a job-specific candidate cache so EPUB image discovery is not repeated on every UI refresh and cannot carry across jobs;
- advisory image discovery so an image-scanning problem does not remove otherwise-valid text assignment choices.

Remaining before integration:

1. complete formatting/Clippy-oriented static review;
2. run compile/tests/native Slint build in a non-hosted environment when available, or use GitHub-hosted CI only after explicit user authorization;
3. fix any validation findings;
4. integrate only after green validation;
5. during allocator UI polish, rename the current `EPUB TEXT CANDIDATES` heading to reflect mixed text/image rows without expanding the UI into the old editor.

### Extra Audio — no renderer planned by default

`ExtraAudio` exists in the small classification enum, but recovered evidence does not establish a distinct required Lite output contract. A historical standalone audio-player page is only a possible product/interoperability fallback, not compatibility debt. Do **not** add a player-page/Extra Audio renderer merely because the old application had an `other` category. Add behavior only when there is a concrete user-facing requirement and a validation strategy.

After the manual Graphic Readout allocator is coherent, P2 should be considered functionally complete unless real books expose another narrowly scoped unmatched-audio gap.

## P3 — main Slint UI alignment

Bring the main experience closer to the supplied UI guides without turning them into a fixed pixel canvas:

- compact creation controls;
- one rich processing card;
- clear seven-stage visualization;
- real timing/backend/model/match metrics only when available;
- queue/recent management that remains visible and understandable;
- responsive reflow for narrower windows;
- reduced allocator presentation that clearly separates text/image choices without restoring the old editor complexity.

## P4 — installer archaeology only when needed

Inspect the supplied v0.39.0 installer only when a live Lite behavior is still ambiguous. Verify documented provenance/hash first and record evidence before implementation. Do not broaden scope simply because a legacy feature exists.

## P5 — later polish and compatibility

After P2/P3 stabilize:

- explicit relaunch/checkpoint-resume UX once persistence behavior is fully defined;
- diagnostics and packaging/release/update polish;
- EPUBCheck and reading-system interoperability testing;
- deliberate additional EPUB compatibility policy only where needed;
- benchmark 1–4 Whisper workers across representative CPU/CUDA systems before changing defaults or adding VRAM/hardware-aware clamping.

## Engineering migration rule

Preserve old material until replacement behavior is implemented and regression-covered.

> **Replace → regression-test → delete. Never delete → hope we remembered everything.**
