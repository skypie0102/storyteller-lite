# Recovered frontend and concurrency behavior — v0.39.0

This document captures additional behavior recovered statically from the embedded Tauri frontend and compiled Rust backend in the user-provided Storyteller One-Click v0.39.0 installer.

It is historical evidence for the Lite rebuild, not a requirement to restore the old full application's feature surface.

## Embedded frontend recovery

The old desktop executable contains the Tauri web assets as Brotli-compressed embedded resources. Static recovery yielded at least:

- `/index.html`
- `/assets/index-CudKkaga.js`
- `/assets/index-DJf4moyJ.css`
- `/assets/main-current-CPAuPPOe.css`
- `/assets/allocator-current-CszKz2tT.css`

The recovered JavaScript is the minified production React/Tauri bundle and exposes the real IPC names, settings labels, allocator state transitions, validation rules, and autosave behavior.

Do not commit/copy the old bundled frontend wholesale into the Lite implementation. Use this document and narrow extracted invariants as the migration reference.

## Production Settings defaults recovered from Rust machine code

The browser bundle contains a non-Tauri fallback snapshot with machine-specific paths. Those paths are demo/fallback values and are not production defaults.

However, the compiled Rust backend's actual default constructor was located in the executable and independently confirms these old defaults:

- engine: `npx`
- model: `large-v3-turbo`
- language: `en-US`
- bitrate: `64K`
- threads per Whisper job: `6`
- parallel transcription jobs: `3`
- parallel FFmpeg jobs: `6`

The constructor also installs the built-in read-aloud CSS. Optional runtime-path values are not constructed there, so machine-specific Node/FFmpeg paths seen in the browser fallback must not be treated as defaults.

These are **old OneClick defaults**, not automatically Lite defaults. Current Lite product decisions override them.

## Critical concurrency finding

The old UI labeled the controls:

- `Parallel Whisper jobs` — valid range 1–4
- `Parallel FFmpeg jobs` — valid range 1–8
- `Threads per Whisper job` — valid range 1–32

The old alignment launcher command was recovered from the compiled backend. It constructed arguments equivalent to:

```text
--epub <...>
--audiobook <...>
--output <...>
--engine whisper.cpp
--model <...>
--language <...>
--threads <settings.threads>
--processors 1
--parallel-transcodes <settings.parallelTranscodes>
--parallel-transcribes <settings.parallelTranscribes>
--granularity <sentence|word>
...
```

Machine-code references identify the three numeric settings independently:

- `threads` from the Settings byte used by `--threads`
- `parallelTranscodes` from the byte used by `--parallel-transcodes`
- `parallelTranscribes` from the byte used by `--parallel-transcribes`

`--processors` was explicitly followed by constant `1`.

### Consequence for Lite

Historical `parallelTranscribes` was **not whisper.cpp's processor count**. It was a higher-level concurrency control in the old alignment launcher, while that launcher fixed Whisper `processors` to one.

Therefore the new Lite request for a user-facing **Whisper worker count** must be designed deliberately. Do not claim historical compatibility by simply mapping the old `parallelTranscribes` value to whisper.cpp `-p`.

When implementation begins, first decide which current-Lite behavior the setting controls:

1. whisper.cpp's current internal processor/parallel option, if verified to provide the intended worker semantics; or
2. Lite-managed transcription/chunk concurrency, if that is the safer and faster architecture for long audiobooks.

In either case, keep Lite's automatic CPU-thread selection unless the product requirement changes, and avoid making a performance-only setting invalidate semantic Analyze/Align checkpoints unnecessarily.

## Allocator frontend: durable state behavior

The recovered frontend calls these manual-allocation IPC commands:

- `get_pending_manual_request`
- `get_manual_allocation_draft`
- `save_manual_allocation_draft`
- `manual_audio_preview_path`
- `manual_image_preview`
- `submit_manual_allocations`
- `cancel_manual_allocation`

The last name is singular in the frontend IPC call.

### Draft restoration

When the allocator opens it:

1. requests the current pending allocation session;
2. builds a fresh initial draft;
3. attempts to load the backend-persisted draft;
4. falls back to a per-job browser/local-storage draft if needed;
5. validates/normalizes the restored structure before accepting it.

The UI distinguishes:

- `Restored the allocator autosave after restart.` for a backend draft;
- `Restored the previous allocator autosave.` for the local fallback.

If the old draft is malformed, the allocator discards it and creates a fresh allocation.

### Autosave cadence

Every draft change starts a **450 ms debounce**. When it fires, the frontend:

- writes a local-storage copy; and
- calls `save_manual_allocation_draft` with the normalized allocations.

This is strong evidence that allocator edits were intentionally durable continuously, not only when the user pressed Apply.

### Pause/close behavior

Pausing manual allocation explicitly tells the user that autosaved edits are retained and the book waits until **Retry**.

A normal allocator-window close request is intercepted. If the allocation is still pending, closing routes through the same pause behavior instead of silently abandoning edits.

The queue had distinct states for:

- `waiting_for_allocation`
- `allocation_paused`

Retrying a paused allocation returned it to the manual-allocation waiting state.

### Apply behavior

Apply is blocked while any allocation validation error exists. The confirmation text states that Storyteller will render the draft and then audit output audio/SMIL before saving.

That reinforces the old invariant that a manual decision is not trusted merely because the UI accepted it: generated output still undergoes structural/media validation.

## Allocator frontend: initial decisions and normalization

Old category labels were:

- `introduction` → Audiobook introduction
- `graphic_readout` → Graphic readout
- `credits` → Audiobook credits
- `other` → Other extra audio
- `silence` → Silence / noise

For a fresh old-style edge segment, the frontend initially created one allocation spanning the entire segment:

- fully silent segment → `silence` + `discard`;
- first opening segment → `introduction` + `page`;
- first closing segment → `credits` + `page`;
- later same-side segments → `other` + `page`.

Draft normalization also enforced at most one Introduction and one Credits allocation globally; duplicates were downgraded to `other`/`page`.

This taxonomy is historical. Lite should preserve the useful invariant — every unresolved region gets an explicit durable disposition — without restoring the whole category system by default.

## Exact frontend allocation validation

For each old inspection segment, frontend validation required:

- finite positive segment duration;
- at least one allocation row;
- finite, non-negative row start/end times;
- rows sorted into continuous coverage;
- gap/overlap tolerance of **0.002 seconds**;
- allocation duration greater than **0.01 seconds**;
- no allocation extending beyond segment duration;
- final coverage reaching segment end within **0.002 seconds**;
- target must be `page`, `image`, or `discard`;
- `image` requires both `document_path` and `image_path`;
- `discard` is allowed only for `silence`.

The frontend considered a range fully silent when detected silence intervals continuously covered it allowing at most **0.08 seconds** between markers/edges.

Split suggestions were built from silence markers of at least **0.35 seconds** and ranked higher when close to transcript boundaries.

These numerical values are useful historical test vectors. They are not necessarily the correct values for the reduced Lite allocator, which initially does not need split/merge editing.

## Undo/redo and editing scope

The old allocator retained up to **50** undo/redo states. It also had split/merge/trim controls, timeline editing, keyboard shortcuts, ordering controls, image targets, and classification UI.

Those features explain the complexity visible in the old code but are explicitly **not the initial Lite target**. The supplied Lite allocator mockup remains the visual guide, and the current Lite roadmap intentionally starts with a reduced per-segment assignment/exclusion workflow.

## What Lite should retain from this evidence

The strongest behaviors to carry forward are:

1. manual review is a durable workflow state, not a modal one-shot prompt;
2. drafts are persisted while the user edits;
3. closing/pausing review must not silently lose work;
4. every reviewed audio region receives an explicit disposition;
5. invalid/backwards/ambiguous assignments are rejected before continuing;
6. generated EPUB/media-overlay output is audited after manual decisions;
7. execution parallelism is separate from semantic job settings and from per-worker CPU threads.

What Lite should **not** infer from the old app:

- that `parallelTranscribes` means whisper.cpp `-p`;
- that the old values 6/3/6 should become Lite defaults;
- that the full split/merge/trim editor must be rebuilt;
- that the old edge-only unmatched-audio discovery policy should replace Lite's current segment-level alignment review;
- that machine-specific browser fallback paths are real settings defaults.
