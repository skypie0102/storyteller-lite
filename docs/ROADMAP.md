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
- Recovery Actions are intentionally opt-in/manual-only. GitHub-hosted runners may be used as deliberate validation checkpoints; after a failure, inspect the full log/failure surface, batch fixes, and avoid repeated one-error-at-a-time runner cycles.

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

### P2 Review Audio — functionally complete

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
15. Manual Graphic Readout allocation for unresolved/ambiguous non-edge narration. The allocator exposes only bounded discovered image candidates; core reruns discovery before persistence, rejects edge narration and duplicate image ownership, and records Manual provenance.
16. Job-specific allocator candidate caching so image discovery is not repeated by the UI refresh loop or leaked across books; image discovery remains advisory and cannot erase text choices.
17. End-to-end Graphic Readout coverage for both automatic mixed text/image overlays and manual bounded-image rediscovery → durable decision → EPUB build → final structural validation.

Validation history:

- supplemental rendering: `34738027167`;
- Smart edge preservation: `34745516558`;
- supplemental/Smart regression: `34746062156`;
- bounded image discovery: `34811582552`;
- lazy image evidence: `34815433905`;
- deterministic image scoring: `34820045285`;
- Smart Graphic Readout assignment + rendering: `34826426511`;
- manual Graphic Readout allocation: final green run `34918789594`.

The automatic Graphic Readout slice was integrated as `9673dc7a6f41493e13b24724b3123659fcbaa271`. Manual bounded Graphic Readout allocation is integrated as `a27df630a8626d1c0ba846deca8f44acde125fe8`.

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

## Extra Audio — no renderer planned by default

`ExtraAudio` exists in the small classification enum, but recovered evidence does not establish a distinct required Lite output contract. A historical standalone audio-player page is only a possible product/interoperability fallback, not compatibility debt. Do **not** add a player-page/Extra Audio renderer merely because the old application had an `other` category. Add behavior only when there is a concrete user-facing requirement and a validation strategy.

P2 should now be treated as functionally complete unless real books expose another narrowly scoped unmatched-audio gap.

## P3 — main Slint UI alignment — active

Validated baseline now implemented:

- review presentation clearly represents mixed bounded text/image destinations and `… & Next` behavior;
- seven-stage visualization shows real elapsed time per stage when available;
- creation source pickers/options reflow below 900px while preserving the wide-window hierarchy;
- active `Queue Another Book`, review headings/actions, bottom job actions, and queue/recent entries also reflow below 900px;
- real timing/backend/model/match/current-activity data remains the only metrics surface;
- queue/recent management remains visible and understandable in both wide and compact layouts.

Validation runs:

- review presentation: `34919543203`;
- stage elapsed timing: `34926648718`;
- responsive creation layout: `34927154367`;
- responsive active/review/queue layout: `34927928639`.

Integrated recovery commits for those slices are `1a645685549a0796b960d18723bb0cd42799c79d`, `c79c834328edabc4fef72d0d0cd5fb8343499411`, `c8a6461eaeb4d2535af26172876b729cf0d2abf5`, and `17ef8d7948a04d74fd1f0ea3a60a0e8da140e8b8` respectively.

Remaining P3 work should stay presentation-focused:

- make NeedsReview read as a dedicated unresolved-audio surface rather than a normal processing card with an embedded allocator;
- add localized scrolling/overflow where real content can exceed the supported 620px minimum height;
- polish settings and secondary-metadata reflow/hiding at narrower widths;
- keep the reduced allocator focused on safe bounded decisions rather than restoring old editor complexity.

The current minimum width remains 820px. Do not lower it until the review/settings surfaces have explicit compact behavior and have been validated at the narrower target.

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