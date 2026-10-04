# Reliability rebuild — Whistle foundation

The rebuild keeps StoryTeller’s EPUB plus audiobook workflow, native Rust and Slint, and the seven visible stages. Whistle replaces Whisper completely. There is one supported transcription engine and no Whisper fallback.

## Boundaries

| Layer | Responsibility | Dependencies |
|---|---|---|
| `storyteller-core` | Job rules, queue, cancellation, resume fingerprints, normalized transcripts, conservative alignment, review decisions, EPUB construction and validation | Rust libraries; no Slint |
| `storyteller-application` | Runtime discovery/acquisition, isolated Whistle processes, FFmpeg conversion, chunk orchestration, stage execution and workspace paths | Core; no Slint |
| `storyteller-ui` | File selection, queue presentation, review interaction and application event polling | Application and core |

The first milestone moves the existing pipeline implementation behind the application boundary. Queue ownership and some review operations still live in the UI bridge; their migration is an explicit later milestone, not an architectural claim already completed.

## Whistle contract

- Model: `whistle.cact`, 16.9 MB, native CPU inference through the Needle 3.1.0 runtime.
- Audio: FFmpeg converts each window into 16 kHz mono PCM WAV. Plan targets 25 seconds, prefers nearby chapters and silence, and enforces the engine’s 30-second hard limit, including the final remainder.
- Supported languages: English, German, French, Spanish, Italian, Dutch and Polish. Auto detection is the default. Explicit unsupported languages fail before inference.
- Concurrency: 1–4 independently owned native processes, default 1. Process isolation respects the engine’s global, non-thread-safe model state.
- Timing: native word times are checked for text coverage, finite ordered ranges and valid confidence. Adjacent words become nonoverlapping phrases for the existing conservative alignment engine. Overlapping attention intervals remain together. The product remains phrase-level synchronization.
- Silence: empty native results are valid within a book. An entirely empty book does not yield a fabricated transcript or a successful analysis.
- Progress: completed audio duration determines Analyze progress; no simulated percentages or inferred decoder progress.
- Identity: engine and model contents enter Analyze fingerprints. Recovery schema 2 migrates schema-1 Whisper jobs to Whistle and discards Analyze and later checkpoints. Restored work remains paused.

## Rebuild milestones

| Milestone | State | Acceptance |
|---|---|---|
| R0 — Preserve contracts and establish application boundary | Validated on Windows | Existing queue, review, recovery and EPUB regressions pass; Slint compiles |
| R1 — Replace Whisper with Whistle | Validated on Windows | Verified runtime/model acquisition; native short and multi-window speech; silence rejection; timing survives EPUB construction |
| R2 — Own lifecycle in the application | Planned | One command/event interface owns start, pause, cancel, review and resume; UI cannot bypass transition rules |
| R3 — Make stage artifacts and publication explicit | Planned | Typed stage outputs, atomic candidate promotion, crash tests across validation/publication boundaries |
| R4 — Reduce UI and review coupling | Planned | UI consumes snapshots, application owns durable decisions and lazy evidence; 820×620 behavior preserved |
| R5 — Validate complete books and release | Planned | Representative long audiobooks in supported languages, difficult speech cuts, accuracy/timing review, reader interoperability and full Windows packaging |

Native smoke tests establish integration correctness, not a quality or speed advantage over the released large-v3-turbo implementation. Cactus’s published M4 Pro comparison against Whisper base is not a Windows audiobook benchmark. R5 must measure representative books before release.

## Validation

The Windows checkpoint [37183579467](https://github.com/skypie0102/storyteller-lite/actions/runs/37183579467) passed on product-source commit `57e85e85a019e821d374ca63159a75fa6e648b5b`:

- Formatting and strict workspace Clippy with all targets and the committed lockfile.
- All 116 workspace tests, including legacy recovery migration and Whistle timing through EPUB validation.
- Native Slint build and packaged Running/NeedsReview relaunch recovery.
- Hash-verified model, Windows native engine and FFmpeg acquisition.
- Actual English speech with native word timestamps and empty native silence.
- Application runtime discovery/verification, 6.44-second speech, and 55-second audio merged from three windows with two isolated workers.
- Global timestamp ordering/source bounds, complete hard-capped chunk coverage, entirely silent book rejection and temporary PCM cleanup.

The temporary branch-only checkpoint workflow was removed after this pass. Subsequent edits record this evidence and remove validation scaffolding; they do not change tested product source. Normal hosted validation remains opt-in under [CI_POLICY.md](CI_POLICY.md).

Full-book recognition accuracy, difficult speech boundaries, multilingual timing and performance are still R5 acceptance work. The integration pass does not establish those outcomes.

```text
cargo fmt --all -- --check
cargo clippy -p storyteller-core -p storyteller-application --all-targets -- -D warnings
cargo test --locked -p storyteller-core -p storyteller-application
```

At the Windows checkpoint, additionally run strict workspace Clippy, all workspace tests, the native Slint build, packaged relaunch recovery, and `.github/scripts/whistle-smoke.ps1` with verified assets. The script exercises the same adapter as Analyze through `transcribe_whistle`.

References: [Whistle announcement](https://cactuscompute.com/blog/whistle), [model card](https://huggingface.co/Cactus-Compute/whistle), [supported native devices](https://cactuscompute.com/blog/needle-supported-devices), [reference wrapper](https://github.com/cactus-compute/needle/blob/main/needle/agent/whistle.py).
