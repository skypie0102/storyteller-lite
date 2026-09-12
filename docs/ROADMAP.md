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
- An unimplemented or unsupported boundary must fail explicitly. It must never be represented as completed or skipped just to make the pipeline look finished.
- Human review decisions must be explicit and durable. Continuing past unmatched audio records that those regions were intentionally excluded from synchronization.
- Publication occurs only after the candidate EPUB passes an independent structural audit and the runner has accepted the Validate stage artifacts.

## Pipeline

The user-facing stage order is fixed:

1. **Prepare** — validate and fingerprint inputs, create the per-job workspace, copy the source EPUB, and hard-link/copy the audiobook.
2. **Analyze** — extract EPUB reading-order text, normalize audio for speech recognition, and generate a timestamped Whisper transcript.
3. **Align** — align transcript/audio timing against the EPUB reading text and derive real match/confidence information.
4. **Review Audio** — surface unresolved or ambiguous alignment regions that genuinely require user review.
5. **Encode** — create the final audio representation according to Copy / Opus / AAC and bitrate settings.
6. **Build EPUB** — construct the synchronized read-aloud EPUB without replacing the source.
7. **Validate** — independently audit the completed candidate and only then publish/complete the job.

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
- Resume fingerprints for Whisper, alignment, audio encoder, and EPUB builder backend identity.
- Stage finalization hook that runs after artifact capture and before the stage can complete; final EPUB publication uses this path.

### Prepare

Implemented. EPUB is copied. Audiobook staging prefers a hard link and falls back to cancellable copying. Prepared sources are validated before later stages reopen them.

### Analyze

Implemented using native EPUB parsing plus external `ffmpeg` and `whisper.cpp` CLI tooling. The stage extracts a bounded/cancellable reading-order corpus, converts the audiobook to 16 kHz mono PCM, invokes Whisper with JSON-full output, publishes only Whisper's real progress callbacks, and checkpoints `book-corpus.json`, `audio.wav`, and `transcript.json`. Whisper segment timing must be positive, chronological, and non-overlapping.

### Align

Implemented with the conservative `monotonic-ngram-edit-v2-block-safe` engine. Whisper segment timestamps remain the timing authority. Strong n-gram anchors and token edit similarity are used to accept monotonic book-text matches; weak evidence stays explicitly unmatched. Accepted matches cannot cross normalized XHTML block boundaries. The stage checkpoints `alignment.json` and publishes actual processed-segment and match metrics.

### Review Audio

Implemented. `review.json` contains the real unmatched transcript segments and audio ranges. If there are no unmatched segments, processing continues automatically. Otherwise the job enters `NeedsReview`; the native UI previews unmatched regions and offers Cancel or **Continue without unmatched audio**. Continuing records the exclusion decision in the review artifact before the pipeline resumes.

### Encode

Implemented as whole-audiobook output. Copy mode performs a cancellable byte-preserving copy only when the source extension maps safely to an EPUB 3.3 core Media Overlay audio type. Opus and AAC use external `ffmpeg`; live processed-audio time is sourced from ffmpeg's machine-readable progress output. Generated Opus is declared `audio/ogg; codecs=opus`; AAC is `audio/mp4`. `encoded-audio.json` records the output filename, codec, bitrate, and EPUB media type alongside the encoded audio artifact.

### Build EPUB

Implemented for EPUB 3 source packages that do not already contain Media Overlays. The builder:

- preserves the source EPUB and writes a workspace candidate;
- copies unrelated ZIP resources through without unpacking the publication tree;
- writes `mimetype` first and uncompressed;
- maps accepted alignment lines to real XHTML block elements;
- reuses existing block IDs or injects deterministic collision-resistant `stl-mo-*` anchors;
- creates SMIL `<par>` entries with real Whisper segment clip times;
- embeds the encoded audiobook under a collision-free package-relative resource tree;
- adds `media-overlay` manifest associations, SMIL/audio manifest items, per-overlay `media:duration`, and total `media:duration` metadata;
- explicitly rejects EPUB 2 and already-synchronized source publications rather than guessing at a migration/merge.

Current synchronization granularity is block-level: multiple real transcript segments may legitimately reference the same XHTML block fragment.

### Validate

Implemented as an independent structural audit of the built candidate. It reopens the ZIP and verifies the mimetype ordering/compression/value, duplicate entries, package/Media Overlay links, SMIL media types, text-fragment existence, audio manifest targets, positive clip ranges, and refined/total Media Overlay duration consistency. It writes `validation.json`. Publication to `<title> (readaloud).epub` occurs in the runner finalization phase only after the validation artifact manifest has been captured. Existing output files are not overwritten.

An end-to-end core fixture builds a minimal EPUB with two synchronized blocks, validates it, publishes it once, and verifies that a second publication attempt is rejected.

## Next development sequence

1. Perform an intentional exact-head validation checkpoint now that all seven development backends and an end-to-end EPUB fixture exist. Keep hosted validation manual and sparse.
2. Fix any formatting/Clippy/test/native-build issues exposed by that checkpoint; do not broaden scope while establishing the new baseline.
3. Add runtime/tool/model discovery and configuration to the Settings experience instead of relying only on environment variables and conventional folders.
4. Expand EPUB compatibility: existing Media Overlay migration/merge policy, EPUB 2 conversion policy if desired, remote resources, SVG/text alternatives, and more semantic/skippable structures.
5. Improve review UX with audio preview/seeking and finer-grained decisions instead of only global exclusion acceptance.
6. Add explicit validated checkpoint-resume controls to the UI after persistence/relaunch behavior is defined.
7. Finish diagnostics, packaging, release/update behavior, installer/tool bundling policy, and migration polish.
8. Test interoperability against multiple EPUB reading systems and, when available in the release workflow, EPUBCheck in addition to the internal validator.

## Failure policy still to finalize

The historical v1 behavior for whether a failed book should automatically advance to the next waiting book could not be recovered after the original repositories became unavailable. The current bridge advances after a terminal worker unless the queue is paused. Revisit this policy when a trustworthy v1 reference or an explicit product decision is available.
