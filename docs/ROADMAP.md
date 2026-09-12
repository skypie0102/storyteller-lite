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

## Pipeline

The user-facing stage order is fixed:

1. **Prepare** — validate and fingerprint inputs, create the per-job workspace, copy the source EPUB, and hard-link/copy the audiobook.
2. **Analyze** — normalize audio for speech recognition and generate a timestamped Whisper transcript.
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
- Cancellable external command runner with streamed stdout/stderr callbacks.

### Prepare

Implemented. EPUB is copied. Audiobook staging prefers a hard link and falls back to cancellable copying. Prepared sources are validated before later stages reopen them.

### Analyze

Implemented using external `ffmpeg` and `whisper.cpp` CLI tooling. The stage converts to 16 kHz mono PCM, invokes Whisper with JSON-full output, publishes only Whisper's real progress callbacks, and checkpoints `audio.wav` and `transcript.json`.

### Align and later stages

Not implemented yet. They must continue to fail explicitly until their real backends exist.

## Next development sequence

1. Harden and validate the recovered Analyze integration on Windows without turning hosted CI back into an every-push workload.
2. Add runtime/tool/model discovery to the Settings experience rather than relying only on environment variables and conventional folders.
3. Implement EPUB reading-order extraction for the Align input corpus.
4. Implement transcript-to-text alignment with real confidence/match reporting and durable alignment artifacts.
5. Define Review Audio artifacts and UI for ambiguous/unmatched regions.
6. Implement final audio encoding with Copy / Opus / AAC behavior.
7. Build Media Overlay / read-aloud EPUB output and preserve source metadata/navigation.
8. Add final EPUB validation/audit and publication behavior.
9. Finish queue management: reorder/remove waiting jobs, retry failed/cancelled jobs, resume from validated checkpoints, and recent/completed job presentation.
10. Finish settings, diagnostics, packaging, release/update behavior, and migration polish.

## Failure policy still to finalize

The historical v1 behavior for whether a failed book should automatically advance to the next waiting book could not be recovered after the original repositories became unavailable. The current bridge advances after a terminal worker unless the queue is paused. Revisit this policy when a trustworthy v1 reference or an explicit product decision is available.
