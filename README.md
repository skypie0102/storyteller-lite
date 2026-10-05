# Storyteller Lite

Rust + Slint successor to Storyteller OneClick.

The reconstructed implementation has now been promoted to **`main`**, which is the canonical product branch. The historical `recovery/rust-slint` branch is retained only as reconstruction history. See `docs/HANDOFF.md` for current implementation truth, `docs/ROADMAP.md` for scope/status, and `docs/RECOVERY.md` for provenance.

## Beta testing

The Windows x64 prerelease target is **v0.2.0-beta.1**, prepared from `feature/whisper-live-progress` with the complete PR #32–34 stack. It adds longer Whisper inputs, shared model loads, contextual word reconciliation and live per-part progress to the rebuild. [Release notes and testing steps](docs/releases/v0.2.0-beta.1.md) explain setup and the remaining NVIDIA/full-book acceptance work. The release branch publishes it as a prerelease only after the full Windows package gate passes. The stable v0.1.0 and main are unchanged.

## Current rebuild

PR #30 merged the validated rebuild into `main` at `e04853cb26475a6aa94d8b5c658713473f7e3e7a`. **Whistle** is the lightweight default, with runtime, lifecycle and pipeline execution in `storyteller-application`, independent of Slint. [PR #31](https://github.com/skypie0102/storyteller-lite/pull/31) merged optional Whisper Turbo NVIDIA GPU support and dark/light appearance into `main` at `afb28e5d619a7be2712df51a99797cf02f3dffef`. Both integrations support English only; Whistle uses the published 16.9 MB native CPU model. [The rebuild milestones](docs/REBUILD.md) record validation and remaining acceptance work; [the runtime contract](docs/RUNTIME.md) defines pinned assets.

The native UI has been rebuilt around New book, Queue and Settings. Dark is the default; Settings → Appearance → Theme offers Light and remembers your choice. It previews the output destination, keeps processing and review controls readable at 820×620, and uses native keyboard-accessible buttons. CPU/RAM detection recommends Whistle workers through the default Automatic setting, with manual 1–16 selection. Review evidence and audio preview run on application-owned workers; cached snapshots update only changed rows. See [the UI guide](docs/UI.md) for behavior and reproducible native scenes.

Typed stage outputs, cancellable SHA-256 cache verification and validated publication protect resumed work and existing outputs. R0–R4 passed Windows checkpoints. The optional Whisper follow-up passed strict locked workspace Clippy, all 183 workspace tests, native keyboard/pointer checks, 48 scene renders including 200% scaling, relaunch recovery, 135-second Whistle transcription with six native workers, native Turbo English JSON and rejection of GPU-to-CPU fallback. A separate formatting check passed after a formatting-only correction; [REBUILD.md](docs/REBUILD.md) records both runs. Real NVIDIA offload/performance and representative full English books remain acceptance work. The published v0.1.0 release remains unchanged.

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

Optional Whisper Turbo Q5 uses a separately downloaded, pinned whisper.cpp CUDA runtime on Windows x64 NVIDIA systems. Choose the engine in Settings; jobs save the backend/model and use one GPU worker. Inference must confirm CUDA offload; CPU fallback is rejected. See [RUNTIME.md](docs/RUNTIME.md) for installation and hardware validation limits.

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

The public distribution path is a portable Windows x64 ZIP. The current published release is `v0.1.0`. An explicitly authorized branch named `release/vMAJOR.MINOR.PATCH` triggers the release workflow. Optional `-alpha.N`, `-beta.N` and `-rc.N` suffixes publish GitHub prereleases and explicitly keep the existing Latest release. The branch version must match all three workspace packages. The gate runs formatting, strict locked workspace Clippy/tests, the locked release build, native Dark/Light scene and input checks, package provenance/hash verification and packaged relaunch recovery before publication. Version-specific notes come from `docs/releases/<tag>.md` when present.

The current executable is unsigned and there is no installer or auto-update channel yet.
