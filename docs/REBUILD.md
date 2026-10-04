# Reliability rebuild — English transcription

The rebuild keeps StoryTeller’s EPUB plus audiobook workflow, native Rust and Slint, and the seven visible stages. The validated Whistle rebuild is merged into `main` through PR #30. Whistle remains the default; the user subsequently authorized an explicit optional Whisper NVIDIA GPU backend. Both integrations force English, with no automatic backend fallback.

## Boundaries

| Layer | Responsibility | Dependencies |
|---|---|---|
| `storyteller-core` | Job rules, queue, cancellation, resume fingerprints, normalized transcripts, conservative alignment, review decisions, EPUB construction and validation | Rust libraries; no Slint |
| `storyteller-application` | Private queue, lifecycle commands, workers, recovery, durable review decisions, runtime discovery/acquisition, isolated native transcription processes, FFmpeg conversion, chunk orchestration and stage execution | Core; no Slint |
| `storyteller-ui` | File selection, snapshot presentation and application command dispatch | Application and core |

The application controller now owns queue transitions, processing workers and recovery. The desktop shell dispatches commands and reads borrowed snapshots; it has no mutable queue handle. Review report/evidence loading, cached navigation and audio preview now have application-owned workers. The rebuilt native interface and validated Windows scenes are described in [UI.md](UI.md).

## Application controller scaffold

`ApplicationController` provides one command interface for enqueue, pause after the current book, resume, cancel, reorder, remove, retry from scratch, finish review, explicit pending-audio exclusion, individual review decisions and dependency setup. It owns the processing worker and applies final results before permitting conflicting commands for that book. A newly queued book remains Waiting until the previous processing thread has been joined, even if a terminal snapshot has already arrived. Cancellation also survives a worker yielding review before its thread exits.

- Startup recovery loads before any new enqueue command. Recovered books remain paused.
- Explicit commands and worker completion save recovery immediately; progress saves are dirty-only and throttled to one second. Idle polling performs no recovery writes.
- Worker startup failure marks that book failed and advances to the next waiting book on the next poll. Pause-after-current still takes precedence.
- Closing saves interrupted work before requesting cancellation and joining the processing worker. That saved work restores as Waiting; closing is distinct from an explicit Cancel command.
- Durable text/image/exclusion/edge decisions and review completion checks execute in the application. Stale job IDs cannot change another book's review.
- Dependency scans and downloads run on an application-owned background thread, keeping model hashing and probing for Settings off the UI thread.
- Borrowed snapshots have separate queue, runtime and review revisions. Unchanged polls do not rebuild the main view or re-read review reports. Queue/stage/candidate models update changed rows; progress and seeking no longer reset the whole list model. Submission requires both source selections and ready local tools.

Controller regressions exercise lifecycle, failure continuation, paused recovery, malformed-state preservation, durable review gates, real core worker handoff/cancellation and asynchronous runtime-result handling. R4 adds displayed-job cancellation, stale evidence, cached seeking, durable review reload and real child-process lifetime regressions. The controller and connected Slint shell passed the native Windows checkpoints below. The superseded UI recovery module was removed only after its replacement regressions passed in the application crate.

## Stage artifacts and publication

`StageRunOutput` carries a `StageArtifacts` variant for its actual stage. Required roles are explicit: source EPUB/audiobook; corpus/plan/transcript; alignment; review report; encoded audio/descriptor; candidate/effective alignment; and validation report. The runner rejects a stage mismatch or a review request outside Review Audio before finalization and checkpointing.

Stage completion writes a version-2 JSON `.artifacts` manifest with typed output roles, byte lengths and SHA-256 contents. The full manifest is flushed and atomically replaced only after every required nonempty file has been inspected. Resume rehashes the contiguous reusable prefix with cancellation checks, rejects missing/changed/conflicting outputs and invalid or old line-list manifests, and rewinds from the first invalid stage. The new format intentionally rebuilds older unsealed work rather than assigning unverified hashes to it. Review completion seals the edited report before continuing, so durable manual decisions remain reusable.

`ValidatedEpub` can only be created by independent structural validation with matching content hashes before and after the audit. Publication consumes that proof, streams and hashes a uniquely owned sibling staging file, flushes it, records a job-owned `publication.json` intent, and commits complete bytes without overwriting an existing path. Windows uses a same-directory `MoveFileExW` rename without the replace/cross-volume-copy flags; non-Windows uses an exclusive hard link and fails safely on filesystems without support. No new runtime dependency is introduced.

Validate always reruns on resume. An existing output is accepted only when its bytes and destination match the current validated candidate and the saved job-owned intent. A missing output is republished; a modified or unrelated output is preserved and fails validation/publication. Cancellation before commit leaves no output, while cancellation after commit reports success. A force termination can leave a unique staging file; later attempts neither trust it nor delete another attempt's staging name.

The failure-boundary regressions inject interruptions before/after intent and after commit; they are application/process-boundary checks, not a storage-device power-loss simulation. A real builder/validator/worker-resume fixture additionally verifies that all seven cached checkpoints cannot bypass external publication checks. This R3 implementation passed the native Windows checkpoint recorded below.

## Whistle contract

- Model: `whistle.cact`, 16.9 MB, native CPU inference through the Needle 3.1.0 runtime.
- Audio: FFmpeg converts each window into 16 kHz mono PCM WAV. Plan targets 25 seconds, prefers nearby chapters and silence, and enforces the engine’s 30-second hard limit, including the final remainder.
- Language: English only. Every invocation explicitly requests `--audio-language en`; the adapter exposes no language selector or automatic detection. New explicit non-English requests fail before inference.
- Concurrency: Automatic uses a background CPU/available-RAM scan to recommend a concrete count for newly queued books; manual selection supports 1–16 independently owned native processes. Saved counts remain stable across recovery. Process isolation respects the engine’s global, non-thread-safe model state. The recommendation is a starting heuristic, not a throughput benchmark; [RUNTIME.md](RUNTIME.md) defines the policy.
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
| R2 — Own lifecycle in the application | Validated on Windows | One command/snapshot interface owns start, pause, cancel, review and resume; UI has no mutable queue handle |
| R3 — Make stage artifacts and publication explicit | Validated on Windows | Typed stage outputs, atomic candidate promotion, crash tests across validation/publication boundaries |
| R4 — Rebuild the native UI and reduce review coupling | Validated on Windows | New book/Queue/Settings workspaces, output selection, readable review layout, background evidence/preview, stale-result guards, keyboard/pointer checks and 39 compact/standard/high-DPI scenes |
| R5 — Validate complete books and release | In progress | Representative long English audiobooks, difficult speech cuts, accuracy/timing review, reader interoperability and full Windows packaging |

Native smoke tests establish integration correctness, not a quality or speed advantage over the released large-v3-turbo implementation. Cactus’s published M4 Pro comparison against Whisper base is not a Windows audiobook benchmark. R5 must measure representative books before release.

## Validation

The automatic-worker checkpoint [37203054403](https://github.com/skypie0102/storyteller-lite/actions/runs/37203054403) passed on source commit `e59482318b661621309179f918dc5056ad48c912`:

- Formatting, strict locked workspace Clippy with all targets, all **171 Windows workspace tests**, native Slint build, 39 scene renders including 200% scaling, keyboard/pointer checks and packaged relaunch recovery.
- Seven CPU/RAM recommendation regressions covering scaling above four, low memory, missing probes, exact manual overrides, display text, Linux memory parsing and the real Windows memory probe. A recovery regression preserves concrete worker counts 1, 4, 8 and 16.
- Hash-verified unchanged Whistle/Needle/FFmpeg assets, 14 native timed English words and empty native silence. Automatic short-input selection passed. Explicit eight-worker selection transcribed **135 seconds across six chunks with six native workers**, exercising concurrency above four.
- Global timing order/source bounds, complete contiguous chunk coverage with every input at most 30 seconds, silent-book rejection and temporary PCM cleanup.
- Six updated Settings scenes were visually inspected at compact, standard and 200% scale; all 33 other scene files matched the earlier inspected UI checkpoint byte for byte. Updated screenshots and hashes are retained in [UI.md](UI.md).

Automatic leaves CPU/RAM headroom and is a starting heuristic. This checkpoint verifies integration and recovery, not optimal throughput, full-book speech-boundary accuracy or GPU transcription. No dependency or lockfile changes were needed.

The native UI checkpoint [37199779738](https://github.com/skypie0102/storyteller-lite/actions/runs/37199779738) passed on source commit `47e74e944fbdcd691c2956050ed26ae4c961f51f`:

- Formatting, strict locked workspace Clippy with all targets, all **164 Windows workspace tests**, the native Slint build and packaged Running/NeedsReview relaunch recovery.
- Ten additional regressions: displayed-job cancellation cannot cancel another active book; stale job/generation evidence is ignored; cached seeking and idle polling do not reload; durable decisions reload and advance; broken review reports cannot enable completion and can recover after repair; preview natural exit, stop/shutdown/reaping and startup failure; the preview child fixture; and custom output-folder selection.
- Thirteen actual compiled Slint scenes at 820×620 and 1040×760, plus compact renders at 200% scale: **39 screenshots**, all inspected. The compact processing view shows all seven stages, and compact review keeps the first match/action beside the transcript. Settings, queue and long status details use local scrolling.
- Real dispatched Tab/Space and pointer events at both normal sizes verify source selection, incomplete/busy start guards, ready submission, Settings/New book navigation, unresolved-review completion gating and the two-click bulk exclusion path. The harness installs fixture callbacks rather than invoking processing, downloads or file dialogs.
- Five representative lossless PNG renders and all 39 scene hashes were retained in the UI guide. Its screenshots and provenance now track the later automatic-worker checkpoint above. This earlier artifact ZIP SHA-256 is `9630442c1bde3cccb7d80219c50199489130d09714f18e0851d299a77e17ddb6`.

This checkpoint changes UI and application review ownership; it does not change the pinned Whistle adapter/assets. The R3 native speech/silence and multi-window integration evidence below still describes that adapter. Preview child-process tests establish lifecycle behavior; they do not establish audible-device quality. No runtime dependencies or lockfile changes were needed.

The stage artifact/publication checkpoint [37192046677](https://github.com/skypie0102/storyteller-lite/actions/runs/37192046677) passed on product-source commit `9ec8847b16416466b3bbfd83f6081c74ea2351c7`:

- Formatting, strict locked workspace Clippy, all **154 Windows workspace tests**, and the native Slint build. No dependency or lockfile changes.
- Typed-output mismatch and cancelled-capture gates; same-size cache edits; missing required roles; incomplete/conflicting/old/truncated manifests; conservative contiguous rewind; review resealing; and mandatory publication revalidation after resume.
- Eleven publication regressions covering interruption before/after intent and after commit, competing outputs, missing ownership intent, changed bytes, cancellation around commit, invalid intent preservation, abandoned staging names and aliased paths.
- Real EPUB builder/structural validator plus worker resume with all seven checkpoint stages present. A matching committed output is recovered without rewriting, a missing output is republished, and a changed output fails without overwrite.
- Hash-verified pinned assets, 14 native timed English speech words and empty native silence, 6.44-second and 55-second application transcription (the latter with three windows/two isolated workers), timestamp/chunk bounds, all-silence rejection and PCM cleanup.
- Packaged Running/NeedsReview relaunch recovery and absent-language migration, with all restored work remaining paused.

The Windows count includes eight artifact-manifest regressions. The additional Unix-only symbolic-link regression is implemented but was not run by this Windows checkpoint; Linux-specific filesystem coverage remains separate from the Windows acceptance claim. The boundary tests are injected application interruptions, not hardware power-loss tests.

The application controller and first UI optimization checkpoint [37189418973](https://github.com/skypie0102/storyteller-lite/actions/runs/37189418973) passed on product-source commit `24f128886f24cd8bf9cefa83b3a3e8b020c78c3e`:

- Formatting and strict workspace Clippy with all targets and the committed lockfile.
- All 135 workspace tests, including 14 controller lifecycle/recovery/review/background-task regressions, rejection of non-English requests/results, mixed-language recovery and Whistle timing through EPUB validation.
- Deterministic held-thread regressions prove that enqueue/resume cannot start a new book before the previous worker is joined, and cancellation is retained when a worker yields review before exiting.
- Native Slint build and packaged Running/NeedsReview relaunch recovery, including absent-language migration to English.
- Hash-verified model, Windows native engine and FFmpeg acquisition.
- Actual English speech with 14 native timed words and empty native silence, both explicitly requesting English.
- Application runtime discovery/verification, English-only 6.44-second speech, and 55-second audio merged from three windows with two isolated workers.
- Global timestamp ordering/source bounds, complete hard-capped chunk coverage, entirely silent book rejection and temporary PCM cleanup.

The first controller checkpoint [37189026768](https://github.com/skypie0102/storyteller-lite/actions/runs/37189026768) passed 133 tests on `e7a87f9fc1b4fb6e2f89e0b429e7e4dfa2f2f9c7`; the final checkpoint adds the worker-handoff guard and two race regressions. The English-only checkpoint [37186176210](https://github.com/skypie0102/storyteller-lite/actions/runs/37186176210) passed 121 tests on `df1e49faece11e28a0cb11bb0c79a5a91defea43`, and the initial foundation checkpoint [37183579467](https://github.com/skypie0102/storyteller-lite/actions/runs/37183579467) passed 116 tests on `57e85e85a019e821d374ca63159a75fa6e648b5b`.

Temporary branch-only checkpoint workflows are removed after their passes. The automatic-worker evidence/cleanup commit only updated documentation/screenshots and removed its workflow, with compiled source unchanged from `e59482318b661621309179f918dc5056ad48c912`. Earlier controller cleanup also removed the unreferenced legacy UI recovery file. Normal hosted validation remains opt-in under [CI_POLICY.md](CI_POLICY.md).

Full-book English recognition accuracy, difficult speech boundaries and performance are still R5 acceptance work. The integration pass does not establish those outcomes.

```text
cargo fmt --all -- --check
cargo clippy -p storyteller-core -p storyteller-application --all-targets -- -D warnings
cargo test --locked -p storyteller-core -p storyteller-application
```

At the Windows checkpoint, additionally run strict workspace Clippy, all workspace tests, the native Slint build, packaged relaunch recovery, and `.github/scripts/whistle-smoke.ps1` with verified assets. The script exercises the same adapter as Analyze through `transcribe_whistle`.

References: [Whistle announcement](https://cactuscompute.com/blog/whistle), [model card](https://huggingface.co/Cactus-Compute/whistle), [supported native devices](https://cactuscompute.com/blog/needle-supported-devices), [reference wrapper](https://github.com/cactus-compute/needle/blob/main/needle/agent/whistle.py).

## Optional Whisper GPU follow-up

[PR #31](https://github.com/skypie0102/storyteller-lite/pull/31) merged this implementation and dark/light appearance into `main` at `afb28e5d619a7be2712df51a99797cf02f3dffef`, following the Whistle rebuild merge `e04853cb26475a6aa94d8b5c658713473f7e3e7a`. The optional backend uses pinned whisper.cpp CUDA 11.8 Windows x64 binaries and the 574 MB Turbo Q5 model, one GPU worker, English native JSON and the shared bounded chapter/silence-aware chunk pipeline. Backend/model identity is preserved in schema-3 recovery and Analyze/Align/Review cache identity. Existing Whistle settings and schema-2 recovery remain compatible. Setup and book readiness follow the selected backend; queue resume follows the next book's saved backend.

GPU availability/free VRAM checks are a conservative startup policy. Actual invocations must show CUDA backend initialization plus model weights on CUDA and must not report failed initialization or CPU fallback. Real-GPU acceptance and throughput measurements remain outstanding; the CPU-hosted native validation helper checks the ABI/JSON adapter and rejects unconfirmed GPU execution. Model residency across chunks and non-NVIDIA runtime packages are not implemented in this milestone.

The implementation is available in [merged PR #31](https://github.com/skypie0102/storyteller-lite/pull/31). Windows checkpoint [37211560379](https://github.com/skypie0102/storyteller-lite/actions/runs/37211560379) tested source `8fb11d4e0172541e7818c5c46fde5731f129aa5d`:

- Strict locked workspace Clippy, all **183 tests** (47 application, 122 core, eight integration and six UI), and the native Slint build passed.
- All **48 compiled Slint scenes** and real keyboard/pointer checks passed at compact/standard sizes and 200% scale. All 48 captures match the previously inspected native capture byte for byte. Seven representative PNGs and hashes are retained in [UI.md](UI.md).
- The pinned CUDA archive, all 18 supported executable/DLL hashes and the actual Turbo Q5 model checksum passed. Native Turbo inference on the CPU-only runner produced two English timed segments; the product GPU adapter stopped no-GPU fallback and removed temporary WAV/JSON files.
- Whistle native/application speech and silence checks passed. Automatic recommended two workers on the four-thread runner; 135 seconds used six real native workers for six chunks, retaining full coverage/global timing and cleaning temporary PCM.
- Packaged relaunch restored interrupted work paused, retained Whisper's saved GPU backend/model/one-worker setting, and preserved existing Whistle recovery behavior.

The overall run failed **only** formatting: Rust 1.99 required a multiline expression in one controller test after boxing the runtime result. A formatting-only correction at `9190f50c215d0ed29579a4a244abb6fda428dbea` passed [formatting run 37215449790](https://github.com/skypie0102/storyteller-lite/actions/runs/37215449790). The functional gates were not repeated for this formatting-only change, in accordance with CI policy. The final evidence commit updates docs/screenshots and removes the temporary workflow; compiled source remains unchanged from that formatting commit. That checkpoint did not change Cargo dependencies/lockfile or publish a release. PR #31 is now merged; real-GPU and full-book release acceptance remains open.

## Dark-default appearance follow-up

Dark is now the default regardless of system appearance, with a live Light option in Settings → Appearance → Theme. Both the custom palette and native Slint controls switch together. The preference is written to the per-user `appearance.json` using sibling-file replacement and restored before the window opens; missing or unreadable settings use Dark. It is independent of job/recovery/cache settings.

Windows checkpoint [37216454960](https://github.com/skypie0102/storyteller-lite/actions/runs/37216454960) passed on `855a086fbbb9ee88901d884fa294e2cf2133b412`: formatting, strict locked UI Clippy/all targets, all six UI tests, the native build and **96 native scenes** covering sixteen states in both themes at 820×620, 1040×760 and compact 200% scale. Existing pointer/keyboard flows passed in both themes. Additional native input checks selected Light/Dark, verified immediate rendered canvas updates, reloaded both saved choices in fresh windows and verified Dark fallback for unreadable preferences at both normal sizes. All 96 scenes were inspected; nine PNGs and complete hashes are retained in [UI.md](UI.md).

The final evidence/cleanup changes only docs/screenshots and removes the temporary workflow. Compiled source remains the checkpoint source, with no dependency, lockfile or processing changes. The earlier 183-test/native Whistle/Whisper checkpoint still supplies processing evidence; this UI-only follow-up did not repeat ASR downloads or full-book/GPU acceptance.

## Next work: R5 long-book hardening and release acceptance

1. Reduce repeated Whisper model loading with a bounded number of independent files per invocation. Keep every input at or below 30 seconds, one GPU context and bounded temporary PCM. Validate file-to-output pairing, local/global timing, missing outputs, cancellation and rejected CPU fallback using the pinned native CLI. This does not establish a measured speedup or full-book model residency.
2. Protect speech around difficult chapter/synthetic cuts with context overlap and boundary-word reconciliation. Validate crossing words and repeated phrases without dropping or duplicating narration.
3. Process representative multi-hour English audiobooks with both backends. Inspect recognition/alignment, resume/cancellation, tail coverage and final EPUB playback; record elapsed time, peak RAM/VRAM and hardware. Real NVIDIA offload/performance is a separate required acceptance check.
4. Build and validate a rebuilt Windows package, repeat reader interoperability with the new exports, then prepare the next release. The published v0.1.0 is the historical build.
