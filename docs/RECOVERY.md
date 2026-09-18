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
- Resume fingerprints include source identities, settings, Whisper/alignment/audio/EPUB backend identities, effective language, and effective Whisper model/runtime identity.
- Prepare always copies the EPUB; audiobook staging prefers a hard link and falls back to a cancellable copy.
- Per-stage artifact manifests are validated before cached stages are restored.
- Cancellation returns an interrupted running stage to Pending.
- Worker snapshots are reconciled through the queue so a terminal transition applies pause-after-current once and duplicate final snapshots are idempotent.
- Worker snapshots cannot mutate immutable job inputs/settings.
- Worker-thread panics terminalize the active job instead of leaving it Running and eligible for accidental restart.
- Failed/cancelled jobs support an explicit safe retry-from-start path that clears progress and checkpoints.
- Waiting jobs can be queued while processing is active, reordered, or removed; recent terminal jobs remain visible in the native shell.
- Human review is a first-class `NeedsReview` state rather than a fake failure or completion.
- Final publication is separated from backend computation: Validate artifacts are captured before the publication finalizer runs.

## Current processing implementation

All seven development stages have concrete backends on the recovery branch:

- **Prepare** stages validated source files in an isolated job workspace.
- **Analyze** extracts bounded EPUB reading-order text, converts audio to 16 kHz mono PCM with ffmpeg, and produces timestamped `whisper.cpp` JSON-full output. Segment timing must be positive and non-overlapping.
- **Align** uses the conservative `monotonic-ngram-edit-v2-block-safe` engine. Accepted matches retain real Whisper segment timing and cannot cross normalized XHTML block boundaries; weak segments stay unmatched.
- **Review Audio** writes durable unmatched ranges/text to `review.json` and pauses only when a real user decision is required. Continuing records explicit acceptance to exclude unmatched audio from synchronization.
- **Encode** supports cancellable Copy for safely identifiable EPUB core audio, plus ffmpeg Opus/AAC. Generated Opus uses `audio/ogg; codecs=opus`; AAC uses `audio/mp4`. The stage writes `encoded-audio.json` alongside the audio artifact.
- **Build EPUB** targets EPUB 3 source packages without pre-existing Media Overlays. It creates deterministic XHTML block anchors, SMIL Media Overlays using real segment clips, embeds encoded audio, updates package associations and duration metadata, and writes a workspace candidate without modifying the source EPUB.
- **Validate** independently reopens that candidate and audits container/package/SMIL/text/audio relationships and durations. It writes `validation.json`; only after artifact capture does the finalizer publish beside the source. Existing output files are never overwritten.

An end-to-end core fixture now exercises build -> structural validation -> publish on a minimal EPUB and verifies no-overwrite behavior.

## Recovery provenance

Some files were available as exact preserved source late in development, notably the Slint dashboard structure, the original Prepare backend, and the worker bridge shape. Core modules were reconstructed to the preserved public interfaces and tested behavioral contracts where the exact final text was not retained. Development after the recovery point is new work in this repository rather than an attempt at byte-identical restoration.

Do not treat commit hashes in this new repository as equivalent to the original repository hashes. They represent the reconstructed lineage.

## Validation status

The last fully validated checkpoint in the new repository remains:

`55d2c262e327151a07cbffccdb8d0e398b2807c2`

Its Windows run passed:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. `cargo build -p storyteller-ui`

The complete seven-stage development head is newer than that checkpoint. Later development commits intentionally have not launched GitHub-hosted Actions. Hosted validation is manual-only under `docs/CI_POLICY.md` to minimize public runner use. Therefore the current branch must not be called fully green until an exact-head validation is deliberately performed.
