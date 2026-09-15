# Agent handoff — Storyteller OneClick Lite recovery

Read this file before making substantial changes.

## Current repository truth

- Repository: `skypie0102/storyteller-lite`.
- Active Rust + Slint implementation branch: `recovery/rust-slint`.
- Default branch `main` is **not** current implementation truth.
- P1 bounded Whisper chunk workers are implemented and Windows-validated.
- P2 now includes durable per-segment review decisions, reduced manual allocation, Smart/ReviewAll edge behavior, supplemental Introduction/Credits rendering, bounded EPUB image discovery, lazy image text evidence, deterministic image scoring, conservative Smart Graphic Readout assignment, manual bounded Graphic Readout allocation, and native image-target Media Overlay rendering.
- The automatic Graphic Readout implementation was integrated in recovery as commit `9673dc7a6f41493e13b24724b3123659fcbaa271` after Windows validation run `34826426511`.
- Manual Graphic Readout allocation is integrated in recovery as commit `a27df630a8626d1c0ba846deca8f44acde125fe8` after final Windows validation run `34918789594` passed rustfmt, strict Clippy, full workspace tests, the native Slint build, and the aggregate gate.
- Temporary Graphic Readout validation PRs #3 and #4 were closed without merge and their temporary feature workflows were removed after validation.
- Recovery CI remains manual/opt-in to avoid unnecessary hosted-runner use. GitHub runners may be used when they are the right validation tool; before rerunning after a failure, inspect the full logs and likely downstream failure surface, batch fixes, and make the next run a meaningful near-final checkpoint rather than using Actions as an edit/compile loop.

Historical branch names and SHAs in recovered transcripts are clues only. Inspect the live branch before relying on them.

## Product identity and authority order

Storyteller Lite is the Rust + Slint successor to Storyteller OneClick. The old app is a behavioral reference where useful, but Lite is deliberately smaller and must not become a line-for-line port.

Use these references in order:

1. live source on `recovery/rust-slint` for implementation truth;
2. `docs/HANDOFF.md` for current implementation handoff;
3. `docs/ROADMAP.md` for current product/pending-work truth;
4. `docs/ui-guides/README.md` plus mockups for UI hierarchy;
5. `docs/RUNTIME.md` for owned external runtime behavior;
6. `docs/recovery/*.md` for recovered historical evidence and test vectors.

Installer archaeology is demand-driven. Do not recover unrelated legacy behavior unless a live Lite decision is genuinely ambiguous.

## Product decisions that remain authoritative

Keep the queue-first seven-stage flow: Prepare → Analyze → Align → Review Audio → Encode → Build EPUB → Validate. Keep one real overall progress bar, automatic CPU allocation, a simple Whisper worker count of 1–4 (default 1), Smart/ReviewAll unmatched-audio policy, conservative automatic review, bounded/lazy OCR, and a reduced allocator for unresolved audio. Failed books remain failed/retryable but do not stall the queue unless explicitly paused.

Do not restore manual CPU allocation, word-level synchronization, the old activity-console-first UI, Runtime Health, process-now flow, broad runtime path controls, EPUB standardization/CSS editor controls, a permanent OCR toggle, or the old general-purpose split/merge/trim/rules allocator.

## Current pipeline behavior

### Prepare / Analyze / Align

Prepare fingerprints and stages sources. Analyze transcribes deterministic bounded chunks, prefers chapter/silence-aware boundaries, runs at most the selected 1–4 independent Whisper processes, divides available logical CPU threads across workers, and merges chunk-local timestamps into one validated global transcript. Temporary PCM chunks are removed after Analyze. Alignment uses the conservative monotonic block-safe engine and leaves weak evidence unmatched rather than forcing a book position.

P1 Windows validation run: `34732299289`.

### Review Audio

Review decisions are durable per segment:

- `Pending`;
- `Assigned { destination, classification?, source = Automatic | Manual }`;
- `Excluded { reason, source }`.

`review.json` contains the current report and `review-draft.json` preserves decisions outside the disposable Review Audio stage directory. Manual decisions win over automatic Smart recomputation. Review cannot finish while any item remains Pending.

The reduced allocator supports navigation, audio preview/seek, transcript/timing/evidence context, bounded EPUB text candidates, bounded nearby image candidates for non-edge narration, explicit text assignment, manual Graphic Readout assignment, explicit exclusion, and manual Introduction/Credits preservation. Text assignment is revalidated against the nearest accepted alignment neighbors before materialization. Manual image assignment reruns bounded image discovery in core before persisting the destination, rejects edge narration and duplicate image ownership, and never treats a free-form UI path as authority.

Candidate discovery is cached per job + review item in the allocator so the 100 ms UI refresh loop does not repeatedly rescan the EPUB or leak candidates across books. Image discovery remains advisory: failure to discover image candidates does not erase otherwise-valid text choices.

Leading/trailing unmatched segments are marked as Introduction/Credits candidates and may receive bounded FFmpeg silence evidence. Silence is advisory and never by itself authorizes deletion. In Smart mode, anchored edge narration is conservatively preserved as supplemental Introduction/Credits pages; ReviewAll leaves those cases Pending. Smart does not auto-discard unmatched narration.

### Image evidence and Graphic Readout

Bounded image discovery reads the EPUB package/spine directly, including image-only XHTML pages that the text corpus may omit. Work is constrained by neighboring accepted alignment anchors, defaults to at most 12 nearby spine documents and 24 images, accepts only manifest-declared image resources, rejects missing/empty/resources over 25 MiB, and collects embedded `alt`, `title`, SVG `title`/`desc`/text evidence first.

Lazy image text evidence prefers embedded hints. Optional Tesseract is per-call fallback only; there is no permanent OCR setting/runtime requirement. OCR extracts only the selected bounded image, rechecks size, honors cancellation, uses TSV output, and returns normalized evidence only above conservative confidence/text bounds.

`review_image_matches` deterministically ranks candidate evidence against one unmatched transcript. It prefers embedded evidence, uses optional OCR only when needed, enforces phrase/coverage/similarity/distinctive-word bounds, and returns a recommendation only when the winner is strong and sufficiently separated from its runner-up. Weak or ambiguous evidence remains unresolved.

`apply_smart_graphic_readouts` wires that scorer into Smart review for pending **non-edge** items. It assigns only a strong unambiguous bounded image winner, records `GraphicReadout` with `Automatic` provenance, and refuses duplicate image ownership. ReviewAll never applies this automatic image decision.

`assign_manual_graphic_readout` provides the corresponding conservative manual path for unresolved/ambiguous non-edge narration. It reloads the current review item, reruns bounded image discovery, requires the exact current document/image candidate, rejects edge narration and duplicate image ownership, and records `GraphicReadout` with `Manual` provenance. The allocator exposes only discovered candidates rather than arbitrary EPUB paths.

Graphic Readout rendering is implemented. Build EPUB validates each image destination/classification, finds the real `<img>`/SVG image target, reuses or injects a durable fragment ID, and adds the narration as a native Media Overlay cue. Text and Graphic Readout cues on the same XHTML share one SMIL sequence ordered by audio time. Duplicate image targets and overlapping audio cues are hard build errors. Image-only XHTML is supported. Graphic Readout decisions stay out of effective text alignment materialization and are consumed directly from review data by the EPUB builder.

The end-to-end fixture `crates/storyteller-core/tests/graphic_readout.rs` verifies mixed text/image overlay behavior, durable image anchoring, audio-time ordering, final structural validation, and manual bounded-image rediscovery → Manual provenance persistence → Build EPUB → independent Validate.

Automatic Graphic Readout validation run `34826426511` passed:

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- `cargo test --workspace`;
- `cargo build -p storyteller-ui`;
- aggregate validation gate.

Manual Graphic Readout validation used the same gate set. Run `34918305701` exposed one test-only variable-shadowing compile error in the new regression; the complete log showed Clippy/tests were blocked by that same error while the native Slint build was already green. After fixing that root cause, final run `34918789594` passed all four substantive gates and the aggregate gate.

### Introduction / Credits supplemental rendering

Introduction/Credits preservation generates real XHTML + SMIL pages before/after the chosen spine anchor, with manifest/spine/media-overlay relationships, real audiobook clip timing, and Media Overlay duration metadata. Manual and conservative Smart preservation share this path.

Relevant Windows validation runs:

- supplemental rendering `34738027167`;
- Smart edge preservation `34745516558`;
- supplemental/Smart end-to-end regression `34746062156`.

### Encode / Build EPUB / Validate

Encode creates the final Copy/Opus/AAC representation. Build EPUB consumes the effective reviewed text alignment plus supplemental and Graphic Readout review destinations. Validate independently reopens the candidate EPUB and audits package/SMIL/text-or-image/audio relationships, positive non-overlapping clip ranges, durations, duplicates, and mimetype rules before publication.

The builder semantic fingerprint was bumped when Graphic Readout output behavior landed so stale build checkpoints are not silently reused.

## Extra Audio decision

`AudioReviewClassification::ExtraAudio` exists, but Lite currently has **no distinct Extra Audio destination/rendering rule**. Recovered legacy evidence documents an optional standalone audio-player page as one historical fallback, but explicitly treats that as a product/interoperability choice rather than a required Lite behavior. Do not invent a separate Extra Audio renderer merely for historical taxonomy compatibility. Add one only if a concrete product requirement and validation strategy justify it.

## Immediate implementation order

1. Treat the reduced P2 allocator as functionally coherent for text, Introduction/Credits, and automatic/manual Graphic Readout paths. Keep weak/ambiguous evidence Pending until the user chooses a bounded destination or exclusion.
2. Do not add a distinct Extra Audio renderer unless a real product/output contract emerges.
3. Move to P3 main Slint UI alignment: compact creation controls, one rich processing card, seven-stage visualization, real metrics, queue/recent management, reduced allocator presentation polish, and responsive reflow. The allocator section heading still says `EPUB TEXT CANDIDATES` even though Graphic Readout rows may appear; correct that wording when touching the P3 UI without expanding the allocator into the old editor.
4. Later: interoperability/EPUBCheck testing, packaging/release/update polish, explicit relaunch/resume UX, and real 1–4 worker CPU/CUDA benchmarks.

## Recovered invariants worth preserving

- Human/manual decisions are explicit and durable.
- Destinations are bounded and validated; the UI never supplies arbitrary EPUB paths as authority.
- One audio interval must not be ambiguously owned twice.
- Weak/ambiguous evidence remains Pending rather than being forced.
- Final output is independently audited after review decisions are applied.
- Historical OCR/scoring thresholds are test vectors, not permanent product constants.
- The recovered GPL/Sigil-derived helper is behavioral evidence unless licensing for direct reuse is deliberately resolved.

## Engineering migration rule

Preserve old behavior/evidence until replacement behavior is implemented and regression-covered.

> **Replace → regression-test → delete. Never delete → hope we remembered everything.**