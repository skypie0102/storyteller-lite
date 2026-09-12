# CI policy

Storyteller Lite intentionally minimizes use of GitHub-hosted runners.

## Default policy

- Pushes do **not** start hosted CI.
- Pull requests do **not** start hosted CI.
- Validation runs only when a maintainer explicitly starts the `Manual Rust validation` workflow.
- Use `core` for routine validation. It runs only rustfmt, `storyteller-core` Clippy, and `storyteller-core` tests on Ubuntu.
- Use `full-windows` only at meaningful checkpoints where the native Slint shell or Windows-specific behavior must be validated.
- The workflow cancels an older validation for the same ref when a replacement is started.
- Jobs have hard time limits so a stuck build cannot consume a runner indefinitely.

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

This recovered repository does not yet contain `Cargo.lock`. Do not start a hosted run solely to create it. The next intentional Cargo validation will generate a lockfile in its checkout; generate and commit the same lockfile from a local Rust environment when available. Keeping `Cargo.lock` committed is recommended for this application workspace because it reduces resolver churn and makes validation reproducible.

## When to use full Windows validation

Use the expensive hosted check sparingly, for example after:

- changing Slint UI/build integration;
- changing Windows-specific filesystem/process behavior;
- changing workspace dependencies or Rust toolchain assumptions;
- preparing a release or merge checkpoint.

Do not use full Windows validation for documentation-only changes, comments, formatting-only changes already checked locally, or every intermediate commit.
