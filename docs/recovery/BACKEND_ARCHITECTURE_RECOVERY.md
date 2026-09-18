# Backend architecture recovery — Storyteller One-Click v0.39.0

This note records module/source-path boundaries embedded in the old compiled Rust/Tauri executable. It is an architectural clue for reconstructing Lite, not a requirement to reproduce the old module tree.

## Recovered application source modules

Compiler source-path strings identify at least these old application modules:

```text
src/alignment_report.rs
src/integrity.rs
src/process_runtime.rs
src/state.rs

src/commands/app.rs
src/commands/manual.rs
src/commands/queue.rs
src/commands/tools.rs

src/services/manual.rs
src/services/queue.rs
src/services/settings.rs
src/services/tools.rs
```

The embedded strings and Tauri command table make the responsibilities of most modules fairly clear.

## Architectural shape

Conceptually the old desktop backend was separated like this:

```text
Tauri frontend
    |
    v
commands/*                 thin IPC boundary
    |
    v
services/*                 product/state operations
    |
    +------------+------------------+
    |            |                  |
    v            v                  v
state.rs   process_runtime.rs  integrity.rs
                 |
                 v
         external alignment / helper
                 |
                 v
         alignment_report.rs
```

This is useful because it shows that queue/manual/state behavior was not supposed to live inside the UI itself.

Lite should preserve the **separation of responsibilities**, while using native Rust/Slint interfaces instead of Tauri IPC.

## `state.rs`

Recovered serde metadata identifies the durable state model and several core structs.

`PersistedState` had exactly four top-level fields:

```text
settings
queue
runningJobId
manualDrafts
```

Other recovered serialized types include:

- `Settings`
- `JobDraft`
- `QueueJob`
- `PhaseTiming`
- `AlignmentStatistics`
- `ManualAllocation`
- `ManualSegment`
- `ManualSilenceMarker`
- `ManualTranscriptCue`
- `ManualImage`

The state module therefore owned durable product state rather than temporary processing media.

### Lite lesson

Keep durable job/settings/review decision state structurally separate from temporary stage/workspace artifacts. The current Lite job/checkpoint/workspace architecture already moves in this direction.

## `commands/*`

The recovered Tauri command names align with these source files.

### `commands/app.rs`

App/dependency-level operations. Recovered messages include:

- preventing dependency installation while a book is active;
- application snapshot/runtime interactions.

### `commands/queue.rs`

Queue control IPC such as:

- start queue / start single book;
- cancel current;
- stop after current;
- retry/reprocess;
- reorder/remove;
- clear completed / requeue failures.

### `commands/manual.rs`

Manual review IPC such as:

- pending allocation request;
- draft load/save;
- audio/image preview;
- submit/pause/cancel allocation.

### `commands/tools.rs`

One-off EPUB tool invocation boundary. Recovered messages explicitly refuse tools while the queue is active.

### Lite lesson

The command modules were boundary adapters, not the product model. Slint callbacks in Lite should likewise delegate into testable Rust services/core functions instead of accumulating queue/review/business logic in `main.rs` or UI callbacks.

## `services/queue.rs`

This appears to have been the central old workflow coordinator.

Recovered responsibilities/messages include:

- start pending book;
- output-conflict blocking;
- retry and reprocess validation;
- queue reorder/remove rules;
- dependency preflight;
- retry-safe workspace preparation;
- audiobook hard-link/copy staging;
- alignment process launch;
- alignment-report validation;
- unmatched-audio inspection;
- manual allocation wait/resume;
- EPUB finishing;
- final EPUB audit;
- output publication;
- terminal-state handling and auto-advance;
- workspace cleanup;
- restart normalization of interrupted jobs.

The old coordinator therefore mixed more orchestration into one service than the recovered Lite design should. Lite's split among runner/backend/workspace/scheduler/queue is cleaner and should be retained.

## `services/manual.rs`

Recovered messages indicate manual-review product operations including:

- verify the requested unmatched audio range still exists;
- generate/cache an allocator preview;
- validate selected graphic destination eligibility;
- pause manual allocation while retaining draft;
- resume manual allocation.

### Lite lesson

This maps naturally to a dedicated native review service/API behind the Slint allocator. Candidate generation, decision validation, preview lookup, autosave and apply should stay out of view code.

## `services/settings.rs`

Owned settings validation/persistence rather than UI formatting. The old serialized `Settings` type contained the large 17-field OneClick settings surface documented elsewhere.

### Lite lesson

The reduced Lite settings model should remain a core/runtime concept, with Slint only presenting the deliberately small supported surface.

## `services/tools.rs`

Owned isolated EPUB-tool execution. Recovered behavior:

- refuses to run while queue processing is active;
- creates a Storyteller-owned temporary/output context;
- invokes the Sigil-derived repair helper;
- verifies output existence;
- returns diagnostic output.

This old tooling service is mostly historical because Lite's direction is native EPUB processing, but its isolation reinforces the rule that one-off tools should not mutate active queue workspaces concurrently.

## `process_runtime.rs`

This module contained old process/pipeline execution helpers.

Recovered nearby strings and command construction show responsibility for:

- cancellation-aware process launching;
- alignment engine invocation;
- Sigil-derived finishing helper invocation;
- generated read-aloud CSS staging;
- stage/activity labels;
- dependency/process errors;
- final publication workflow integration.

### Lite lesson

Do not recreate one giant process-runtime module. Current Lite's `command`, runner/backend, scheduler/runtime coordinator, and stage-specific native modules are a better decomposition. The useful invariant is that external-process control remains below product/UI state.

## `alignment_report.rs`

Recovered validation strings show this was a compatibility/normalization layer over the external aligner's report rather than blindly trusting JSON.

It handled variants such as:

- `matchedRanges` / `matched_ranges` / `alignedRanges`
- `audioFiles` / `audio_files`
- `unalignedAudioFiles` / `unaligned_audio_files`
- multiple possible source/name/duration field forms

It rejected invalid/non-finite/negative timestamps, merged duplicate source records, warned about object-shape anomalies, and compared reported used audio with the processed-audio workspace.

### Lite lesson

Current Lite owns its own transcript/alignment artifact formats, which is preferable. Preserve the principle: stage boundaries validate and normalize their artifacts before downstream work uses them.

## `integrity.rs`

This was an independent final EPUB audit/repair boundary.

Recovered checks/messages include:

- final EPUB is a readable ZIP;
- XML/XHTML paths/text are UTF-8;
- SMIL `<audio>` has `src` and `clipEnd`;
- `<text>` has `src`;
- audio/SMIL reference counts;
- narrowly repairing zero-length SMIL clips;
- emitting an audit summary.

The presence of `integrity.rs` separately from finishing code is important: the old product deliberately audited output independently after mutation.

### Lite lesson

Retain the current independent `Validate` stage. Do not merge structural audit into Build EPUB merely because the native builder is trusted.

## Mapping to the recovered Lite architecture

A sensible correspondence is:

```text
Old OneClick                  Recovered Lite direction
---------------------------   ---------------------------------
commands/*                    Slint callback/adaptor layer
services/queue.rs             JobQueue + runner + worker bridge
services/manual.rs            native Review Audio service/model
services/settings.rs          reduced app/runtime settings model
process_runtime.rs            command runner + backend + scheduler
alignment_report.rs           owned artifact parsers/validators
integrity.rs                  independent Validate stage
state.rs                      durable app/job/review state
Sigil helper                  native Rust EPUB modules
```

Do **not** use this mapping to reintroduce old architecture. It is mainly a check that the new Lite decomposition still gives queue, review, persistence, processing, and audit distinct owners.

## Recovery confidence

High confidence:

- module filenames;
- command/service separation;
- broad responsibilities above, because nearby compiled error/activity strings and recovered IPC names support them.

Lower confidence:

- exact internal function names and call relationships not present in symbols/source paths;
- source module contents beyond recovered strings/disassembly.

When a detail matters to implementation, prefer the live Lite source and explicit current roadmap over inference from this module inventory.
