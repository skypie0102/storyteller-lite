# StoryTeller Lite — Agent Guide

This file is the first-stop guide for any coding agent working in this repository.

## Start here

Before changing code, read these files in order:

1. `AGENTS.md` — this file.
2. `docs/HANDOFF.md` — current implementation state, last validated work, and next task.
3. `docs/ROADMAP.md` — canonical product scope and implementation order.
4. `docs/recovery/README.md` — provenance and authority of recovered information.
5. Relevant focused recovery notes under `docs/recovery/` for the subsystem being changed.

Do not infer current status from old chat history, old branches, or the legacy installer when the handoff/roadmap already settles the question.

## Canonical branch and branch safety

- `recovery/rust-slint` is the canonical integration branch for the recovered Rust + Slint Lite application.
- `main` is not the active reconstruction branch. Do not modify or move `main` unless the user explicitly asks.
- Start feature work from the current head of `recovery/rust-slint`.
- Keep feature slices narrow and reviewable.
- Integrate validated work into `recovery/rust-slint` with a normal non-forced fast-forward whenever possible.
- Never force-update `recovery/rust-slint` to hide divergence.
- Do not resurrect stale feature branches whose assumptions conflict with the recovered design.

## Product scope

This repository is **StoryTeller OneClick Lite**, not a full restoration of historical OneClick.

Preserve the intentionally reduced product:

- Rust + Slint native application.
- Queue-first workflow.
- Fixed seven stages: Prepare → Analyze → Align → Review Audio → Encode → Build EPUB → Validate.
- One overall progress bar backed by structured metrics.
- Automatic CPU/resource management.
- One user-facing Whisper worker-count setting; workers mean concurrent transcription chunks/tracks, not manual CPU allocation and not whisper.cpp `-p`.
- Smart / ReviewAll unmatched-audio policy.
- Conservative automatic edge handling.
- Durable per-segment review decisions and a reduced allocator.
- Lazy/on-demand OCR only for bounded EPUB image candidates.
- Native EPUB finishing and validation.

Do **not** restore by default:

- Node/npx architecture.
- Manual CPU/thread allocation UI.
- Full historical settings complexity.
- Runtime Health or activity-console-first UI.
- Word-level sync.
- CSS editor.
- Permanent OCR toggle.
- General-purpose split/merge/trim/rule-editor tooling.
- Sigil as the finishing architecture.

If a proposed feature expands scope beyond the Lite roadmap, stop and check `docs/ROADMAP.md` before implementing it.

## Recovery-source authority

Use recovered evidence in this order:

1. Current repository code and canonical docs for present product decisions.
2. Preserved Lite planning history and supplied UI mockups for intended Lite behavior.
3. Recovered installer/frontend/backend behavior for historical semantics and regression clues.
4. Current upstream Storyteller code only as a cross-check when the old app dynamically depended on `@storyteller-platform/align@latest`.

The old installer is a static reference source only. Do not execute it. Do not copy GPL helper source wholesale unless licensing is explicitly resolved.

When archaeology and the Lite plan disagree, preserve the Lite plan unless the user explicitly chooses otherwise.

## GitHub Actions / hosted runner policy — use extremely sparingly

GitHub-hosted runners are a scarce external resource. Excessive automated runs can waste quota and may look abusive. **Use them as sparsely as possible.**

Mandatory rules:

- Prefer local/static reasoning, repository inspection, unit-level code review, and local `cargo` commands whenever they are available.
- Batch related fixes before requesting a hosted validation run.
- Do not create a new Actions run after every small edit.
- Do not repeatedly rerun the same cold build to poll for progress or to discover one formatting issue at a time.
- Do not use GitHub Actions as a source-code patching mechanism, formatter, auto-fixer, or bot-commit workflow.
- Source changes and `cargo fmt` fixes should be made directly on the feature branch before CI.
- Do not add temporary push-triggered workflows merely to get rapid feedback when the existing manual validation workflow can be used.
- Prefer the existing manual Windows validation workflow for the final Windows-specific gate of a meaningful feature slice.
- One hosted validation run per coherent slice should be the norm. A rerun is justified only after a concrete failure has been fixed.
- If a documentation-only commit should not run CI, use the repository-supported CI-skip convention where appropriate.
- Remove any temporary validation workflow immediately after its purpose is complete; do not leave feature-specific runner triggers in the integrated branch.
- Never start multiple redundant validation runs for the same SHA.

A normal validation sequence for a meaningful Rust/Slint slice is:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build -p storyteller-ui
```

Run these locally when possible. Use a hosted Windows runner only when Windows/native-Slint confirmation materially adds confidence that cannot be obtained locally.

## Testing and correctness rules

- Do not claim a feature is validated until the relevant checks have actually passed.
- Treat formatting-only, compile, unit-test, integration-test, and native-shell failures as different signals; fix the concrete failure instead of broadening scope.
- Add focused regression tests for recovered behavioral contracts when practical.
- Preserve source artifacts for provenance: automatic alignment should remain distinct from reviewed/effective alignment.
- Review decisions must be durable; preview/workspace artifacts may be rebuildable.
- Ambiguous unmatched audio must remain Pending rather than being silently discarded.
- Manual decisions override automatic Smart decisions.
- Smart automation must be conservative, auditable, and reversible.
- Keep EPUB path/resource handling bounded and validated; never let UI-provided free-form package paths become authoritative.
- Publication must remain staged/validated/atomic and must not overwrite source EPUBs.

## Current recovery principles that must not regress

- Whisper workers are concurrent deterministic chunks/tracks. Do not map the worker count directly to whisper.cpp processors or `-p`.
- Execution-only settings such as worker count must not unnecessarily invalidate semantic checkpoints.
- The Analyze stage must not regress to a permanent whole-book PCM `audio.wav` artifact.
- Queue terminal behavior advances to the next pending book unless Pause/Stop-After is active; this includes completed, failed, and cancelled current books.
- Introduction/Credits can be real supplemental XHTML + SMIL destinations.
- Graphic Readout/image placement requires its dedicated bounded image path; do not fake image assignments as ordinary text-block matches.
- OCR is lazy and internal, not a permanent user-facing toggle.

## UI guidance

Use `docs/ui-guides/README.md` and the supplied mockups as visual/product guides, not pixel-perfect mandates. Preserve the simplified Lite interaction model.

The allocator should remain focused on unresolved audio: preview/seek, transcript/timing/silence context, previous/next navigation, Smart suggestion/evidence, bounded EPUB candidate context, explicit assignment/exclusion, and durable decisions. Do not turn it into a general audio editor.

## When finishing a slice

Before handing work off:

- Ensure the feature branch contains only intended source/docs/test changes.
- Remove temporary scripts/workflows/markers.
- Update `docs/HANDOFF.md` and `docs/ROADMAP.md` when implementation state materially changed.
- Record the actual validation result/run only if it happened.
- State unresolved caveats explicitly.
- Leave the next task precise enough that a new agent can continue without redoing archaeology.
