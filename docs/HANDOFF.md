# Agent handoff — Storyteller OneClick Lite recovery

Read this file before making substantial changes.

## Current repository truth

- Repository: `skypie0102/storyteller-lite`
- Active recovered code branch: `recovery/rust-slint`
- Head when this recovery packet was created: `56a48b4142caa89f49cd9020bc537c698932d57c`
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

## User decisions recovered in this chat

### Keep / restore

- Queue-first workflow.
- Fixed seven-stage pipeline: Prepare → Analyze → Align → Review Audio → Encode → Build EPUB → Validate.
- Exactly one overall progress bar with real structured metrics.
- Existing automatic CPU-thread selection for Whisper.
- **A simple user-facing Whisper worker-count setting**, default `1`.
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

## Immediate implementation order

### P0 — validate current head

Before feature changes, establish that the current branch builds/tests on the intended Windows validation path. Do not assume historical CI claims in the planning transcript still apply.

### P1 — Whisper workers setting

Implement one simple setting for the number of whisper.cpp processors/workers.

Requirements:

- default `1`;
- user-adjustable from Settings;
- validate/clamp to a sensible positive range;
- pass the value through to whisper.cpp as the appropriate current `-p`/processor option after confirming the bundled/current whisper.cpp CLI semantics;
- keep automatic CPU-thread selection — do not add manual CPU allocation;
- avoid obvious CPU/GPU oversubscription after verifying how `-t` and `-p` interact in the exact whisper.cpp build being shipped;
- treat worker count as **execution/performance configuration**, not semantic output configuration. Do not make changing worker count invalidate otherwise reusable Analyze/Align checkpoints unless the backend actually produces semantically different results.

Prefer an app/runtime settings model separate from output-affecting `JobSettings` fingerprints. If implementation constraints require storing it on a queued job for determinism, exclude it from semantic stage fingerprints.

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
- durable decisions that survive resume/review transitions.

Preserve monotonic EPUB order. Candidate assignments should normally be constrained between the nearest accepted matched neighbors, with validation rejecting backwards/cross-block-invalid results.

Do **not** initially implement the mockup’s split/merge/trim editor, permanent OCR controls, automatic apply-to-similar, or full historical classification system unless a concrete Lite requirement emerges.

### P3 — align Slint UI with the supplied mockups

Use the main UI mockup for hierarchy and information density and the allocator mockup for the dedicated review experience. They are wide-window guides, not fixed pixel canvases.

## Engineering rule

Do not start with broad legacy deletion. The preserved historical material is useful for regression behavior. Use:

> Replace → regression-test → delete.

When old behavior and Lite scope conflict, prefer current Lite scope and document the decision.
