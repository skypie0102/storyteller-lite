# Agent handoff — Storyteller OneClick Lite recovery

Read this file before making substantial changes.

## Current repository truth

- Repository: `skypie0102/storyteller-lite`
- Active recovered code branch: `recovery/rust-slint`
- Default branch `main` is not the Rust + Slint implementation branch.
- P1 was reconstructed on `feature/whisper-chunk-workers` and validated on Windows before integration.

Historical branch names and SHAs in recovered transcripts are clues only. Inspect the live branch before relying on them.

## Product identity

Storyteller Lite is the Rust + Slint successor to Storyteller OneClick. The pre-Rust application is a behavioral reference where useful, but Lite is deliberately smaller and must not become a line-for-line port.

Primary references:

1. `docs/ROADMAP.md` — canonical current roadmap/product scope.
2. `docs/ui-guides/README.md` + mockups — current visual/product references.
3. `docs/RUNTIME.md` — owned external runtime behavior.
4. `docs/recovery/README.md` — provenance and authority order.
5. `docs/recovery/WORKER_SEMANTICS.md` — recovered transcription concurrency semantics.
6. `docs/recovery/UNMATCHED_AUDIO_RECOVERY.md` — Smart/edge/lazy-OCR recovery.
7. `docs/recovery/ALLOCATOR_OUTPUT_RECOVERY.md` — downstream Introduction/Credits/Graphic Readout semantics.
8. `docs/recovery/QUEUE_FAILURE_RECOVERY.md` — queue auto-advance behavior after failure.
9. `docs/recovery/INTEGRITY_RECOVERY.md` — legacy finishing/integrity behavior, including the 1 ms zero-length SMIL repair.
10. `docs/recovery/FRONTEND_AND_CONCURRENCY_RECOVERY.md` — old frontend state, persistence, and allocator invariants.
11. `docs/recovery/INSTALLER_DISSECTION.md` — static findings from v0.39.0.
12. `tools/recovery/extract_legacy_nsis.py` and `tools/recovery/extract_tauri_assets.py` — reproducible recovery tools.

Installer archaeology is now **demand-driven**. Do not spend time recovering unrelated EXE details unless a live Lite behavior remains ambiguous.

## User decisions that remain authoritative

### Keep / restore

- Queue-first workflow.
- Fixed seven-stage pipeline: Prepare → Analyze → Align → Review Audio → Encode → Build EPUB → Validate.
- Exactly one overall progress bar backed by real metrics.
- Automatic CPU-thread selection for Whisper.
- A simple user-facing **Whisper worker count**, default `1`, range `1–4` for the current Lite implementation.
- Worker count means **concurrent bounded transcription chunks**, not manual CPU allocation and not whisper.cpp `-p`.
- **Smart / ReviewAll** unmatched-audio policy.
- Conservative automatic edge handling for safe high-confidence cases.
- Lazy/on-demand OCR of bounded EPUB image candidates; no permanent OCR toggle.
- Reduced manual allocation for unresolved audio.
- Limited useful dispositions such as Introduction, Credits, Graphic Readout, and Extra Audio where they affect build behavior.
- Failed books remain failed/retryable but do not stall the queue unless the queue was explicitly paused or Pause-after-current was requested.
- Supplied mockups as UI hierarchy guides.

### Do not restore by default

- Manual CPU/thread allocation UI.
- Full old OneClick settings complexity.
- Word-level synchronization.
- Activity-console-first UI.
- Runtime Health page.
- Process-now flow.
- Engine/runtime path tuning UI.
- Standardize-EPUB toggle.
- CSS editor.
- Permanent OCR toggle.
- Arbitrary split/merge/trim/rules/general-purpose allocator complexity.

## Current code behavior

### P0/P1 — complete and Windows-validated

P1 replaced the previous single full-book PCM transcription path.

Current Analyze behavior:

- `JobSettings` carries `whisper_workers`, validated to `1..=4`, default `1`.
- The Settings UI exposes only that simple worker selector. There is no manual CPU allocation control.
- The selected worker value is snapshotted into each queued job.
- Worker count is execution-only for resume fingerprints; changing it does not invalidate semantic checkpoints.
- Analyze probes audiobook duration/chapter metadata using the existing ffmpeg executable; no ffprobe dependency was added.
- Long audio is divided into deterministic ordered chunks. Chapter boundaries are preferred; synthetic boundaries are refined around nearby detected silence.
- Each chunk is temporarily decoded to 16 kHz mono signed-16-bit PCM and passed to its own whisper.cpp CLI process.
- At most `whisper_workers` chunks are transcribed concurrently.
- Each whisper invocation uses `-t` with an automatically divided CPU-thread budget. The worker setting is **not** mapped to `-p`.
- Local Whisper timestamps are offset back to global audiobook time, merged, sorted/validated, and written as one normalized transcript.
- Silent individual chunks are allowed; an invalid/empty final transcript is not.
- Temporary PCM chunk files are deleted when Analyze finishes. There is no permanent whole-book `audio.wav` artifact.
- Durable Analyze artifacts are `book-corpus.json`, `transcription-plan.json`, and `transcript.json`.
- User cancellation propagates to active chunk processes. Internal worker failure cancels siblings through a private worker token without misclassifying the whole job as user-cancelled.
- Analyze fingerprinting includes both ffmpeg and whisper.cpp executable identity plus language/model identity.
- Audio codec/bitrate changes invalidate Encode/downstream rather than unnecessarily invalidating Analyze.

Windows validation for the feature passed:

- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo build -p storyteller-ui`

Validation run: GitHub Actions run `34732299289`. The validated source cleanup commit is `d7d421135078f76ef54bda0adc821f9c60afc898`.

### Known P1 caveats

- Multiple GPU-backed whisper processes can each load the model. The current UI deliberately limits concurrency to 1–4, but there is not yet a GPU/VRAM-aware automatic clamp. Benchmark before changing the default above 1.
- Progress is aggregated from real Whisper callback percentages across chunks; it is not a wall-clock guess. A future refinement may duration-weight chunks if useful.
- Changing worker count is intentionally excluded from semantic cache fingerprints. If future evidence shows materially different transcript semantics at different concurrency, revisit that policy with regression data rather than assumptions.

### Review Audio — current next gap

Review Audio still writes `review.json`, previews unmatched regions, and offers only Cancel or global **Continue without unmatched audio**. `AudioReviewReport` still uses a global `accepted_unmatched_exclusion` decision; there is no durable per-segment assignment model yet.

Smart edge handling, ReviewAll, lazy OCR/image classification, durable manual drafts, and reduced per-segment allocation remain unimplemented. Alignment itself remains conservative/monotonic, and accepted transcript matches stay tied to real XHTML block ranges.

Queue failure behavior already matches recovered OneClick: terminal worker results advance to the next waiting job unless pause-after-current has paused the queue.

## Installer recovery facts worth preserving

- Old OneClick had separate `threads`, `parallelTranscribes`, and `parallelTranscodes` settings.
- Historical validation allowed 1–32 CPU threads, 1–4 parallel transcription jobs, and 1–8 parallel FFmpeg jobs; historical backend defaults were different from Lite and are not compatibility requirements.
- Old alignment launched Whisper with `--processors 1` separately from `--parallel-transcribes`, proving parallel transcribes was higher-level work concurrency rather than whisper.cpp processor count.
- Old manual allocation persisted `manualDrafts`; the recovered frontend autosaved after roughly 450 ms and kept a local fallback.
- Durable review decisions were separate from disposable/rebuildable preview workspaces. Preserve that boundary in Lite.
- Old finishing logic required complete non-overlapping coverage and explicit targets, followed by a final independent audit.
- Old zero-length SMIL repair was narrowly `clipEnd = clipBegin + 0.001s`; the final audit could still reject invalid/overlapping output.
- Historical OCR was already bounded/lazy: narrow candidates first, embedded text hints where possible, OCR only when needed.
- Introduction/Credits could become supplemental XHTML+SMIL pages; Graphic Readout narration could attach to an existing image page when validated.
- The recovered GPL/Sigil-derived helper is a behavioral/test-vector reference unless licensing for direct reuse is deliberately resolved.

## Immediate implementation order

### P2 — Smart unmatched-audio pipeline and reduced manual allocator

This is the next product milestone. Replace the current all-or-nothing unmatched-audio review with the recovered Lite model from `docs/recovery/UNMATCHED_AUDIO_RECOVERY.md` and `docs/recovery/ALLOCATOR_OUTPUT_RECOVERY.md`.

Required behavior:

- reduced **Smart / ReviewAll** policy;
- conservative automatic edge handling;
- bounded EPUB candidate generation using reading order and neighboring accepted matches;
- lazy embedded-image text/OCR only when Smart or the current unresolved segment needs it;
- small optional disposition set such as Introduction, Credits, Graphic Readout, Extra Audio where destination/rendering semantics require it;
- unresolved segments remain Pending rather than being silently discarded;
- durable per-segment Pending / Assigned / Excluded decisions with provenance and optional disposition metadata;
- autosaved drafts that survive app restart/retry independently of disposable preview workspaces;
- previous/next unresolved navigation, audio preview/seek, transcript/timing/silence context, Smart suggestion, EPUB candidate context, explicit assignment/exclusion/override, and Apply & Next;
- monotonic EPUB ordering and real XHTML block/image candidate validation;
- a materialized effective downstream allocation/alignment result while retaining original automatic alignment and review provenance;
- Build EPUB support for supplemental Introduction/Credits pages and validated image-bound Graphic Readout narration when those dispositions are used;
- final structural audit after automatic/manual decisions.

Do **not** initially add arbitrary split/merge, a general waveform trim editor, broad Apply-to-similar rules, permanent OCR controls, or the unrestricted old allocator taxonomy.

### P3 — align Slint UI with supplied mockups

Use the main mockup for hierarchy/information density and the allocator mockup for the dedicated review experience. Treat them as responsive wide-window guides, not fixed pixel canvases.

## Engineering rule

Do not start with broad legacy deletion. Use:

> Replace → regression-test → delete.

When old behavior and current Lite scope conflict, prefer current Lite scope and document the decision.
