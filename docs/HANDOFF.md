# Agent handoff — Storyteller OneClick Lite recovery

Read this file before making substantial changes.

## Current repository truth

- Repository: `skypie0102/storyteller-lite`.
- Active Rust + Slint implementation branch: `recovery/rust-slint`.
- Default branch `main` is **not** current implementation truth.
- P1 bounded Whisper chunk workers are implemented and Windows-validated.
- P2 includes durable per-segment review decisions, reduced manual allocation, Smart/ReviewAll edge behavior, supplemental Introduction/Credits rendering, bounded EPUB image discovery, lazy image text evidence, deterministic image scoring, conservative Smart Graphic Readout assignment, manual bounded Graphic Readout allocation, and native image-target Media Overlay rendering.
- The automatic Graphic Readout implementation was integrated as `9673dc7a6f41493e13b24724b3123659fcbaa271` after Windows validation run `34826426511`.
- Manual Graphic Readout allocation was integrated as `a27df630a8626d1c0ba846deca8f44acde125fe8` after final Windows validation run `34918789594` passed rustfmt, strict Clippy, full workspace tests, the native Slint build, and the aggregate gate.
- P3 main Slint alignment is functionally complete for the supported 820×620 minimum window. The validated sequence includes mixed review presentation (`1a645685549a0796b960d18723bb0cd42799c79d`, run `34919543203`), real stage elapsed time (`c79c834328edabc4fef72d0d0cd5fb8343499411`, run `34926648718`), responsive creation (`c8a6461eaeb4d2535af26172876b729cf0d2abf5`, run `34927154367`), responsive active/review/queue controls (`17ef8d7948a04d74fd1f0ea3a60a0e8da140e8b8`, run `34927928639`), dedicated Review Audio presentation (`1602021a9c52eadb66fa7bc73937e56dacfd4c57`, run `34952813936`), locally scrollable queue/recent (`26d11580e328da36d7a6169be22ec91e2ef3a157`, run `34953863269`), locally scrollable Settings (`e98adae2dc0fff77923fd62d02768c9b01609e8f`, run `35058804437`), and final compact review/settings polish (`fe5b140b2319229b4083c6aa4d1c7d386a604bbd`, run `35059503255`).
- The first P5 interoperability slice is integrated as `ef7900ceb5efd3541d8b33f056d3f3ff8d9920e8`. It adds standards-clean representative EPUB fixtures, a manual EPUBCheck 5.4.0 gate, required Media Overlay `epub:textref` output, and matching internal-validator enforcement. Final interoperability run `35065877026` passed rustfmt, strict core Clippy, core tests, fixture export, pinned EPUBCheck checksum verification, and EPUBCheck on text-overlay, supplemental Introduction/Credits, and Graphic Readout outputs.
- Validated resume preflight is integrated as `19ecde9b5082393a9ccb056a3abe189bfa89393a`. Worker startup now revalidates both semantic checkpoint fingerprints and stage artifact manifests before reusing cached stages; the first invalid stage truncates downstream checkpoints. Core run `35067743420` passed rustfmt, strict core Clippy, and core tests.
- Durable paused relaunch recovery is integrated as `8c0c3d35dff44e0d616077d627a3a4e1e5c478fe`. Recoverable jobs persist to a versioned queue snapshot, interrupted Running work restores as Waiting, restored work never auto-runs, terminal jobs are omitted, and a job that was waiting at Review Audio is rewound before Review Audio so unresolved human review cannot be bypassed across relaunch. Final Windows run `35068910791` passed rustfmt, strict workspace Clippy, all workspace tests, and the native Slint build.
- Temporary validation PRs #3 through #15 were closed without merge and their temporary feature workflows/scaffolding were removed after validation.
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

Resume must reuse only a validated contiguous checkpoint prefix. Relaunch recovery must never silently continue work: recovered jobs come back in a paused queue and require the existing explicit Resume queue action.

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

All generated SMIL sequences now include the EPUB Media Overlays `epub:textref` relationship back to their associated XHTML document. The internal validator requires that attribute on every generated `seq`, resolves it relative to the SMIL file, and rejects a target that does not match the package-associated XHTML. This was added after EPUBCheck exposed the missing relationship in the first standards-clean Graphic Readout fixture.

The builder semantic fingerprint was bumped when Graphic Readout output behavior landed so stale build checkpoints are not silently reused.

### EPUBCheck interoperability baseline

External conformance checking is development-only and supplements—rather than replaces—the internal publication validator. `.github/workflows/epubcheck-validation.yml` is manual-only and currently pins EPUBCheck 5.4.0. It downloads the official `epubcheck-5.4.0.zip`, verifies SHA-256 `33350c61038e71dfb3d45a76aed04bf5481e6d5500cb780f6e98db8bbd15a28c`, and runs EPUBCheck under Temurin Java 21. Java and EPUBCheck are **not** runtime dependencies of Storyteller Lite.

`crates/storyteller-core/examples/export_epubcheck_fixtures.rs` builds three deliberately standards-clean test books through the real Storyteller builder and internal validator:

- ordinary text Media Overlay;
- supplemental Introduction/Credits pages around a normal chapter;
- mixed text + Graphic Readout Media Overlay.

The exporter uses a valid navigation document, required package metadata, a valid tiny PNG, and a real tiny MP3 so external failures describe Storyteller output rather than deliberately fake test resources. Existing internal fixtures may still use synthetic bytes when their purpose is purely structural; do not indiscriminately point EPUBCheck at every unit-test EPUB.

Validation history for this slice:

- run `35064940210` exposed both rustfmt-only exporter wrapping and the real missing `seq epub:textref` conformance defect;
- run `35065707149` failed immediately in temporary patch-application scaffolding and did not exercise product code;
- final run `35065877026` passed formatting, strict core Clippy, all core tests, fixture export/internal validation, official EPUBCheck checksum verification, EPUBCheck on all three fixtures, and the aggregate gate.

### Validated resume and paused relaunch recovery

Worker startup now applies the existing resume rules instead of trusting in-memory Completed stages. It first invalidates stale semantic fingerprints, obtains the contiguous fingerprint resume plan, validates that plan against each stage's artifact manifest in the job workspace, truncates downstream checkpoints to the validated reusable prefix, and marks only that verified prefix Cached. A retry/relaunch therefore cannot reuse a missing, modified, or semantically stale artifact simply because an old `Job` snapshot still says that stage completed.

The durable relaunch layer persists only recoverable queue state: job identity, immutable source/output/settings fields, the previous recoverable status, and completed checkpoint fingerprints. It does **not** persist transient progress percentages, elapsed live metrics, terminal history, or a live worker object. The versioned snapshot is `queue-recovery.json` under the existing app-data root (`%LOCALAPPDATA%\Storyteller OneClick Lite` on Windows; existing temp-directory fallback elsewhere).

Restore is intentionally conservative:

- Running → Waiting;
- Waiting → Waiting;
- NeedsReview → Waiting with checkpoints at/after Review Audio removed;
- Completed / Failed / Cancelled are not persisted;
- any recovered work forces the queue to Paused;
- the user must choose the existing `Resume queue` action before processing restarts.

Rewinding NeedsReview is deliberate. `review-draft.json` already preserves durable manual decisions outside the disposable Review Audio stage, so rerunning Review Audio can recover those decisions without treating an unresolved human-review boundary as implicitly accepted after an application restart.

Recovery parsing rejects unsupported snapshot versions, duplicate job IDs, invalid codec/bitrate/stage values, blank checkpoint fingerprints, non-strict stage ordering, and non-contiguous checkpoint prefixes. The UI bridge loads once and saves at a throttled one-second cadence rather than every 100 ms poll.

Validation history:

- resume preflight integration `19ecde9b5082393a9ccb056a3abe189bfa89393a`, core run `35067743420` green;
- relaunch recovery integration `8c0c3d35dff44e0d616077d627a3a4e1e5c478fe`, final Windows run `35068910791` green across formatting, strict workspace Clippy, all workspace tests, and native Slint build.

## Extra Audio decision

`AudioReviewClassification::ExtraAudio` exists, but Lite currently has **no distinct Extra Audio destination/rendering rule**. Recovered legacy evidence documents an optional standalone audio-player page as one historical fallback, but explicitly treats that as a product/interoperability choice rather than a required Lite behavior. Do not invent a separate Extra Audio renderer merely for historical taxonomy compatibility. Add one only if a concrete product requirement and validation strategy justify it.

## P3 UI state — functionally complete

The main Slint UI now follows the intended Lite hierarchy for the supported 820×620 minimum window without recreating the historical editor:

- creation source pickers/options and active queue-another controls reflow below 900px;
- review is visually promoted to a dedicated `REVIEW AUDIO` mode, suppressing unrelated queue-another and processing noise while decisions are required;
- review content is locally scrollable at short window heights, and queue/recent is temporarily hidden during review so unresolved-audio decisions remain usable at the 620px minimum height;
- mixed bounded text/image destinations and assign/exclude-and-advance behavior are explicit;
- the seven-stage strip displays real per-stage elapsed time when available;
- queue/recent uses `ListView`, so unbounded history/waiting rows scroll locally and only visible delegates are instantiated;
- Settings keeps its header/back and bottom actions fixed while the runtime body scrolls, and its worker/runtime/action rows reflow below 900px;
- long top-level status text is bounded/elided rather than competing with the application title;
- only real backend-provided timing/backend/model/match/activity data is shown; no synthetic metrics were added.

P3 UI validation runs:

- review presentation: `34919543203`;
- stage elapsed timing: `34926648718`;
- responsive creation layout: `34927154367`;
- responsive active/review/queue layout: `34927928639`;
- dedicated review presentation: `34952813936`;
- queue/recent local scrolling: `34953863269`;
- Settings local scrolling: `35058804437`;
- compact review/settings/header polish: `35059503255`.

These UI-only slices were validated with the relevant native gate, `cargo build -p storyteller-ui`, rather than repeatedly spending full workspace test runs when no Rust/backend behavior changed. Keep the 820px minimum width unless real usage justifies and validates a narrower contract; lowering it is not required to complete P3.

## Immediate implementation order

1. Treat P2 and P3 as functionally complete unless real books or real window use expose a narrowly scoped regression.
2. Keep the internal validator + manual EPUBCheck baseline intact; new output/rendering paths should extend representative external fixtures when appropriate rather than weakening either gate.
3. Preserve the validated-resume and explicit paused-relaunch contracts; exercise them through installed/packaged builds before changing lifecycle behavior.
4. Continue P5 with reading-system interoperability evidence plus packaging/release/update and owned-runtime polish. Do not turn EPUBCheck/Java into a shipped dependency.
5. Do not add a distinct Extra Audio renderer unless a real product/output contract emerges, and skip installer archaeology unless a live Lite behavior is genuinely ambiguous.
6. Keep real 1–4 worker CPU/CUDA benchmarking as later evidence work before changing worker defaults or adding hardware-aware heuristics.

## Recovered invariants worth preserving

- Human/manual decisions are explicit and durable.
- Destinations are bounded and validated; the UI never supplies arbitrary EPUB paths as authority.
- One audio interval must not be ambiguously owned twice.
- Weak/ambiguous evidence remains Pending rather than being forced.
- Resume trusts only the contiguous checkpoint prefix whose semantic fingerprints and stage artifacts still validate.
- Relaunch never silently resumes processing; recovered work returns paused and requires explicit user action.
- Final output is independently audited after review decisions are applied.
- External EPUBCheck is an interoperability development gate, not a substitute for deterministic internal validation before publication.
- Historical OCR/scoring thresholds are test vectors, not permanent product constants.
- The recovered GPL/Sigil-derived helper is behavioral evidence unless licensing for direct reuse is deliberately resolved.

## Engineering migration rule

Preserve old behavior/evidence until replacement behavior is implemented and regression-covered.

> **Replace → regression-test → delete. Never delete → hope we remembered everything.**