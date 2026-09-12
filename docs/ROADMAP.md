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
- `docs/recovery/README.md` — authority order and provenance index.

The planning transcript references a historical `refactor/rust-slint` branch and commit SHAs. The recovered live code branch is `recovery/rust-slint`; never assume a historical transcript claim is present without checking the repository.

## Product contracts

- Queue-first workflow. The first waiting book starts automatically; later books wait their turn.
- Exactly one pipeline worker may own foreground book processing at a time.
- `Pause after book` takes effect on the current job's terminal transition and prevents the next waiting job from starting until the queue is resumed.
- Long-running work never runs on the Slint UI thread.
- Exactly one weighted overall progress bar is shown.
- Progress, elapsed audio, ETA, speed, backend, model, and match values are displayed only when backed by real measurements or backend callbacks.
- The normal Details view is structured. Raw stdout/stderr belongs to diagnostics, not the primary progress UI.
- Source files are never intentionally overwritten. Output is written alongside the source EPUB by default.
- Retry/resume may reuse only a validated contiguous prefix of stage checkpoints. A stale stage invalidates itself and all downstream stages.
- An unimplemented or unsupported boundary must fail explicitly. It must never be represented as completed or skipped just to make the pipeline look finished.
- Human review decisions must be explicit and durable.
- Publication occurs only after the candidate EPUB passes an independent structural audit and the runner has accepted the Validate stage artifacts.

## Current Lite scope decisions

### Keep / restore

- Fixed seven-stage Lite pipeline.
- Queue-first processing and pause-after-current.
- Structured real progress/metrics.
- Native Rust + Slint application architecture.
- Existing automatic CPU-thread selection for Whisper.
- **One simple Whisper worker-count setting**, default `1`.
- **Reduced manual audio allocation** for unresolved/unaligned segments.
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
- Permanent OCR controls.
- Full historical unmatched-audio editor/classification surface.

The recovered historical plan once listed worker/thread controls as removable. The newer explicit user decision restores **worker count only**; manual CPU allocation remains out of scope.

## Pipeline

The user-facing stage order is fixed:

1. **Prepare** — validate and fingerprint inputs, create the per-job workspace, copy the source EPUB, and hard-link/copy the audiobook.
2. **Analyze** — extract EPUB reading-order text, normalize audio for speech recognition, and generate a timestamped Whisper transcript.
3. **Align** — align transcript/audio timing against the EPUB reading text and derive real match/confidence information.
4. **Review Audio** — surface unresolved or ambiguous alignment regions that genuinely require user review.
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
- Queue auto-advance after the previous worker has been joined.
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

Current recovered implementation uses all available logical CPU threads with whisper.cpp `-t` and does not expose/pass a user worker/processor count. Adding the worker-count setting is immediate pending work.

### Align

Implemented with the conservative `monotonic-ngram-edit-v2-block-safe` engine. Whisper segment timestamps remain the timing authority. Strong n-gram anchors and token edit similarity are used to accept monotonic book-text matches; weak evidence stays explicitly unmatched. Accepted matches cannot cross normalized XHTML block boundaries. The stage checkpoints `alignment.json` and publishes actual processed-segment and match metrics.

### Review Audio

Partially implemented relative to the recovered Lite product intent. `review.json` contains the real unmatched transcript segments and audio ranges. If there are no unmatched segments, processing continues automatically. Otherwise the job enters `NeedsReview`.

**Current limitation:** the UI only previews unmatched regions and offers Cancel or global **Continue without unmatched audio**. Continuing records a global exclusion decision.

**Pending requirement:** replace that all-or-nothing path with a reduced manual allocator that stores durable per-segment decisions. Minimum decisions are Pending, manually Assigned to valid EPUB block/range, and Explicitly Excluded. The allocator should provide audio preview/seek, transcript/timing, EPUB candidate context, previous/next unresolved navigation, and Apply & Next. Candidate assignments should preserve monotonic EPUB order and be constrained by neighboring accepted matches where possible.

Do not initially restore the full old editor/classification surface (split/merge, trim editor, permanent OCR, apply-to-similar, or the complete Introduction/Credits/Graphic Readout/Extra Audio taxonomy) unless a concrete Lite requirement emerges.

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

### P1 — Whisper worker-count setting

Add one simple user setting for whisper.cpp processors/workers.

Requirements:

- default `1`;
- exposed in Settings without reintroducing manual CPU allocation;
- positive validated/clamped range appropriate for the detected machine/runtime;
- wire to the exact current whisper.cpp CLI processor option (expected `-p`) only after checking the bundled/current CLI semantics;
- keep current automatic CPU-thread selection;
- avoid obvious CPU/GPU oversubscription after validating the exact interaction between `-t` and `-p` in the shipped build;
- keep worker count out of semantic output/checkpoint fingerprints unless evidence shows it changes transcript semantics.

Preferred architecture: app/runtime execution settings separate from output-affecting `JobSettings`. If queued-job determinism requires capturing the value per job, still exclude it from semantic stage fingerprints.

### P2 — reduced manual allocator

Create the per-segment review data model and Slint review experience described above and in `docs/ui-guides/storyteller-lite-manual-allocation.webp`.

Suggested implementation shape:

1. Extend/replace the current global review artifact with durable per-unmatched-segment decisions.
2. Load EPUB corpus context and neighboring accepted alignment positions for candidate generation.
3. Validate manual assignments for chronology/monotonicity and existing XHTML block boundaries.
4. Materialize an effective alignment used by downstream Build EPUB while retaining the original automatic alignment for audit/debugging.
5. Update NeedsReview resume behavior so processing continues only when every required unresolved segment has an explicit decision.
6. Build the reduced allocator UI: audio preview, transcript, candidate context, assign/exclude, previous/next, Apply & Next.
7. Add regression tests for assignment ordering, exclusion, resume durability, invalid/cross-boundary decisions, and mixed automatic/manual alignment.

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

## Failure policy still to finalize

The historical v1 behavior for whether a failed book should automatically advance to the next waiting book could not be recovered after the original repositories became unavailable. The current bridge advances after a terminal worker unless the queue is paused. Revisit this policy only when a trustworthy behavioral reference or an explicit product decision is available.
