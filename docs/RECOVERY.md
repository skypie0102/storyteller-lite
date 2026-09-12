# Recovery ledger

The original Lite repository (`shadowmonarchbooks-cloud/storyteller-oneclick-lite`) became unavailable after the owning GitHub account was suspended. This repository was reconstructed from source text, interfaces, CI results, and commit state preserved during the development conversation.

## Last known original state

- Active original branch: `refactor/rust-slint`
- Original draft PR: #1, `refactor: bootstrap Rust + Slint successor`
- Last fully green core execution checkpoint: `85c30d1b6898a73da40f035e9d17fb0dbae7a056`
- Last known pushed original head: `0f93ccbe731bc29855061e5f9814097d0d2a577b`
- The final original head was not fully revalidated because GitHub access was suspended during its CI cycle.

## Preserved behavior restored here

- Queue-first auto-start model with pause-after-current semantics.
- Exactly one worker owns processing at a time; queue handoff occurs only after the previous worker is joined.
- Exactly one weighted overall progress bar.
- Seven stages: Prepare, Analyze, Align, Review Audio, Encode, Build EPUB, Validate.
- Truthful live activity/metrics only when supplied by the core or processing backend; no fabricated ETA, speed, backend, model, match, or completion.
- Job execution occurs on a worker thread; the Slint event loop polls snapshots every 100 ms.
- SHA-256 content fingerprints for EPUB and audiobook sources, with cancellation checks between streamed reads.
- Resume fingerprints include source identities, settings, backend identities, effective language, and effective Whisper model/runtime identity.
- Prepare always copies the EPUB; audiobook staging prefers a hard link and falls back to a cancellable copy.
- Per-stage artifact manifests are validated before cached stages are restored.
- Cancellation returns an interrupted running stage to Pending.
- Worker snapshots are reconciled through the queue so a terminal transition applies pause-after-current once and duplicate final snapshots are idempotent.
- Worker snapshots cannot mutate immutable job inputs/settings.
- Worker-thread panics terminalize the active job instead of leaving it Running and eligible for accidental restart.
- Failed/cancelled jobs support an explicit safe retry-from-start path that clears progress and checkpoints.
- Waiting jobs can be queued while processing is active, reordered, or removed; recent terminal jobs remain visible in the native shell.

## Current processing boundary

Prepare is implemented and stages real source files into a per-job workspace.

Analyze is implemented using external `ffmpeg` plus the `whisper.cpp` CLI. It converts the staged audiobook to 16 kHz mono PCM, produces JSON-full Whisper output, publishes only Whisper's real progress callbacks, and checkpoints `audio.wav` plus `transcript.json`. Tool/model setup is documented in `docs/RUNTIME.md`.

Align, Review Audio, Encode, Build EPUB, and Validate remain intentionally unimplemented and must fail explicitly until real backends exist.

## Recovery provenance

Some files were available as exact preserved source late in development, notably the Slint dashboard structure, the original Prepare backend, and the worker bridge shape. Core modules were reconstructed to the preserved public interfaces and tested behavioral contracts where the exact final text was not retained. Development after the recovery point is new work in this repository rather than an attempt at byte-identical restoration.

Do not treat commit hashes in this new repository as equivalent to the original repository hashes. They represent the reconstructed lineage.

## Validation status

The last fully validated checkpoint in the new repository is:

`55d2c262e327151a07cbffccdb8d0e398b2807c2`

Its Windows run passed:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. `cargo build -p storyteller-ui`

Later recovery/development commits intentionally have not launched GitHub-hosted Actions. Hosted validation is manual-only under `docs/CI_POLICY.md` to minimize public runner use. Therefore newer heads must not be called fully green until an exact-head validation is deliberately performed.
