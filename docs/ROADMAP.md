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

Installer archaeology is now demand-driven. Inspect more only when a live Lite behavior is genuinely ambiguous.

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
- Conservative automatic edge handling.
- Lazy/on-demand OCR only for bounded candidate EPUB images when needed.
- Reduced manual audio allocation for unresolved segments.
- Limited useful dispositions such as Introduction, Credits, Graphic Readout, and Extra Audio where they change destination/build behavior.
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

Validated maintenance source commit: `d7d421135078f76ef54bda0adc821f9c60afc898`.

Current caveat: multiple GPU-backed Whisper processes may each load the model. Keep the default at 1 until benchmarking and, if needed, hardware/VRAM-aware clamping justify a different default.

### Align — implemented

Uses the conservative `monotonic-ngram-edit-v2-block-safe` engine. Whisper timestamps remain timing authority. Strong n-gram anchors/token similarity accept monotonic book matches; weak evidence remains unmatched. Accepted matches cannot cross normalized XHTML block boundaries. `alignment.json` records output and real match metrics are surfaced.

### Review Audio — partial; next major milestone

`review.json` contains real unmatched transcript/audio ranges. With no unmatched segments processing continues automatically; otherwise the job enters `NeedsReview`.

Current limitation: the UI only previews unmatched regions and offers Cancel or global **Continue without unmatched audio**. Continuing stores a global exclusion flag. Smart edge handling, ReviewAll, lazy OCR/classification, durable per-segment decisions, and manual assignment are not implemented yet.

### Encode — implemented

Whole-audiobook output. Copy mode performs cancellable byte-preserving copy only when the source maps safely to an EPUB Media Overlay audio type. Opus/AAC use ffmpeg machine-readable progress. `encoded-audio.json` records filename, codec, bitrate, and media type.

### Build EPUB — implemented for current allocation model

For EPUB 3 sources without existing Media Overlays, the builder preserves unrelated resources, creates valid SMIL/audio manifest links, injects deterministic block anchors where required, writes real Whisper clip times, and embeds encoded audio. Current synchronization is block-level.

P2 will require extending this builder for supplemental Introduction/Credits XHTML+SMIL pages and validated image-bound Graphic Readout narration when review decisions use those dispositions.

### Validate — implemented

Independently reopens/audits the candidate ZIP, package/SMIL relationships, text fragments, audio targets, positive clip ranges, duration consistency, duplicates, mimetype rules, and related structural invariants. `validation.json` is captured before publication. Existing output files are not overwritten.

Recovered legacy compatibility note: old OneClick repaired only zero-length SMIL clips by setting `clipEnd = clipBegin + 0.001s`, then still subjected output to a final audit. See `docs/recovery/INTEGRITY_RECOVERY.md`; do not generalize that into a broad timing fixer.

## Immediate pending work

### P0 — baseline validation — complete

A known recovered Windows build existed before P1, and the P1 feature itself now passes strict Windows lint, workspace tests, and native Slint build.

### P1 — Whisper workers + chunked Analyze — complete

The current implementation follows recovered semantics: workers are higher-level bounded chunk concurrency, while each whisper.cpp invocation retains independent thread/process settings. Manual CPU allocation remains out of scope.

### P2 — Smart unmatched-audio pipeline and reduced manual allocator — next

Implement the Review Audio behavior recovered in `docs/recovery/UNMATCHED_AUDIO_RECOVERY.md` and `docs/recovery/ALLOCATOR_OUTPUT_RECOVERY.md`.

Suggested implementation sequence:

1. Define a durable reduced decision model with at least Pending / Assigned / Excluded, plus optional disposition/suggestion/provenance.
2. Add a reduced **Smart / ReviewAll** policy without restoring the old four-mode selector.
3. Add native silence/edge evidence and conservative automatic edge handling.
4. Generate bounded EPUB candidates from reading order and neighboring accepted alignment positions.
5. Use embedded image text hints first and lazy OCR only when Smart/current review segment needs it.
6. Support high-confidence Graphic Readout → real image/page assignment where evidence validates it.
7. Preserve Introduction/Credits/Extra Audio dispositions only where they materially change destination/rendering behavior.
8. Persist review decisions/drafts separately from rebuildable preview workspaces; retry/relaunch should restore review work.
9. Constrain manual assignments by monotonic EPUB ordering and real XHTML/image candidates.
10. Materialize an effective downstream allocation/alignment result while retaining original automatic alignment plus decision provenance.
11. Build the reduced Slint allocator from the supplied mockup: audio preview/seek, transcript/timing/silence context, Smart suggestion, candidate context, previous/next unresolved, assign/exclude/override, Apply & Next.
12. Extend Build EPUB for supplemental Introduction/Credits pages and validated image-bound Graphic Readout narration.
13. Require every unresolved region to have a validated disposition before continuing and independently audit final output.
14. Add regression tests for ordering, exclusions, Smart/manual mixes, OCR/image candidates, restart durability, and invalid/cross-boundary decisions.

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
