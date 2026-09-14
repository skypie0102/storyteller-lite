# Agent handoff — Storyteller OneClick Lite recovery

Read this file before making substantial changes.

## Current repository truth

- Repository: `skypie0102/storyteller-lite`.
- Active Rust + Slint implementation branch: `recovery/rust-slint`.
- Default branch `main` is **not** current implementation truth.
- P1 bounded Whisper chunk workers are implemented and Windows-validated.
- P2 now includes durable per-segment review decisions, reduced manual allocation, Smart/ReviewAll edge behavior, supplemental Introduction/Credits rendering, bounded EPUB image discovery, lazy image text evidence, deterministic image scoring, conservative Smart Graphic Readout assignment, and native image-target Media Overlay rendering.
- The validated Graphic Readout implementation is integrated in recovery as commit `9673dc7a6f41493e13b24724b3123659fcbaa271`.
- Final Graphic Readout Windows validation run `34826426511` passed committed-source rustfmt, strict Clippy, full workspace tests, the native Slint build, and the aggregate gate.
- Temporary Graphic Readout validation PR #3 is closed without merge, and its temporary workflow was removed from `feature/graphic-readout`.
- Hosted CI is intentionally opt-in/manual-only on recovery. **Do not create temporary validation PRs/workflows or dispatch GitHub-hosted runners unless the user explicitly asks.** Prefer local/static validation and report any remaining validation requirement.
- Unvalidated follow-up work currently lives on `feature/manual-graphic-readout`; do not present or integrate it as validated until it has been checked without violating the hosted-runner rule.

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

The reduced allocator supports navigation, audio preview/seek, transcript/timing/evidence context, bounded EPUB text candidates, explicit text assignment, explicit exclusion, and manual Introduction/Credits preservation. Text assignment is revalidated against the nearest accepted alignment neighbors before materialization.

Leading/trailing unmatched segments are marked as Introduction/Credits candidates and may receive bounded FFmpeg silence evidence. Silence is advisory and never by itself authorizes deletion. In Smart mode, anchored edge narration is conservatively preserved as supplemental Introduction/Credits pages; ReviewAll leaves those cases Pending. Smart does not auto-discard unmatched narration.

### Image evidence and Graphic Readout

Bounded image discovery reads the EPUB package/spine directly, including image-only XHTML pages that the text corpus may omit. Work is constrained by neighboring accepted alignment anchors, defaults to at most 12 nearby spine documents and 24 images, accepts only manifest-declared image resources, rejects missing/empty/resources over 25 MiB, and collects embedded `alt`, `title`, SVG `title`/`desc`/text evidence first.

Lazy image text evidence prefers embedded hints. Optional Tesseract is per-call fallback only; there is no permanent OCR setting/runtime requirement. OCR extracts only the selected bounded image, rechecks size, honors cancellation, uses TSV output, and returns normalized evidence only above conservative confidence/text bounds.

`review_image_matches` deterministically ranks candidate evidence against one unmatched transcript. It prefers embedded evidence, uses optional OCR only when needed, enforces phrase/coverage/similarity/distinctive-word bounds, and returns a recommendation only when the winner is strong and sufficiently separated from its runner-up. Weak or ambiguous evidence remains unresolved.

`apply_smart_graphic_readouts` wires that scorer into Smart review for pending **non-edge** items. It assigns only a strong unambiguous bounded image winner, records `GraphicReadout` with `Automatic` provenance, and refuses duplicate image ownership. ReviewAll never applies this automatic image decision.

Graphic Readout rendering is implemented. Build EPUB validates each image destination/classification, finds the real `<img>`/SVG image target, reuses or injects a durable fragment ID, and adds the narration as a native Media Overlay cue. Text and Graphic Readout cues on the same XHTML share one SMIL sequence ordered by audio time. Duplicate image targets and overlapping audio cues are hard build errors. Image-only XHTML is supported. Graphic Readout decisions stay out of effective text alignment materialization and are consumed directly from review data by the EPUB builder.

The end-to-end fixture `crates/storyteller-core/tests/graphic_readout.rs` verifies mixed text/image overlay behavior, durable image anchoring, audio-time ordering, and final structural validation.

Graphic Readout validation run `34826426511` passed:

- `cargo fmt --all -- --check`;
- `cargo clippy --workspace --all-targets -- -D warnings`;
- `cargo test --workspace`;
- `cargo build -p storyteller-ui`;
- aggregate validation gate.

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

## Current unvalidated follow-up: manual Graphic Readout allocation

Branch: `feature/manual-graphic-readout`.

Purpose: let unresolved/ambiguous non-edge narration be manually attached to a **bounded discovered image candidate**, without allowing free-form `document_href` / `image_href` destinations.

Current branch work includes:

- core `assign_manual_graphic_readout(...)`, which reloads the review item, reruns bounded image discovery, rejects edge narration and duplicate image ownership, requires the exact current candidate, then records a Manual Graphic Readout decision;
- unit tests for accepted bounded targets, arbitrary target rejection, edge rejection, and duplicate image ownership;
- allocator work that appends a small number of bounded image rows to the existing candidate model and revalidates the chosen image at click time;
- candidate caching so EPUB image discovery is not repeated by the 100 ms UI refresh loop.

This branch is **not validated or integrated**. Before integration, fix/verify that image-discovery failure is advisory and never hides valid text candidates, make the candidate cache job-specific, perform a formatting/Clippy-oriented static pass, and validate locally or only with explicitly authorized hosted CI.

## Immediate implementation order

1. Finish the manual Graphic Readout allocator on `feature/manual-graphic-readout` under the no-hosted-runner rule; keep image paths bounded/revalidated and image-discovery failure advisory.
2. If validation becomes available without hosted runners, run rustfmt/Clippy/tests/native build and integrate only after green validation. Otherwise leave the branch explicitly unvalidated.
3. Treat Extra Audio as no-op taxonomy unless a real product/output contract emerges; do not add a player-page renderer by default.
4. Move to P3 main Slint UI alignment once the reduced P2 allocator is coherent: compact creation controls, one rich processing card, seven-stage visualization, real metrics, queue/recent management, and responsive reflow.
5. Later: interoperability/EPUBCheck testing, packaging/release/update polish, explicit relaunch/resume UX, and real 1–4 worker CPU/CUDA benchmarks.

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
