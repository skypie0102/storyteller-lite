# Agent handoff — Storyteller OneClick Lite recovery

Read this file before making substantial changes.

## Current repository truth

- Repository: `skypie0102/storyteller-lite`
- Active recovered code branch: `recovery/rust-slint`
- Default branch `main` is not the code branch to use for the Rust + Slint implementation.

The recovered planning transcript mentions a historical `refactor/rust-slint` branch and several commit SHAs. Those are useful clues but are **not authoritative current repository state**. Inspect the live branch before relying on them.

## Product identity

Storyteller Lite is the Rust + Slint successor to Storyteller OneClick. The pre-Rust application is a behavioral reference where needed, but Lite is deliberately smaller and should not become a line-for-line port.

Primary references:

1. `docs/ROADMAP.md` — canonical current roadmap/product scope.
2. `docs/ui-guides/README.md` + mockups — visual/product references.
3. `docs/recovery/README.md` — provenance and source hierarchy.
4. `docs/recovery/LITE_PLANNING_HISTORY.txt` — recovered historical planning transcript.
5. `docs/recovery/LEGACY_INSTALLER_REFERENCE.md` — old installer provenance and usage rules.
6. `docs/recovery/INSTALLER_DISSECTION.md` — facts recovered by static analysis of the v0.39.0 installer.
7. `docs/recovery/FRONTEND_AND_CONCURRENCY_RECOVERY.md` — recovered old frontend state machine, exact allocator invariants, backend defaults, and concurrency semantics.
8. `tools/recovery/extract_legacy_nsis.py` — reproducible static extractor for the exact known installer hash.

## User decisions recovered in this chat

### Keep / restore

- Queue-first workflow.
- Fixed seven-stage pipeline: Prepare → Analyze → Align → Review Audio → Encode → Build EPUB → Validate.
- Exactly one overall progress bar with real structured metrics.
- Existing automatic CPU-thread selection for Whisper.
- **A simple user-facing Whisper worker-count setting**, default `1` for Lite unless benchmarking/product evidence changes it.
- **Manual allocation for unresolved/unaligned audio** in a reduced Lite-specific review screen.
- Old installer only as a behavioral reference when needed.
- The supplied mockups as current UI layout guides.

### Do not restore by default

- Manual CPU/thread allocation UI.
- Full old OneClick settings complexity.
- Word-level synchronization.
- Activity console as primary UI.
- Runtime Health page.
- Process-now flow.
- Engine/runtime path tuning UI.
- Standardize-EPUB toggle.
- CSS editor.
- Permanent OCR toggle.
- Full historical allocator/editor complexity.

The recovered planning transcript previously said worker/thread controls could be removed. The newer explicit user request for a **worker count** overrides that old note; it does **not** restore manual CPU allocation.

## Current code behavior relevant to pending work

As of the recovered branch inspected during this chat:

- `spawn_job_worker()` derives all logical CPU threads from `std::thread::available_parallelism()`.
- `LitePipelineBackend` passes that value to whisper.cpp with `-t`.
- The app does **not** currently pass whisper.cpp `-p`.
- `JobSettings` currently contains audio encoding, language override, and Whisper model; no worker-count field exists.
- Settings UI currently focuses on runtime dependency discovery/install/import.
- Review Audio currently writes `review.json`, previews unmatched segments, and only offers Cancel or global **Continue without unmatched audio**.
- `AudioReviewReport` has a global `accepted_unmatched_exclusion` flag; there is no durable per-segment manual assignment model yet.
- Alignment is conservative/monotonic and accepted segments map to single XHTML block ranges.

Inspect the current source before implementation because the branch may have advanced since this snapshot.

## Installer recovery status

Static analysis of the user-provided v0.39.0 installer is reproducible and documented under `docs/recovery/`.

Important recovered clues:

- The old Tauri/Rust app had separate `threads`, `parallelTranscribes`, and `parallelTranscodes` settings. Historical validation allowed 1–32 CPU threads, 1–4 parallel transcription jobs, and 1–8 parallel FFmpeg jobs.
- Machine-code recovery confirms the old backend defaults were `threads=6`, `parallelTranscribes=3`, and `parallelTranscodes=6`, with `npx`, `large-v3-turbo`, `en-US`, and `64K`. These are historical defaults only, not Lite defaults.
- **Critical:** the old alignment launcher passed `--processors 1` separately from `--parallel-transcribes <parallelTranscribes>`. Therefore historical `parallelTranscribes` was higher-level job concurrency and was **not** whisper.cpp processor count / `-p`.
- Old manual-allocation IPC included draft save/restore, pending request retrieval, audio preview, image preview, submit, and pause/cancel operations.
- Old persisted state contained `manualDrafts`; retry after interruption could reopen/restore allocations. The recovered frontend autosaved after a 450 ms debounce and also kept a local fallback copy.
- The installer contains the old finishing helper's **plain Python source**. Its manual allocation code enforces complete time coverage, no gaps/overlaps, explicit targets, and final output auditing.
- The old helper's discovery logic was primarily edge-focused (Introduction/Credits) rather than a generic internal-segment allocator. Do not blindly copy that discovery policy into Lite's current arbitrary unmatched-segment review model.
- The helper is GPLv3-or-later/Sigil-derived. Treat it as a behavioral/test-vector reference unless licensing for direct code reuse is deliberately resolved.

## Immediate implementation order

### P0 — validate current head

Before feature changes, establish that the current branch builds/tests on the intended Windows validation path. Do not assume historical CI claims in the planning transcript still apply.

### P1 — Whisper workers setting

Implement one simple user-facing worker-count setting while retaining automatic CPU-thread selection.

Requirements:

- Lite default `1` unless deliberate benchmarking changes it;
- user-adjustable from Settings;
- validate/clamp to a sensible positive range;
- **do not automatically equate this setting with the old `parallelTranscribes` or with whisper.cpp `-p`;** those are proven to be different historical concepts;
- before wiring it, verify the exact current whisper.cpp build's `-p` semantics and decide whether the desired Lite behavior is whisper.cpp internal processor parallelism or Lite-managed chunk/job concurrency;
- avoid obvious CPU/GPU/VRAM oversubscription;
- keep automatic CPU-thread selection — do not add manual CPU allocation;
- treat worker count as **execution/performance configuration**, not semantic output configuration. Do not make changing worker count invalidate otherwise reusable Analyze/Align checkpoints unless the backend actually produces semantically different results.

Prefer an app/runtime settings model separate from output-affecting `JobSettings` fingerprints. If implementation constraints require storing it on a queued job for deterministic execution, exclude it from semantic stage fingerprints.

### P2 — reduced manual audio allocator

Replace the current all-or-nothing unmatched-audio review with durable per-segment review decisions.

Minimum data model should support at least:

- Pending;
- Assigned to an EPUB block/range;
- Explicitly Excluded.

Minimum UI should support:

- previous/next unresolved segment;
- audio preview/seek for the segment;
- transcript and timing;
- candidate EPUB block/page context;
- explicit assignment;
- explicit exclusion/skip;
- Apply & Next;
- durable decisions that survive resume/review transitions and application restart/retry where practical.

Preserve monotonic EPUB order. Candidate assignments should normally be constrained between the nearest accepted matched neighbors, with validation rejecting backwards/cross-block-invalid results. Also preserve the old allocator's strong accounting rule: a reviewed segment must not silently develop uncovered gaps or overlaps.

Historical frontend details useful as test/reference evidence are in `docs/recovery/FRONTEND_AND_CONCURRENCY_RECOVERY.md`, including durable autosave, pause/retry semantics, exact old gap/overlap tolerances, and output auditing.

Do **not** initially implement the mockup’s split/merge/trim editor, permanent OCR controls, automatic apply-to-similar, or full historical classification system unless a concrete Lite requirement emerges.

### P3 — align Slint UI with the supplied mockups

Use the main UI mockup for hierarchy and information density and the allocator mockup for the dedicated review experience. They are wide-window guides, not fixed pixel canvases.

## Engineering rule

Do not start with broad legacy deletion. The preserved historical material is useful for regression behavior. Use:

> Replace → regression-test → delete.

When old behavior and Lite scope conflict, prefer current Lite scope and document the decision.
