# Storyteller Lite

Rust + Slint successor to Storyteller OneClick.

The reconstructed implementation has now been promoted to **`main`**, which is the canonical product branch. The historical `recovery/rust-slint` branch is retained only as reconstruction history. See `docs/HANDOFF.md` for current implementation truth, `docs/ROADMAP.md` for scope/status, and `docs/RECOVERY.md` for provenance.

## Current status

The native application is functionally implemented through the full queue-first seven-stage pipeline:

- **Prepare** — source validation/fingerprinting and safe workspace staging.
- **Analyze** — deterministic bounded audio chunks, 1–4 bounded Whisper workers, real transcription progress, and cleanup of temporary PCM.
- **Align** — conservative monotonic transcript-to-EPUB alignment that leaves weak evidence unmatched.
- **Review Audio** — durable per-segment decisions, Smart/ReviewAll behavior, Introduction/Credits preservation, bounded text/image candidates, and manual/automatic Graphic Readout handling.
- **Encode** — cancellable Copy mode where safe, or FFmpeg Opus/AAC encoding.
- **Build EPUB** — EPUB 3 Media Overlays for text, supplemental Introduction/Credits, and Graphic Readout image targets.
- **Validate** — independent structural validation before publication.

The Slint UI is functionally complete for the supported 820×620 minimum window. Relaunch recovery, checkpoint validation, per-user runtime storage, pinned/verified whisper.cpp and FFmpeg acquisition, and the manual Windows developer-test package are also implemented.

P5 release/interoperability hardening remains active. EPUBCheck 5.4.0 passes the three representative exported fixtures. The same three artifacts have also been manually tested and reported to work normally.

**v0.1.0 is released.** The public portable Windows x64 release was built from commit `f208abbd9ee27eb19cb3ed2f802a8ecda17c38c8`; release run `35302413893` passed version identity, rustfmt, strict workspace Clippy, all workspace tests, the locked release build, package/hash verification, packaged relaunch recovery, archive verification, and GitHub Release publication. The executable is unsigned; there is no installer or auto-update channel.

## Build

```powershell
cargo build -p storyteller-ui
```

## Validation

Routine pushes and pull requests intentionally do not consume hosted runners. Validation workflows are manual/opt-in.

Useful checkpoints include:

- P1 bounded Whisper workers: Windows run `34732299289`.
- Manual Graphic Readout allocation: Windows run `34918789594`.
- EPUBCheck interoperability baseline: Linux run `35065877026`.
- Shared app-data/runtime root: Windows run `35076642104`.
- Packaged NeedsReview relaunch rewind: Windows run `35179721389`.
- Thorium import/open interoperability smoke: Linux run `35220207088`.

See `docs/CI_POLICY.md`, `docs/HANDOFF.md`, and `docs/RUNTIME.md` for the current validation and runtime contracts.


## Release process

The public distribution path is a portable Windows x64 ZIP. The current published release is `v0.1.0`. A branch named `release/vMAJOR.MINOR.PATCH` triggers the release workflow, which requires the branch version to match both Cargo packages, reruns formatting/Clippy/tests, builds with `--locked --release`, verifies package provenance and hashes, runs the packaged relaunch-recovery smoke, and publishes a GitHub Release only after those checks pass.

The current executable is unsigned and there is no installer or auto-update channel yet.
