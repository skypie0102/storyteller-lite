# Storyteller Lite

Rust + Slint successor to Storyteller OneClick.

> Recovery status: this repository was reconstructed after the original development repository became unavailable. The recovery branch is `recovery/rust-slint`; see `docs/RECOVERY.md` for provenance and `docs/ROADMAP.md` for the restored product roadmap.

## Current development boundary

The recovered application has the queue-first native shell, weighted seven-stage progress model, worker-thread execution, source fingerprint preflight, resumable checkpoint infrastructure, source staging, active cancellation, pause/resume queue controls, and automatic handoff to the next queued book after the previous worker is joined.

Prepare is fully implemented. It copies the source EPUB into a per-job workspace and prefers a hard link for the audiobook with a cancellable copy fallback. The source files are never intentionally overwritten.

Analyze now has a real offline backend based on `ffmpeg` and the `whisper.cpp` CLI. It converts the staged audiobook to 16 kHz mono PCM, runs Whisper with JSON-full output, forwards Whisper's real progress callbacks, and checkpoints the generated WAV and transcript JSON. See `docs/RUNTIME.md` for tool/model discovery and setup.

Align and later pipeline backends are intentionally not implemented yet and fail explicitly rather than reporting fake progress.

## Build

```powershell
cargo build -p storyteller-ui
```

## Validation policy

GitHub-hosted CI is intentionally manual-only to avoid wasting public runner capacity. Routine pushes and pull requests do not start hosted jobs. Use the workflow's `core` validation for an occasional lightweight core checkpoint and `full-windows` only for meaningful native integration/release checkpoints. See `docs/CI_POLICY.md`.
