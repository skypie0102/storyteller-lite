# Storyteller Lite roadmap

> **Canonical recovery roadmap.** Read `docs/HANDOFF.md` before starting substantial work. The live Rust + Slint source on `recovery/rust-slint` is technical truth; this document is the current product/pending-work truth.

Storyteller Lite is the Rust + Slint successor to Storyteller OneClick. The original pre-Rust application remains a behavioral reference where its behavior is useful, but Lite is intentionally smaller and is not intended to be a line-for-line port.

## Recovery references

The recovery packet is stored in the repository so another agent can continue without this chat:

- `docs/HANDOFF.md` — first-read agent handoff and immediate implementation order.
- `docs/ui-guides/storyteller-lite-main-ui.webp` — current main-screen layout reference.
- `docs/ui-guides/storyteller-lite-manual-allocation.webp` — current manual-allocation layout reference.
- `docs/ui-guides/README.md` — mockup interpretation and Lite trimming rules.
- `docs/recovery/LITE_PLANNING_HISTORY.txt` — recovered planning/implementation transcript.
- `docs/recovery/LEGACY_INSTALLER_REFERENCE.md` — old installer provenance and behavioral-reference policy.
- `docs/recovery/INSTALLER_DISSECTION.md` — static installer findings.
- `docs/recovery/FRONTEND_AND_CONCURRENCY_RECOVERY.md` — recovered old frontend/state/persistence behavior.
- `docs/recovery/WORKER_SEMANTICS.md` — recovered meaning of Parallel Whisper jobs and preferred Lite direction.
- `docs/recovery/UNMATCHED_AUDIO_RECOVERY.md` — recovered Smart/edge/lazy-OCR behavior and allocator scope.
- `docs/recovery/QUEUE_FAILURE_RECOVERY.md` — recovered queue auto-advance behavior after failures.
- `docs/recovery/README.md` — authority order and provenance index.

The planning transcript references a historical `refactor/rust-slint` branch and commit SHAs. The recovered live code branch is `recovery/rust-slint`; never assume a historical transcript claim is present without checking the repository.

## Product contracts

- Queue-first workflow. The first waiting book starts automatically; later books wait their turn.
- Exactly one pipeline worker may own foreground book processing at a time.
- A failed book does not stall the whole queue: mark it failed, preserve retry/diagnostic state, and continue to the next waiting book unless the queue was explicitly paused or `Pause after book` was requested.
- `Pause after book` takes effect on the current job's terminal transition and prevents the next waiting job from starting until the queue is resumed.
- Long-running work never runs on the Slint UI thread.
- Exactly one weighted overall progress bar is shown.
- Progress, elapsed audio, ETA, speed, backend, model, and match values are displayed only when backed by real measurements or backend callbacks.
- The normal Details view is structured. Raw stdout/stderr belongs to diagnostics, not the primary progress UI.
- Source files are never intentionally overwritten. Output is written alongside the source EPUB by default.
- Retry/resume may reuse only a validated contiguous prefix of stage checkpoints. A stale stage invalidates itself and all downstream stages.
- An unimplemented or unsupported boundary must fail explicitly. It must never be represented as completed or skipped just to make the pipeline look finished.
- Human review decisions must be explicit and durable.
- Smart automatic decisions must be conservative, auditable, and reversible before publication.
- Publication occurs only after the candidate EPUB passes an independent structural audit and the runner has accepted the Validate stage artifacts.

## Current Lite scope decisions

### Keep / restore

- Fixed seven-stage Lite pipeline.
- Queue-first processing and pause-after-current.
- Structured real progress/metrics.
- Native Rust + Slint application architecture.
- Existing automatic CPU-thread selection for Whisper.
- **One simple Whisper worker-count setting**, default `1` unless deliberate benchmarking changes it.
- Worker count means **concurrent transcription chunks/tracks**, not manual CPU allocation and not a direct alias for whisper.cpp `-p`.
- **Smart / ReviewAll** unmatched-audio policy.
- **Automatic edge trimming/handling** where confidence and safety are high.
- **Lazy/on-demand OCR** for bounded candidate EPUB images when Smart classification or review needs it.
- **Reduced manual audio allocation** for unresolved/unaligned segments.
- Limited useful classifications/destinations such as Introduction, Credits, Graphic Readout, and Extra Audio when they materially help Smart/manual placement; do not expose the entire old editor taxonomy merely for parity.
- The supplied mockups as current visual hierarchy references.
- The old installer only as a behavioral regression/reference source when a Lite behavior is ambiguous.

### Explicitly not restored by default

- Manual CPU/thread allocation UI.
- Word-level synchronization.
- Activity/console-first primary UI.
- Runtime Health page.
- Process-now flow.
- Full engine/runtime path tuning UI.
- Standardize-EPUB toggle.
- CSS editor.
- Permanent OCR enable/disable controls.
- Full historical unmatched-audio editor surface: arbitrary split/merge, general waveform trimming, broad rule editor, and unrestricted old classification/destination UI.

The recovered historical plan once listed worker/thread controls as removable. The newer explicit user decision restores **worker count only**; manual CPU allocation remains out of scope. Likewise, removing the old permanent OCR toggle does **not** remove lazy OCR from the Smart review pipeline.

## Pipeline

The user-facing stage order is fixed:

1. **Prepare** — validate and fingerprint inputs, create the per-job workspace, copy the source EPUB, and hard-link/copy the audiobook.
2. **Analyze** — extract EPUB reading-order text, normalize audio for speech recognition, and generate a timestamped Whisper transcript.
3. **Align** — align transcript/audio timing against the EPUB reading text and derive real match/confidence information.
4. **Review Audio** — Smart-handle safe unmatched regions and surface unresolved or ambiguous regions that genuinely require user review.
5. **Encode** — create the final audio representation according to Copy / Opus / AAC and bitrate settings.
6. **Build EPUB** — construct the synchronized read-aloud EPUB without replacing the source.
7. **Validate** — independently audit the completed candidate and only then publish/complete the job.

## Current implementation status

### Complete foundations

- Rust workspace with UI-independent job, queue, progress, resume, scheduler, runner, and worker abstractions.
- Seven-stage weighted progress model.
- SHA-256 source fingerprint preflight with cancellation checks.
- Per-job and per-stage workspaces plus artifact manifests.
- Worker snapshot reconciliation through the queue.
- 100 ms Slint worker bridge.
- Queue auto-advance after terminal workers, including failed jobs, unless pause-after-current paused the queue. This matches the recovered v0.39.0 behavior.
- Pause-after-current, queue resume, and active worker cancellation controls.
- Queue-another-book flow while a job is active.
- Waiting/recent job management with reorder, remove, safe retry-from-start, and visible terminal errors.
- Cancellable external command runner with streamed stdout/stderr callbacks.
- Resume fingerprints for Whisper, alignment, audio encoder, and EPUB builder backend identity.
- Stage finalization hook that runs after artifact capture and before the stage can complete; final EPUB publication uses this path.

### Prepare

Implemented. EPUB is copied. Audiobook staging prefers a hard link and falls back to cancellable copying. Prepared sources are validated before later stages reopen them.

### Analyze

Implemented using native EPUB parsing plus external `ffmpeg` and `whisper.cpp` CLI tooling. The stage extracts a bounded/cancellable reading-order corpus, converts the audiobook to 16 kHz mono PCM, invokes Whisper with JSON-full output, publishes only Whisper's real progress callbacks, and checkpoints `book-corpus.json`, `audio.wav`, and `transcript.json`. Whisper segment timing must be positive, chronological, and non-overlapping.

Current recovered implementation uses all available logical CPU threads with whisper.cpp `-t`, converts the full book to one `audio.wav`, and launches one whisper-cli process. It therefore has only one transcription work item per book. Adding true worker-count support requires deterministic chunk/track work items; simply passing `-p N` would not restore the recovered worker semantics.

### Align

Implemented with the conservative `monotonic-ngram-edit-v2-block-safe` engine. Whisper segment timestamps remain the timing authority. Strong n-gram anchors and token edit similarity are used to accept monotonic book-text matches; weak evidence stays explicitly unmatched. Accepted matches cannot cross normalized XHTML block boundaries. The stage checkpoints `alignment.json` and publishes actual processed-segment and match metrics.

### Review Audio

Partially implemented relative to the recovered Lite product intent. `review.json` contains the real unmatched transcript segments and audio ranges. If there are no unmatched segments, processing continues automatically. Otherwise the job enters `NeedsReview`.

**Current limitation:** the UI only previews unmatched regions and offers Cancel or global **Continue without unmatched audio**. Continuing records a global exclusion decision. Smart edge handling, lazy OCR/classification, and durable per-segment assignment are not implemented in the current recovered code.

**Pending requirement:** implement the reduced Smart/manual review pipeline:

- default **Smart** policy should automatically resolve only high-confidence safe cases and leave ambiguous cases pending;
- **ReviewAll** should surface regions that Smart could otherwise resolve so the user can inspect/override them;
- perform automatic edge trimming/handling conservatively rather than silently throwing away meaningful narration;
- use lazy OCR only for bounded candidate EPUB images/documents when image text is actually needed;
- support high-confidence Graphic Readout → image/page placement where transcript + EPUB image text establish a safe match;
- preserve optional classifications such as Introduction, Credits, Graphic Readout, or Extra Audio where needed for destination/build behavior;
- persist every unresolved segment's explicit decision and autosave review drafts;
- provide audio preview/seek, transcript/timing/silence context, EPUB candidate context, previous/next unresolved navigation, explicit assignment/exclusion/override, and Apply & Next;
- preserve monotonic EPUB order and validate candidate destinations against neighboring accepted matches and real XHTML block boundaries;
- keep automatic/manual decisions auditable and validate the generated EPUB/Media Overlay afterward.

Do **not** initially restore arbitrary split/merge editing, a general trim editor, a permanent OCR toggle, the old broad Apply-to-similar rule system, or an unrestricted general-purpose allocator.

See `docs/recovery/UNMATCHED_AUDIO_RECOVERY.md` for recovered historical behavior and test-vector thresholds. The GPL/Sigil-derived helper is a behavioral reference, not code to copy wholesale.

### Encode

Implemented as whole-audiobook output. Copy mode performs a cancellable byte-preserving copy only when the source extension maps safely to an EPUB 3.3 core Media Overlay audio type. Opus and AAC use external `ffmpeg`; live processed-audio time is sourced from ffmpeg's machine-readable progress output. Generated Opus is declared `audio/ogg; codecs=opus`; AAC is `audio/mp4`. `encoded-audio.json` records the output filename, codec, bitrate, and EPUB media type alongside the encoded audio artifact.

### Build EPUB

Implemented for EPUB 3 source packages that do not already contain Media Overlays. The builder:

- preserves the source EPUB and writes a workspace candidate;
- copies unrelated ZIP resources through without unpacking the publication tree;
- writes `mimetype` first and uncompressed;
- maps accepted alignment lines to real XHTML block elements;
- reuses existing block IDs or injects deterministic collision-resistant `stl-mo-*` anchors;
- creates SMIL `<par>` entries with real Whisper segment clip times;
- embeds the encoded audiobook under a collision-free package-relative resource tree;
- adds `media-overlay` manifest associations, SMIL/audio manifest items, per-overlay `media:duration`, and total `media:duration` metadata;
- explicitly rejects EPUB 2 and already-synchronized source publications rather than guessing at a migration/merge.

Current synchronization granularity is block-level: multiple real transcript segments may legitimately reference the same XHTML block fragment.

### Validate

Implemented as an independent structural audit of the built candidate. It reopens the ZIP and verifies the mimetype ordering/compression/value, duplicate entries, package/Media Overlay links, SMIL media types, text-fragment existence, audio manifest targets, positive clip ranges, and refined/total Media Overlay duration consistency. It writes `validation.json`. Publication to `<title> (readaloud).epub` occurs in the runner finalization phase only after the validation artifact manifest has been captured. Existing output files are not overwritten.

An end-to-end core fixture builds a minimal EPUB with two synchronized blocks, validates it, publishes it once, and verifies that a second publication attempt is rejected.

## Immediate pending work

### P0 — establish the live baseline

Run the intended Windows validation path on the current `recovery/rust-slint` head before feature work. Historical planning transcript CI claims are not a substitute for validating the recovered branch that exists now.

### P1 — Whisper worker-count setting and chunked Analyze

Add one simple worker-count setting backed by **concurrently transcribed ordered chunks/tracks**.

Recovered evidence is decisive that old `Parallel Whisper jobs` was higher-level file/chunk concurrency while Whisper `processors` was a separate knob fixed to `1`. Current upstream Storyteller independently confirms this conceptual split. See `docs/recovery/WORKER_SEMANTICS.md`.

Requirements:

- default `1` unless deliberate benchmarking changes it;
- exposed in Settings without reintroducing manual CPU allocation;
- initial user range can reasonably benchmark 1–4, matching the old product's parallel-job range without treating it as a compatibility mandate;
- create deterministic ordered transcription work items for one long audiobook, preferring chapter-safe boundaries and a tested silence/VAD-aware fallback for overlong/no-chapter ranges;
- run at most `workers` Whisper tasks concurrently;
- keep each individual whisper.cpp invocation at one processor initially rather than mapping workers to `-p`;
- automatically budget per-worker CPU threads and GPU/VRAM resources instead of giving every simultaneous process all logical CPUs;
- convert local chunk timestamps back to global audiobook timestamps and merge to one deterministic transcript;
- validate merged chronology/non-overlap before publishing the Analyze artifact;
- aggregate progress and cancellation across all active workers;
- capture the worker setting per queued book if needed for deterministic execution, but keep it out of semantic output/checkpoint fingerprints unless evidence shows it changes transcript semantics.

Preferred architecture: app/runtime execution settings separate from output-affecting `JobSettings`. The UI stays simple even if the scheduler is hardware-aware internally.

### P2 — Smart unmatched-audio pipeline and reduced manual allocator

Implement the behavior described in the Review Audio section and `docs/recovery/UNMATCHED_AUDIO_RECOVERY.md`.

Suggested implementation shape:

1. Add a reduced `Smart / ReviewAll` policy without restoring the old four-mode selector.
2. Add native silence/edge analysis and conservative automatic edge handling.
3. Add bounded candidate generation from EPUB context and lazy image OCR/text hints only when needed.
4. Extend/replace the current global review artifact with durable per-segment decisions plus optional classification/suggestion/source metadata.
5. Constrain automatic/manual assignments by neighboring accepted alignment positions and actual XHTML/image candidates.
6. Materialize an effective alignment/allocation result for downstream Build EPUB while retaining original automatic alignment and decision provenance for audit/debugging.
7. Autosave decisions separately from rebuildable preview/workspace artifacts; app restart/retry should restore review work.
8. Continue only when every required unresolved region has an explicit validated disposition.
9. Build the reduced Slint allocator UI from the supplied mockup: preview, transcript/timing, Smart suggestion, candidate context, assign/exclude/override, previous/next, Apply & Next.
10. Add regression tests for ordering, exclusions, Smart/manual mixes, edge cases, OCR/image candidates, restart/retry durability, and invalid/cross-boundary decisions.

### P3 — main Slint UI alignment

Bring the current main experience closer to `docs/ui-guides/storyteller-lite-main-ui.webp`: compact create controls, one rich processing card, seven-stage visualization, real metrics, queue/recent management, and responsive reflow. Do not make the mockup a fixed pixel canvas.

### P4 — installer archaeology only when needed

If a behavior remains ambiguous, statically extract/inspect the supplied v0.39.0 installer after verifying the SHA-256 in `docs/recovery/LEGACY_INSTALLER_REFERENCE.md`. Record recovered behavior before implementing it. Do not revive the old architecture or broaden Lite scope just because a feature exists in the installer.

### P5 — later polish/compatibility

After P1–P3 are stable:

- explicit validated checkpoint-resume controls after persistence/relaunch behavior is defined;
- diagnostics and packaging/release/update polish;
- additional EPUB compatibility policy (existing Media Overlays, EPUB 2 if desired, remote resources, SVG/text alternatives, semantic/skippable structures);
- interoperability testing across reading systems and EPUBCheck when available.

## Engineering migration rule

Do not begin with broad legacy deletion. Preserve old material until the replacement behavior is implemented and covered by tests.

> **Replace → regression-test → delete. Never delete → hope we remembered everything.**

## Queue failure policy — recovered and resolved

The user-supplied v0.39.0 installer resolves the historical failure-policy question. Its compiled backend explicitly reports that the **queue continues after a processing failure when pending books remain**. The old frontend separately exposes `Stop After` as the user action that prevents automatic continuation after the current book.

The current Rust + Slint implementation already matches this: `WorkerBridge` attempts `start_next_worker()` after a terminal worker result, and `JobQueue` remains running across terminal transitions unless pause-after-current moves it to `Paused`.

Keep this behavior:

> **Failed book → preserve Failed/retry state → continue to the next waiting book.** Only an explicit queue pause / Pause-after-current prevents the next book from starting.

See `docs/recovery/QUEUE_FAILURE_RECOVERY.md` for the recovered evidence and regression-test expectations.
