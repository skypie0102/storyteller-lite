# Storyteller Lite roadmap

> **Canonical recovery roadmap.** Read `docs/HANDOFF.md` before substantial work. The live Rust + Slint source on `recovery/rust-slint` is technical truth; this document is current product/pending-work truth.

Storyteller Lite is the Rust + Slint successor to Storyteller OneClick. The original app is a behavioral reference where useful, but Lite is intentionally smaller and is not a line-for-line port.

## Recovery references

The recovery packet is stored in-repo so another agent can continue without this chat:

- `docs/HANDOFF.md` — first-read implementation handoff.
- `docs/RUNTIME.md` — current owned runtime behavior.
- `docs/ui-guides/storyteller-lite-main-ui.webp` — main-screen hierarchy reference.
- `docs/ui-guides/storyteller-lite-manual-allocation.webp` — reduced allocator layout reference.
- `docs/ui-guides/README.md` — mockup interpretation rules.
- `docs/recovery/README.md` — authority order/provenance index.
- `docs/recovery/WORKER_SEMANTICS.md` — recovered meaning of Parallel Whisper jobs.
- `docs/recovery/UNMATCHED_AUDIO_RECOVERY.md` — Smart/edge/lazy-OCR recovery.
- `docs/recovery/ALLOCATOR_OUTPUT_RECOVERY.md` — downstream rendering semantics for review decisions.
- `docs/recovery/QUEUE_FAILURE_RECOVERY.md` — recovered queue continuation behavior.
- `docs/recovery/INTEGRITY_RECOVERY.md` — finishing/integrity behavior including zero-length SMIL repair.
- `docs/recovery/FRONTEND_AND_CONCURRENCY_RECOVERY.md` — recovered old frontend/state/persistence behavior.
- `docs/recovery/INSTALLER_DISSECTION.md` and `docs/recovery/LITE_PLANNING_HISTORY.txt` — historical evidence.

Installer archaeology is demand-driven. Inspect more only when a live Lite behavior is genuinely ambiguous.

## Product contracts

- Queue-first workflow. The first waiting book starts automatically; later books wait their turn.
- Exactly one foreground pipeline worker owns book processing at a time; Analyze may internally run bounded transcription chunks concurrently.
- A failed book remains Failed/retryable and does not stall the queue unless the queue was explicitly paused or `Pause after book` was requested.
- Long-running work never runs on the Slint UI thread.
- Exactly one weighted overall progress bar is shown.
- Progress, timing, speed, backend, model, and match values are shown only when backed by real backend/structured measurements.
- Raw process logs belong to diagnostics, not the primary progress UI.
- Source files are never intentionally overwritten; output defaults next to the source EPUB.
- Resume may reuse only a validated contiguous prefix of stage checkpoints; stale stages invalidate themselves and downstream stages.
- Unsupported boundaries fail explicitly rather than being marked complete/skipped.
- Human review decisions are explicit and durable.
- Smart decisions are conservative, auditable, and reversible before publication.
- Publication occurs only after independent structural validation and runner acceptance of Validate artifacts.

## Current Lite scope

### Keep / restore

- Fixed seven-stage Lite pipeline.
- Queue-first processing and pause-after-current.
- Structured real progress/metrics.
- Native Rust + Slint architecture.
- Automatic CPU-thread selection for Whisper.
- One simple Whisper worker-count setting: default `1`, current range `1–4`.
- Worker count means **concurrent transcription chunks**, not manual CPU allocation and not whisper.cpp `-p`.
- **Smart / ReviewAll** unmatched-audio policy.
- Conservative automatic edge handling only when evidence is high-confidence and destination/rendering is safe.
- Lazy/on-demand OCR only for bounded candidate EPUB images when needed.
- Reduced manual audio allocation for unresolved segments.
- Limited useful dispositions such as Introduction, Credits, Graphic Readout, and Extra Audio where they materially change destination/build behavior.
- Supplied UI mockups as hierarchy references.

### Explicitly not restored by default

- Manual CPU/thread allocation UI.
- Word-level synchronization.
- Activity-console-first UI.
- Runtime Health page.
- Process-now flow.
- Full runtime-path tuning UI.
- Standardize-EPUB toggle.
- CSS editor.
- Permanent OCR controls.
- Full historical allocator/editor surface: arbitrary split/merge, general waveform trimming, broad rules, unrestricted classification/destination editing.

## Pipeline

The user-facing stage order is fixed:

1. **Prepare** — validate/fingerprint inputs, create workspace, stage EPUB/audio.
2. **Analyze** — extract reading-order text and produce one validated global Whisper transcript from bounded audio chunks.
3. **Align** — align transcript timing against EPUB reading text.
4. **Review Audio** — Smart-handle safe unmatched regions and surface unresolved/ambiguous audio.
5. **Encode** — create final Copy / Opus / AAC audio representation.
6. **Build EPUB** — construct synchronized read-aloud EPUB without replacing source.
7. **Validate** — independently audit candidate, then publish.

## Current implementation status

### Complete foundations

- Rust workspace with UI-independent job, queue, progress, resume, scheduler, runner, and worker abstractions.
- Seven-stage weighted progress model.
- SHA-256 source preflight with cancellation checks.
- Per-job/stage workspaces with artifact manifests.
- Worker snapshot reconciliation and 100 ms Slint bridge.
- Queue auto-advance after terminal workers, including failures, unless pause-after-current paused the queue.
- Pause-after-current, resume, active cancellation, queue-another-book, reorder/remove/retry, visible terminal errors.
- Cancellable external process runner with streamed stdout/stderr callbacks.
- Backend-aware stage fingerprints and finalization hook.

### Prepare — implemented

EPUB is copied. Audiobook staging prefers a hard link and falls back to cancellable copying. Prepared sources are validated before later stages reopen them.

### Analyze — implemented and Windows-validated

P1 replaced the previous single full-book PCM path.

Current behavior:

- Settings exposes `1–4` Whisper workers, default `1`; no manual CPU allocation UI.
- The selected value is snapshotted per queued job and is execution-only for semantic resume fingerprints.
- ffmpeg provides duration/chapter metadata; no ffprobe dependency is required.
- Long audio is split into deterministic ordered ranges. Chapter boundaries are preferred and synthetic cuts are refined around nearby silence.
- Only the active ranges are decoded to temporary 16 kHz mono signed-16-bit PCM.
- At most the selected number of independent whisper.cpp CLI processes run concurrently.
- Available logical CPU threads are divided across active workers and passed through `-t`; worker count is not mapped to `-p`.
- Chunk-local Whisper timestamps are converted to global audiobook time, merged, and validated for chronology/range correctness.
- Silent individual chunks are allowed while an invalid/empty final transcript is rejected.
- Temporary PCM is removed after use; there is no permanent whole-book `audio.wav`.
- Durable Analyze artifacts are `book-corpus.json`, `transcription-plan.json`, and normalized `transcript.json`.
- Internal worker failure cancels siblings without being confused with user cancellation.
- Analyze fingerprints include ffmpeg, whisper.cpp, language, and model identity.
- Audio codec/bitrate changes invalidate Encode/downstream, not Analyze.

Windows validation passed on GitHub Actions run `34732299289`:

- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo build -p storyteller-ui`

Current caveat: multiple GPU-backed Whisper processes may each load the model. Keep the default at 1 until benchmarking and, if needed, hardware/VRAM-aware clamping justify a different default.

### Align — implemented

Uses the conservative `monotonic-ngram-edit-v2-block-safe` engine. Whisper timestamps remain timing authority. Strong n-gram anchors/token similarity accept monotonic book matches; weak evidence remains unmatched. Accepted matches cannot cross normalized XHTML block boundaries. `alignment.json` records output and real match metrics are surfaced.

### Review Audio — substantial P2 foundation implemented

Review Audio is no longer a global-exclusion-only gate.

Implemented now:

- `review.json` stores stable per-segment IDs, transcript/timing, optional Smart suggestion, leading/trailing edge identity, optional silence evidence, and an explicit decision.
- Durable decisions are `Pending`, `Assigned`, or `Excluded`, with `Automatic` / `Manual` provenance. A small classification set exists for Introduction, Credits, Graphic Readout, and Extra Audio.
- Review decisions are also persisted in `review-draft.json` outside the disposable stage directory and are restored when the report is rebuilt.
- The UI supports previous/next navigation, audio preview/stop, ±5 s seeking, transcript/timing/context, monotonic EPUB text candidates, explicit Assign, Exclude, and completion only after every region has a decision.
- Text candidates are bounded by the nearest accepted matches in EPUB reading order and ranked by deterministic lexical overlap.
- Manual text assignment is revalidated against the monotonic window before it is materialized into the effective alignment.
- Leading/trailing unmatched segments are marked as Introduction/Credits candidates and receive bounded FFmpeg silence evidence; silence is advisory and never by itself authorizes deletion.
- `Smart` and `ReviewAll` now behaviorally diverge for safe edge preservation. `Smart` automatically assigns leading/trailing unmatched narration to supplemental Introduction/Credits pages when a concrete matched EPUB anchor exists; `ReviewAll` leaves those same segments Pending for user review.
- Smart edge handling is deliberately non-destructive: it does **not** automatically exclude unmatched audio.
- Saved manual decisions always override Smart. Saved automatic decisions are recomputed under the active policy, so switching/rebuilding under ReviewAll cannot silently retain a prior Smart assignment.
- Edge segments can still be manually preserved/overridden in the allocator.

Still pending in Review Audio:

- Lazy image candidate extraction/OCR and Graphic Readout classification/assignment are not implemented yet.
- Extra Audio has a classification value but no dedicated destination/rendering behavior yet.
- Dedicated end-to-end supplemental/Smart regression fixtures are still needed beyond the core policy tests and workspace validation gates.

The Smart edge-preservation slice passed Windows validation on GitHub Actions run `34745516558`:

- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo build -p storyteller-ui`

Validated source commit: `b868eb0cae9c5916ae1048c18cd2c66bf8f6ee8c`.

### Encode — implemented

Whole-audiobook output. Copy mode performs cancellable byte-preserving copy only when the source maps safely to an EPUB Media Overlay audio type. Opus/AAC use ffmpeg machine-readable progress. `encoded-audio.json` records filename, codec, bitrate, and media type.

### Build EPUB — implemented for text allocation plus supplemental edge pages

For EPUB 3 sources without existing Media Overlays, the builder preserves unrelated resources, creates valid SMIL/audio manifest links, injects deterministic block anchors where required, writes real Whisper clip times, and embeds encoded audio. Normal synchronization is block-level.

P2 additions now implemented:

- Reviewed text assignments are materialized into an effective alignment while the original automatic alignment remains the source record.
- A reviewed or Smart-assigned Introduction can become generated XHTML + SMIL immediately before its existing spine anchor.
- A reviewed or Smart-assigned Credits segment can become generated XHTML + SMIL immediately after its existing spine anchor.
- Supplemental pages receive their own manifest items, spine references, Media Overlay association, clip timing from the real audiobook interval, and per-overlay duration metadata.
- Build EPUB fingerprinting was bumped for the supplemental-page behavior so an older Build EPUB checkpoint is not silently reused.

Still pending:

- validated image-bound Graphic Readout rendering;
- any deliberately chosen Extra Audio rendering semantics;
- dedicated regression fixtures that exercise supplemental spine ordering/manifest/duration behavior end-to-end, beyond the workspace compile/test gates already passed.

The supplemental edge-page slice passed Windows validation on GitHub Actions run `34738027167`:

- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo build -p storyteller-ui`

Validated source commit: `cea590ada02c7942a8fda7226631f22d503e55b1`.

### Validate — implemented

Independently reopens/audits the candidate ZIP, package/SMIL relationships, text fragments, audio targets, positive clip ranges, duration consistency, duplicates, mimetype rules, and related structural invariants. `validation.json` is captured before publication. Existing output files are not overwritten.

Recovered legacy compatibility note: old OneClick repaired only zero-length SMIL clips by setting `clipEnd = clipBegin + 0.001s`, then still subjected output to a final audit. See `docs/recovery/INTEGRITY_RECOVERY.md`; do not generalize that into a broad timing fixer.

## Immediate pending work

### P0 — baseline validation — complete

Known recovered Windows validation exists and all integrated P1/P2 slices so far were gated by strict Clippy, workspace tests, and native Slint build.

### P1 — Whisper workers + chunked Analyze — complete

The current implementation follows recovered semantics: workers are higher-level bounded chunk concurrency, while each whisper.cpp invocation retains independent thread/process settings. Manual CPU allocation remains out of scope.

### P2 — finish Smart unmatched-audio pipeline and reduced allocator — active

Already landed:

1. Durable per-segment Pending / Assigned / Excluded decisions with provenance and restart-safe draft persistence.
2. Reduced `Smart / ReviewAll` setting wiring.
3. Native leading/trailing edge identity plus bounded silence evidence.
4. Monotonic bounded EPUB text candidates and manual text assignment/exclusion.
5. Reduced allocator controls for navigation, preview/seek, transcript/timing/context, assign/exclude, and completion gating.
6. Effective reviewed alignment materialization for text assignments.
7. Manual Introduction/Credits preservation through generated supplemental XHTML+SMIL pages.
8. Conservative Smart edge preservation: safe anchored leading/trailing narration is automatically assigned to supplemental Introduction/Credits pages, while ReviewAll keeps those regions Pending and Smart never auto-discards audio.

Next implementation sequence:

1. Add dedicated regression tests for supplemental page manifest/spine order, SMIL clips, duration metadata, restart durability, and Smart/manual mixes.
2. Add bounded EPUB image candidate discovery: nearby reading-order documents first, embedded `alt` / `title` / SVG text hints before OCR.
3. Add lazy OCR only for bounded image candidates needed by Smart or the current unresolved segment; do not restore a permanent OCR setting.
4. Implement high-confidence Graphic Readout classification and validated image/page assignment without allowing overlap/double allocation.
5. Extend Build EPUB for validated image-bound Graphic Readout narration.
6. Decide whether Extra Audio needs a distinct Lite destination/rendering rule; if not, do not grow the taxonomy merely for historical compatibility.
7. Keep every weak/ambiguous region Pending for manual review and independently audit final output.

Historical thresholds in `docs/recovery/UNMATCHED_AUDIO_RECOVERY.md` are recovery test vectors, not mandatory tuning constants. Implement/test the Rust classifier rather than copying the old GPL helper.

Do not initially add arbitrary split/merge, a general waveform trim editor, broad Apply-to-similar rules, permanent OCR controls, or the old unrestricted allocator taxonomy.

### P3 — main Slint UI alignment

Bring the main experience closer to `docs/ui-guides/storyteller-lite-main-ui.webp`: compact creation controls, one rich processing card, seven-stage visualization, real metrics, queue/recent management, and responsive reflow. Do not treat the mockup as a fixed pixel canvas.

### P4 — installer archaeology only when needed

If behavior remains ambiguous, statically inspect the supplied v0.39.0 installer after verifying its documented SHA-256 and record recovered behavior before implementation. Do not broaden Lite scope just because a legacy feature exists.

### P5 — later polish/compatibility

After P2/P3 stabilize:

- explicit checkpoint-resume UX after persistence/relaunch behavior is defined;
- diagnostics and packaging/release/update polish;
- additional EPUB compatibility policy where deliberately chosen;
- interoperability testing across reading systems and EPUBCheck when available;
- benchmark 1–4 transcription workers across CPU/CUDA systems before changing defaults or adding hardware-aware clamping.

## Queue failure policy — recovered and resolved

The old executable explicitly reported queue continuation after a failed book when pending books remained; `Stop After` was the separate opt-out. Current Rust + Slint behavior matches this.

Keep:

> **Failed book → preserve Failed/retry state → continue to next waiting book.** Only explicit queue pause / Pause-after-current prevents the next book from starting.

## Engineering migration rule

Preserve old material until replacement behavior is implemented and regression-covered.

> **Replace → regression-test → delete. Never delete → hope we remembered everything.**
