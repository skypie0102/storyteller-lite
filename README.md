# Storyteller Lite

Rust + Slint successor to Storyteller OneClick.

> Recovery status: this repository was reconstructed after the original development repository became unavailable. The recovery branch is `recovery/rust-slint`; see `docs/RECOVERY.md` for provenance and `docs/ROADMAP.md` for the restored product roadmap.

## Current development boundary

The recovered application has the queue-first native shell, weighted seven-stage progress model, worker-thread execution, source fingerprint preflight, resumable checkpoint infrastructure, source staging, active cancellation, pause/resume queue controls, automatic handoff after worker join, queue/recent management, and a real human-review pause state.

All seven pipeline stages now have concrete development implementations:

- **Prepare** — source validation/fingerprinting and safe workspace staging.
- **Analyze** — bounded EPUB reading-order extraction, ffmpeg PCM conversion, and `whisper.cpp` JSON-full transcription with real backend progress.
- **Align** — conservative monotonic transcript-to-EPUB text alignment with explicit unmatched segments, block-safe matches, and real match metrics.
- **Review Audio** — durable unmatched-audio report plus an explicit native decision to cancel or continue while excluding unmatched regions.
- **Encode** — cancellable Copy mode for compatible EPUB core audio, or ffmpeg Opus/AAC encoding with real processed-audio timestamps and a durable media descriptor.
- **Build EPUB** — deterministic XHTML block anchors, EPUB 3 Media Overlay SMIL, encoded audio embedding, package manifest associations, and Media Overlay duration metadata. The source EPUB is not overwritten.
- **Validate** — independently reopens the candidate EPUB, checks container/package/SMIL/text/audio relationships and durations, writes `validation.json`, then publishes only after the runner has accepted the validation artifact. Existing output files are never overwritten.

The current Media Overlay implementation targets EPUB 3 source packages and deliberately rejects source books that already contain Media Overlays rather than attempting an unsafe merge. Synchronization is currently block-level: accepted Whisper segments retain their real audio timing while SMIL text references target deterministic XHTML block fragments.

See `docs/RUNTIME.md` for ffmpeg/Whisper discovery and model setup.

## Build

```powershell
cargo build -p storyteller-ui
```

## Validation policy

GitHub-hosted CI is intentionally manual-only to avoid wasting public runner capacity. Routine pushes and pull requests do not start hosted jobs. Use the workflow's `core` validation for an occasional lightweight core checkpoint and `full-windows` only for meaningful native integration/release checkpoints. See `docs/CI_POLICY.md`.

The last hosted fully-green checkpoint is still `55d2c262e327151a07cbffccdb8d0e398b2807c2`. The complete seven-stage development head is newer and must not be treated as fully validated until an intentional exact-head checkpoint is run.
