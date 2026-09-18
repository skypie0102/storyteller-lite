# CI policy

Storyteller Lite intentionally minimizes use of GitHub-hosted runners.

## Default policy

- Pushes do **not** start permanent hosted CI.
- Ordinary pull requests do **not** start permanent hosted CI.
- Validation runs only when a maintainer explicitly starts a manual workflow or creates a narrowly scoped temporary validation workflow/PR for a meaningful checkpoint.
- Temporary validation workflows are removed after their checkpoint; they are not a standing PR tax.
- Use `core` for routine validation. It runs only rustfmt, `storyteller-core` Clippy, and `storyteller-core` tests on Ubuntu.
- Use `full-windows` only at meaningful checkpoints where the native Slint shell or Windows-specific behavior must be validated.
- The workflow cancels an older validation for the same ref when a replacement is started.
- Jobs have hard time limits so a stuck build cannot consume a runner indefinitely.
- After a hosted-run failure, inspect the full failure surface/logs first, batch related fixes, and make the next run a meaningful near-final checkpoint rather than using Actions as an edit/compile loop.

## Local-first commands

Run these locally whenever a Rust toolchain is available:

```text
cargo fmt --all -- --check
cargo clippy -p storyteller-core --all-targets -- -D warnings
cargo test -p storyteller-core
```

Before a release or a substantial UI/integration checkpoint, run the full checks locally on Windows when possible:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p storyteller-ui
```

## Dependency locking

`Cargo.lock` is committed and is part of the application build contract. Keep it under version control and review resolver changes like source changes. Release/developer-test packaging uses `cargo build --locked` so a package build fails instead of silently resolving a different dependency graph.

Do not regenerate or update the lockfile merely to make CI move forward. Change it when dependency declarations intentionally change, then validate that change with the appropriate core or full-Windows checkpoint.

## Windows developer-test packaging

`.github/workflows/windows-build.yml` is manual-only. It runs the full workspace validation surface before a locked release build, then emits a short-lived Windows developer-test package. The package includes a machine-readable `BUILD.json` with app version, commit, target, executable name, and executable SHA-256; `SHA256SUMS.txt` is independently checked against the built EXE before upload.

The package is not a public installer or auto-update channel. StoryTeller-owned runtime downloads remain user-initiated and are stored under the per-user application-data root rather than requiring the packaged EXE directory to be writable.

## When to use full Windows validation

Use the expensive hosted check sparingly, for example after:

- changing Slint UI/build integration;
- changing Windows-specific filesystem/process behavior;
- changing workspace dependencies or Rust toolchain assumptions;
- preparing a release or merge checkpoint.

Do not use full Windows validation for documentation-only changes, comments, formatting-only changes already checked locally, or every intermediate commit.
