# Storyteller Lite roadmap

Storyteller Lite is the Rust + Slint successor to Storyteller OneClick. The original application remains the behavioral reference where its behavior is known, but Lite is not intended to be a line-for-line port.

## Product contracts

- Queue-first workflow. The first waiting book starts automatically; later books wait their turn.
- Exactly one pipeline worker may own processing at a time.
- `Pause after book` takes effect on the current job's terminal transition and prevents the next waiting job from starting until the queue is resumed.
- Long-running work never runs on the Slint UI thread.
- Exactly one weighted overall progress bar is shown.
- Progress, elapsed audio, ETA, speed, backend, model, and match values are displayed only when backed by real measurements or backend callbacks.
- The normal Details view is structured. Raw stdout/stderr belongs to diagnostics, not the primary progress UI.
- Source files are never intentionally overwritten. Output is written alongside the source EPUB by default.
- Retry/resume may reuse only a validated contiguous prefix of stage checkpoints. A stale stage invalidates itself and all downstream stages.
- An unimplemented backend must fail explicitly. It must never be represented as completed or skipped just to make the pipeline look finished.
- Human review decisions must be explicit and durable. Continuing past unmatched audio records that those regions were intentionally excluded from synchronization.

## Pipeline

The user-facing stage order is fixed:

1. **Prepare** — validate and fingerprint inputs, create the per-job workspace, copy the source EPUB, and hard-link/copy the audiobook.
2. **Analyze** — extract EPUB reading-order text, normalize audio for speech recognition, and generate a timestamped Whisper transcript.
3. **Align** — align transcript/audio timing against the EPUB reading text and derive real match/confidence information.
4. **Review Audio** — surface unresolved or ambiguous alignment regions that genuinely require user review.
5. **Encode** — create the final audio representation according to Copy / Opus / AAC and bitrate settings.
6. **Build EPUB** — construct the synchronized read-aloud EPUB without replacing the source.
7. **Validate** — audit the completed EPUB and only then mark the job complete.

## Current implementation status

### Complete foundations

- Rust workspace with UI-independent job, queue, progress, resume, scheduler, runner, and worker abstractions.
- Seven-stage weighted progress model.
- SHA-256 source fingerprint preflight with cancellation checks.
- Per-job and per-stage workspaces plus artifact manifests.
- Worker snapshot reconciliation through the queue.
- 100 ms Slint worker bridge.
- Queue auto-advance after the previous worker has been joined.
- Pause-after-current, queue resume, and active worker cancellation controls.
- Queue-another-book flow while a job is active.
- Waiting/recent job management with reorder, remove, safe retry-from-start, and visible terminal errors.
- Cancellable external command runner with streamed stdout/stderr callbacks.
- Resume fingerprints for Whisper, alignment, and audio encoder backend identity.

### Prepare

Implemented. EPUB is copied. Audiobook staging prefers a hard link and falls back to cancellable copying. Prepared sources are validated before later stages reopen them.

### Analyze

Implemented using native EPUB parsing plus external `ffmpeg` and `whisper.cpp` CLI tooling. The stage extracts a bounded/cancellable reading-order corpus, converts the audiobook to 16 kHz mono PCM, invokes Whisper with JSON-full output, publishes only Whisper's real progress callbacks, and checkpoints `book-corpus.json`, `audio.wav`, and `transcript.json`.

### Align

Implemented with the conservative `monotonic-ngram-edit-v1` engine. Whisper segment timestamps remain the timing authority. Strong n-gram anchors and token edit similarity are used to accept monotonic book-text matches; weak evidence stays explicitly unmatched. The stage checkpoints `alignment.json` and publishes actual processed-segment and match metrics.

### Review Audio

Implemented. `review.json` contains the real unmatched transcript segments and audio ranges. If there are no unmatched segments, processing continues automatically. Otherwise the job enters `NeedsReview`; the native UI previews unmatched regions and offers Cancel or **Continue without unmatched audio**. Continuing records the exclusion decision in the review artifact before the pipeline resumes.

### Encode

Implemented as whole-audiobook output. Copy mode performs a cancellable byte-preserving copy. Opus and AAC use external `ffmpeg`; live processed-audio time is sourced from ffmpeg's machine-readable progress output. `encoded-audio.json` records the output filename, codec, bitrate, and EPUB media type alongside the encoded audio artifact.

### Build EPUB and Validate

Not implemented yet. They must continue to fail explicitly until their real backends exist.

Build EPUB must not invent synchronization anchors. The current alignment map has accepted character ranges in normalized XHTML reading text. Before generating Media Overlays, the builder must create deterministic fragment anchors in the source XHTML for accepted matches, then create SMIL that points at those real anchors and the encoded audio clips. Unmatched regions accepted during Review Audio remain excluded.

## Next development sequence

1. Statically harden the new EPUB parser/alignment/review/encode code and perform an intentional validation checkpoint when runner use is justified.
2. Implement deterministic XHTML anchoring for accepted alignment ranges without disturbing source reading order, metadata, navigation, or unrelated markup.
3. Generate EPUB 3 Media Overlay SMIL and package the encoded audio plus modified XHTML into a new output EPUB next to the source.
4. Add final EPUB validation/audit and publication behavior.
5. Add runtime/tool/model discovery to the Settings experience rather than relying only on environment variables and conventional folders.
6. Finish diagnostics, packaging, release/update behavior, and migration polish.
7. Add validated checkpoint-resume controls to the UI after the current retry-from-start path is proven stable.

## Failure policy still to finalize

The historical v1 behavior for whether a failed book should automatically advance to the next waiting book could not be recovered after the original repositories became unavailable. The current bridge advances after a terminal worker unless the queue is paused. Revisit this policy when a trustworthy v1 reference or an explicit product decision is available.
