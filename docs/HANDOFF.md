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
7. `docs/recovery/FRONTEND_AND_CONCURRENCY_RECOVERY.md` — recovered old frontend state machine, exact allocator invariants, backend defaults, and persistence/concurrency behavior.
8. `docs/recovery/WORKER_SEMANTICS.md` — focused evidence for what old Parallel Whisper jobs meant and the preferred Lite worker architecture.
9. `docs/recovery/UNMATCHED_AUDIO_RECOVERY.md` — recovered Smart classification, automatic edge handling, lazy OCR, and reduced allocator scope.
10. `tools/recovery/extract_legacy_nsis.py` — reproducible static extractor for the exact known installer hash.
11. `tools/recovery/extract_tauri_assets.py` — reproducible recovery/verification of the old embedded Tauri frontend assets.

## User decisions recovered in this chat

### Keep / restore

- Queue-first workflow.
- Fixed seven-stage pipeline: Prepare → Analyze → Align → Review Audio → Encode → Build EPUB → Validate.
- Exactly one overall progress bar with real structured metrics.
- Existing automatic CPU-thread selection for Whisper.
- **A simple user-facing Whisper worker-count setting**, default `1` for Lite unless benchmarking/product evidence changes it.
- Worker count means **concurrent transcription chunks/tracks**, not manual CPU allocation and not a direct alias for whisper.cpp `-p`.
- **Smart / ReviewAll** unmatched-audio policy.
- **Automatic edge handling/trimming** for safe high-confidence cases.
- **Lazy/on-demand OCR** of bounded EPUB image candidates; no permanent OCR toggle.
- **Manual allocation for unresolved/unaligned audio** in a reduced Lite-specific review screen.
- Limited useful classifications such as Introduction, Credits, Graphic Readout, and Extra Audio when they materially affect destination/build behavior.
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
- Full historical allocator/editor complexity: unrestricted split/merge/trim/rules/general-purpose classification editing.

The recovered planning transcript previously said worker/thread controls could be removed. The newer explicit user request for a **worker count** overrides that old note; it does **not** restore manual CPU allocation. Likewise, removing the old permanent OCR preference does not remove lazy OCR from Smart review.

## Current code behavior relevant to pending work

As of the recovered branch inspected during this chat:

- `spawn_job_worker()` derives all logical CPU threads from `std::thread::available_parallelism()`.
- `LitePipelineBackend` passes that value to whisper.cpp with `-t`.
- Analyze currently converts the whole audiobook into one `audio.wav` and launches one whisper-cli process, so there is only one transcription work item per book.
- `JobSettings` currently contains audio encoding, language override, and Whisper model; no worker-count field exists.
- Settings UI currently focuses on runtime dependency discovery/install/import.
- Review Audio currently writes `review.json`, previews unmatched segments, and only offers Cancel or global **Continue without unmatched audio**.
- `AudioReviewReport` has a global `accepted_unmatched_exclusion` flag; there is no durable per-segment manual assignment model yet.
- Smart edge handling, lazy OCR/image classification, and ReviewAll are not implemented in the current recovered code.
- Alignment is conservative/monotonic and accepted segments map to single XHTML block ranges.

Inspect the current source before implementation because the branch may have advanced since this snapshot.

## Installer recovery status

Static analysis of the user-provided v0.39.0 installer is reproducible and documented under `docs/recovery/`.

Important recovered clues:

- The old Tauri/Rust app had separate `threads`, `parallelTranscribes`, and `parallelTranscodes` settings. Historical validation allowed 1–32 CPU threads, 1–4 parallel transcription jobs, and 1–8 parallel FFmpeg jobs.
- Machine-code recovery confirms the old backend defaults were `threads=6`, `parallelTranscribes=3`, and `parallelTranscodes=6`, with `npx`, `large-v3-turbo`, `en-US`, and `64K`. These are historical defaults only, not Lite defaults.
- **Critical:** the old alignment launcher passed `--processors 1` separately from `--parallel-transcribes <parallelTranscribes>`. Therefore historical `parallelTranscribes` was higher-level job concurrency and was **not** whisper.cpp processor count / `-p`.
- A current upstream Storyteller source cross-check confirms the same conceptual split: `parallelTranscribes` is a semaphore over multiple processed audio files, while each file has independent Whisper `processors`/`threads`. Current preprocessing splits long audio into bounded chapter/VAD-safe tracks. See `docs/recovery/WORKER_SEMANTICS.md`.
- Old manual-allocation IPC included draft save/restore, pending request retrieval, audio preview, image preview, submit, and pause/cancel operations.
- Old persisted state contained `manualDrafts`; retry after interruption could reopen/restore allocations. The recovered frontend autosaved after a 450 ms debounce and also kept a local fallback copy.
- Durable decisions were separate from disposable/rebuildable preview workspaces. Preserve that boundary in Lite.
- The installer contains the old finishing helper's **plain Python source**. Its manual allocation code enforces complete time coverage, no gaps/overlaps, explicit targets, and final output auditing.
- The old helper's unmatched discovery was primarily edge-focused (Introduction/Credits), while its automatic-player path also used bounded image candidates and OCR/text hints to detect Graphic Readouts.
- Historical OCR was already bounded/lazy in implementation: candidate documents/images were narrowed first, embedded text hints were used first where possible, and OCR ran only on relevant candidates. See `docs/recovery/UNMATCHED_AUDIO_RECOVERY.md`.
- The helper is GPLv3-or-later/Sigil-derived. Treat it as a behavioral/test-vector reference unless licensing for direct code reuse is deliberately resolved.

## Immediate implementation order

### P0 — validate current head

Before feature changes, establish that the current branch builds/tests on the intended Windows validation path. Do not assume historical CI claims in the planning transcript still apply.

### P1 — Whisper workers setting and chunked Analyze

Implement one simple user-facing worker-count setting while retaining automatic CPU-thread selection.

Recovered evidence gives a preferred semantic definition: **worker count bounds concurrently transcribed audio chunks/tracks**, not whisper.cpp `-p`.

Requirements:

- Lite default `1` unless deliberate benchmarking changes it;
- user-adjustable from Settings;
- initial range can reasonably benchmark 1–4, matching the old parallel-job range without treating it as a compatibility mandate;
- split/decode long audiobook input into deterministic ordered transcription work items so `workers > 1` actually has work to schedule;
- prefer chapter-safe boundaries and a tested silence/VAD-aware fallback for overlong/no-chapter ranges;
- run at most `workers` Whisper transcription tasks concurrently;
- keep each individual whisper.cpp invocation at one processor initially; do **not** implement this setting by simply passing `-p N`;
- automatically budget per-worker CPU threads/resources instead of restoring manual thread controls or giving every simultaneous worker all logical CPUs;
- add each chunk's global audio offset to its local Whisper timestamps, then merge and validate one chronological Analyze transcript artifact;
- aggregate progress/cancellation across active workers;
- avoid CPU/GPU/VRAM oversubscription, especially when the model is loaded by multiple concurrent GPU processes;
- treat worker count as **execution/performance configuration**, not semantic output configuration. Do not invalidate reusable Analyze/Align checkpoints solely because N changed if the merged transcript contract is deterministic/equivalent.

`docs/recovery/WORKER_SEMANTICS.md` contains the evidence and implementation cautions in detail.

Prefer an app/runtime settings model separate from output-affecting `JobSettings` fingerprints. If queued-job determinism requires storing it per job, exclude it from semantic stage fingerprints.

### P2 — Smart unmatched-audio pipeline and reduced manual allocator

Replace the current all-or-nothing unmatched-audio review with the recovered Lite model. See `docs/recovery/UNMATCHED_AUDIO_RECOVERY.md`.

Required behavior:

- reduced **Smart / ReviewAll** policy;
- conservative automatic edge trimming/handling;
- bounded candidate generation from EPUB order/context;
- lazy/on-demand OCR and embedded image-text hints only when Smart or the current review segment needs them;
- optional small classification/suggestion set such as Introduction, Credits, Graphic Readout, Extra Audio when useful for destination/build behavior;
- high-confidence Graphic Readout → valid image page/document assignment where evidence supports it;
- unresolved segments remain pending instead of being silently discarded;
- durable per-segment decisions with autosave/retry restoration;
- previous/next unresolved navigation, audio preview/seek, transcript/timing/silence context, Smart suggestion, EPUB candidate context, explicit assignment/exclusion/override, Apply & Next;
- monotonic EPUB ordering and real block/image candidate validation;
- retain automatic alignment and decision provenance for audit/debugging while materializing an effective downstream allocation/alignment result;
- validate/audit output after automatic/manual decisions.

A base decision model should support Pending, Assigned, and Excluded, but preserve optional classification/suggestion/provenance rather than collapsing useful Smart information.

Do **not** initially implement arbitrary split/merge, a general waveform trim editor, permanent OCR controls, broad Apply-to-similar rules, or an unrestricted old-app category/destination editor.

### P3 — align Slint UI with the supplied mockups

Use the main UI mockup for hierarchy and information density and the allocator mockup for the dedicated review experience. They are wide-window guides, not fixed pixel canvases.

## Engineering rule

Do not start with broad legacy deletion. The preserved historical material is useful for regression behavior. Use:

> Replace → regression-test → delete.

When old behavior and Lite scope conflict, prefer current Lite scope and document the decision.
