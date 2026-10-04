# Storyteller Lite

Rust + Slint successor to Storyteller OneClick.

The reconstructed implementation has now been promoted to **`main`**, which is the canonical product branch. The historical `recovery/rust-slint` branch is retained only as reconstruction history. See `docs/HANDOFF.md` for current implementation truth, `docs/ROADMAP.md` for scope/status, and `docs/RECOVERY.md` for provenance.

## Whistle rebuild

The active branch, `rebuild/reliability-foundation`, replaces Whisper with **Whistle** and moves runtime, lifecycle and pipeline execution into `storyteller-application`, independent of Slint. Transcription supports English only, with the published 16.9 MB native CPU model. [The rebuild milestones](docs/REBUILD.md) record validation and remaining acceptance work; [the runtime contract](docs/RUNTIME.md) defines pinned assets.

The native UI has been rebuilt around New book, Queue and Settings. It previews the output destination, keeps processing and review controls readable at 820×620, and uses native keyboard-accessible buttons. CPU/RAM detection recommends Whistle workers through the default Automatic setting, with manual 1–16 selection. Review evidence and audio preview run on application-owned workers; cached snapshots update only changed rows. See [the UI guide](docs/UI.md) for behavior and reproducible native scenes.

Typed stage outputs, cancellable SHA-256 cache verification and validated publication protect resumed work and existing outputs. R0–R4 passed Windows checkpoints; the latest passed all 171 workspace tests, native keyboard/pointer checks, 39 scene renders including 200% scaling, relaunch recovery, and 135-second Whistle transcription with six native workers. Automatic detection and saved 1–16 worker settings are covered. Representative full English books, recognition/timing assessment and release packaging remain R5 acceptance work. This branch has not replaced the published v0.1.0 baseline.

## Released baseline

The native application is functionally implemented through the full queue-first seven-stage pipeline:

- **Prepare** — source validation/fingerprinting and safe workspace staging.
- **Analyze** — deterministic bounded audio chunks, 1–4 bounded transcription workers, real transcription progress, and cleanup of temporary PCM.
- **Align** — conservative monotonic transcript-to-EPUB alignment that leaves weak evidence unmatched.
- **Review Audio** — durable per-segment decisions, Smart/ReviewAll behavior, Introduction/Credits preservation, bounded text/image candidates, and manual/automatic Graphic Readout handling.
- **Encode** — cancellable Copy mode where safe, or FFmpeg Opus/AAC encoding.
- **Build EPUB** — EPUB 3 Media Overlays for text, supplemental Introduction/Credits, and Graphic Readout image targets.
- **Validate** — independent structural validation before publication.

The Slint UI is functionally complete for the supported 820×620 minimum window. Relaunch recovery, checkpoint validation, per-user runtime storage, pinned/verified runtime and FFmpeg acquisition, and the manual Windows developer-test package are also implemented.

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
