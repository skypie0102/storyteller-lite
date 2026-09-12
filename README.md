# Storyteller Lite

Rust + Slint successor to Storyteller OneClick.

> Recovery status: this repository was reconstructed after the original development repository became unavailable. The recovery branch is `recovery/rust-slint`; see `docs/RECOVERY.md` for provenance and `docs/ROADMAP.md` for the restored product roadmap.

## Current development boundary

The recovered application has the queue-first native shell, weighted seven-stage progress model, worker-thread execution, source fingerprint preflight, resumable checkpoint infrastructure, source staging, active cancellation, pause/resume queue controls, automatic handoff after worker join, queue/recent management, and a real human-review pause state.

The pipeline currently has real implementations for:

- **Prepare** — source validation/fingerprinting and safe workspace staging.
- **Analyze** — bounded EPUB reading-order extraction, ffmpeg PCM conversion, and `whisper.cpp` JSON-full transcription with real backend progress.
- **Align** — conservative monotonic transcript-to-EPUB text alignment with explicit unmatched segments and real match metrics.
- **Review Audio** — durable unmatched-audio report plus an explicit native decision to cancel or continue while excluding unmatched regions.
- **Encode** — cancellable Copy mode or ffmpeg Opus/AAC encoding, with a durable codec/media-type descriptor and real processed-audio timestamps when ffmpeg reports them.

**Build EPUB** and **Validate** remain intentionally unimplemented. The next builder milestone is deterministic XHTML fragment anchoring followed by standards-conforming EPUB Media Overlay/SMIL packaging. The app will continue to fail explicitly at an unimplemented boundary rather than report fake completion.

See `docs/RUNTIME.md` for ffmpeg/Whisper discovery and model setup.

## Build

```powershell
cargo build -p storyteller-ui
```

## Validation policy

GitHub-hosted CI is intentionally manual-only to avoid wasting public runner capacity. Routine pushes and pull requests do not start hosted jobs. Use the workflow's `core` validation for an occasional lightweight core checkpoint and `full-windows` only for meaningful native integration/release checkpoints. See `docs/CI_POLICY.md`.

The last hosted fully-green checkpoint predates the newest Align/Review/Encode work; newer commits must not be treated as validated until an intentional checkpoint is run locally or through the manual workflow.
