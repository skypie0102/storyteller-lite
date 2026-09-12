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
- Exactly one weighted overall progress bar.
- Seven stages: Prepare, Analyze, Align, Review Audio, Encode, Build EPUB, Validate.
- Truthful live activity/metrics only when supplied by the core; no fabricated ETA, speed, backend, model, match, or completion.
- Job execution occurs on a worker thread; the Slint event loop polls snapshots every 100 ms.
- SHA-256 content fingerprints for EPUB and audiobook sources, with cancellation checks between streamed reads.
- Resume fingerprints include source identities, settings, backend identities, effective language, and effective Whisper model.
- Prepare always copies the EPUB; audiobook staging prefers a hard link and falls back to a cancellable copy.
- Per-stage artifact manifests are validated before cached stages are restored.
- Cancellation returns an interrupted running stage to Pending.
- Worker snapshots are reconciled through the queue so a terminal transition applies pause-after-current once and duplicate final snapshots are idempotent.
- Worker snapshots cannot mutate immutable job inputs/settings.
- The currently connected native backend implements Prepare only. Analyze and all later stages report an explicit not-implemented failure.

## Recovery provenance

Some files were available as exact preserved source late in development, notably the Slint dashboard structure, the Prepare backend, and the worker bridge shape. Core modules were reconstructed to the preserved public interfaces and tested behavioral contracts where the exact final text was not retained.

Do not treat commit hashes in this new repository as equivalent to the original repository hashes. They represent the reconstructed lineage.

## Validation rule

A recovery checkpoint is considered fully green only after the same Windows gate succeeds on the exact head:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. `cargo build -p storyteller-ui`
