# Storyteller Lite

Rust + Slint successor to Storyteller OneClick.

> Recovery status: this repository was reconstructed after the original development repository became unavailable. The recovery branch is `recovery/rust-slint`; see `docs/RECOVERY.md` for provenance and validation status.

## Current development boundary

The recovered application has the queue-first native shell, weighted seven-stage progress model, worker-thread execution, source fingerprint preflight, resumable checkpoint infrastructure, source staging, and a real Prepare backend.

Prepare copies the source EPUB into a per-job workspace and prefers a hard link for the audiobook with a cancellable copy fallback. The source files are never intentionally overwritten. Analyze and later pipeline backends are intentionally not implemented yet and fail explicitly rather than reporting fake progress.

## Build

```powershell
cargo build -p storyteller-ui
```

The Windows CI gate runs formatting, Clippy with warnings denied, workspace tests, and a native Slint build.
