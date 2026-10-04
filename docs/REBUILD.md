# Reliability rebuild — Whistle foundation

The rebuild keeps StoryTeller’s EPUB plus audiobook workflow, native Rust and Slint, and the seven visible stages. Whistle replaces Whisper completely. There is one supported transcription engine and no Whisper fallback.

## Boundaries

| Layer | Responsibility | Dependencies |
|---|---|---|
| `storyteller-core` | Job rules, queue, cancellation, resume fingerprints, normalized transcripts, conservative alignment, review decisions, EPUB construction and validation | Rust libraries; no Slint |
| `storyteller-application` | Private queue, lifecycle commands, workers, recovery, durable review decisions, runtime discovery/acquisition, isolated Whistle processes, FFmpeg conversion, chunk orchestration and stage execution | Core; no Slint |
| `storyteller-ui` | File selection, snapshot presentation, review selection/navigation and application command dispatch | Application and core |

The application controller now owns queue transitions, processing workers and recovery. The desktop shell dispatches commands and reads borrowed snapshots; it has no mutable queue handle. Review evidence/candidate discovery and audio preview still have UI-side implementation, with further migration and visual optimization tracked under R4.

## Application controller scaffold

`ApplicationController` provides one command interface for enqueue, pause after the current book, resume, cancel, reorder, remove, retry from scratch, finish review, explicit pending-audio exclusion, individual review decisions and dependency setup. It owns the processing worker and applies final results before permitting conflicting commands for that book. A newly queued book remains Waiting until the previous processing thread has been joined, even if a terminal snapshot has already arrived. Cancellation also survives a worker yielding review before its thread exits.

- Startup recovery loads before any new enqueue command. Recovered books remain paused.
- Explicit commands and worker completion save recovery immediately; progress saves are dirty-only and throttled to one second. Idle polling performs no recovery writes.
- Worker startup failure marks that book failed and advances to the next waiting book on the next poll. Pause-after-current still takes precedence.
- Closing saves interrupted work before requesting cancellation and joining the processing worker. That saved work restores as Waiting; closing is distinct from an explicit Cancel command.
- Durable text/image/exclusion/edge decisions and review completion checks execute in the application. Stale job IDs cannot change another book's review.
- Dependency scans and downloads run on an application-owned background thread, keeping model hashing and probing for Settings off the UI thread.
- Borrowed snapshots have separate queue and runtime revisions. Unchanged polls do not rebuild the main view or re-read review reports. Queue/stage models update changed rows; progress no longer resets the whole queue model. Queue buttons require both source selections.

Fourteen controller regressions exercise lifecycle, failure continuation, paused recovery, malformed-state preservation, durable review gates, real core worker handoff/cancellation and asynchronous runtime-result handling. Native Windows validation is pending for this scaffold; the earlier Whistle checkpoint below predates it.

## Whistle contract

- Model: `whistle.cact`, 16.9 MB, native CPU inference through the Needle 3.1.0 runtime.
- Audio: FFmpeg converts each window into 16 kHz mono PCM WAV. Plan targets 25 seconds, prefers nearby chapters and silence, and enforces the engine’s 30-second hard limit, including the final remainder.
- Language: English only. Every invocation explicitly requests `--audio-language en`; the adapter exposes no language selector or automatic detection. New explicit non-English requests fail before inference.
- Concurrency: 1–4 independently owned native processes, default 1. Process isolation respects the engine’s global, non-thread-safe model state.
- Timing: native word times are checked for text coverage, finite ordered ranges and valid confidence. Adjacent words become nonoverlapping phrases for the existing conservative alignment engine. Overlapping attention intervals remain together. The product remains phrase-level synchronization.
- Silence: empty native results are valid within a book. An entirely empty book does not yield a fabricated transcript or a successful analysis.
- Progress: completed audio duration determines Analyze progress; no simulated percentages or inferred decoder progress.
- Identity: engine/model contents and the English adapter profile enter Analyze fingerprints. Recovery schema 2 migrates schema-1 Whisper jobs to Whistle and discards Analyze and later checkpoints. Absent/auto languages become English. Explicit foreign requests remain readable, discard Analyze and later checkpoints, and fail worker preflight without invalidating other queued jobs. Restored work remains paused.

English-only mode removes application language options and detection routing. The published checkpoint stores shared multilingual weights in one file; there are no separately installed language packs to remove. The pinned model remains 16.9 MB, so this change does not claim a smaller model download, lower memory use, or faster inference. Reducing those requires a separately validated smaller checkpoint or runtime change.

## Rebuild milestones

| Milestone | State | Acceptance |
|---|---|---|
| R0 — Preserve contracts and establish application boundary | Validated on Windows | Existing queue, review, recovery and EPUB regressions pass; Slint compiles |
| R1 — Replace Whisper with Whistle | Validated on Windows | Verified runtime/model acquisition; native short and multi-window speech; silence rejection; timing survives EPUB construction |
| R2 — Own lifecycle in the application | Implemented; Windows validation pending | One command/snapshot interface owns start, pause, cancel, review and resume; UI has no mutable queue handle |
| R3 — Make stage artifacts and publication explicit | Planned | Typed stage outputs, atomic candidate promotion, crash tests across validation/publication boundaries |
| R4 — Reduce UI and review coupling | In progress | Snapshot rendering and durable decisions moved; remaining preview/evidence migration, layout/usability optimization and 820×620 visual acceptance |
| R5 — Validate complete books and release | Planned | Representative long English audiobooks, difficult speech cuts, accuracy/timing review, reader interoperability and full Windows packaging |

Native smoke tests establish integration correctness, not a quality or speed advantage over the released large-v3-turbo implementation. Cactus’s published M4 Pro comparison against Whisper base is not a Windows audiobook benchmark. R5 must measure representative books before release.

## Validation

The English-only Windows checkpoint [37186176210](https://github.com/skypie0102/storyteller-lite/actions/runs/37186176210) passed on product-source commit `df1e49faece11e28a0cb11bb0c79a5a91defea43`:

- Formatting and strict workspace Clippy with all targets and the committed lockfile.
- All 121 workspace tests, including rejection of non-English requests/results, mixed-language saved-queue recovery, legacy recovery migration and Whistle timing through EPUB validation.
- Native Slint build and packaged Running/NeedsReview relaunch recovery, including absent-language migration to English.
- Hash-verified model, Windows native engine and FFmpeg acquisition.
- Actual English speech with 14 native timed words and empty native silence, both explicitly requesting English.
- Application runtime discovery/verification, English-only 6.44-second speech, and 55-second audio merged from three windows with two isolated workers.
- Global timestamp ordering/source bounds, complete hard-capped chunk coverage, entirely silent book rejection and temporary PCM cleanup.

The earlier foundation checkpoint [37183579467](https://github.com/skypie0102/storyteller-lite/actions/runs/37183579467) passed 116 tests on `57e85e85a019e821d374ca63159a75fa6e648b5b`, before English-only support. The temporary branch-only checkpoint workflow was removed after the latest pass. Subsequent evidence/cleanup edits do not change tested product source. Normal hosted validation remains opt-in under [CI_POLICY.md](CI_POLICY.md).

Full-book English recognition accuracy, difficult speech boundaries and performance are still R5 acceptance work. The integration pass does not establish those outcomes.

```text
cargo fmt --all -- --check
cargo clippy -p storyteller-core -p storyteller-application --all-targets -- -D warnings
cargo test --locked -p storyteller-core -p storyteller-application
```

At the Windows checkpoint, additionally run strict workspace Clippy, all workspace tests, the native Slint build, packaged relaunch recovery, and `.github/scripts/whistle-smoke.ps1` with verified assets. The script exercises the same adapter as Analyze through `transcribe_whistle`.

References: [Whistle announcement](https://cactuscompute.com/blog/whistle), [model card](https://huggingface.co/Cactus-Compute/whistle), [supported native devices](https://cactuscompute.com/blog/needle-supported-devices), [reference wrapper](https://github.com/cactus-compute/needle/blob/main/needle/agent/whistle.py).
