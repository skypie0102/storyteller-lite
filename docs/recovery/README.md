# Recovery source index

This directory preserves the product/history material used to rebuild Storyteller OneClick Lite after the Rust + Slint branch was lost/recovered.

## Authority order

When sources disagree, use this order:

1. **Current source code and tests on `recovery/rust-slint`** — technical truth about what exists now.
2. **`docs/ROADMAP.md` and `docs/HANDOFF.md`** — current product decisions and pending work.
3. **`docs/ui-guides/` mockups and interpretation notes** — current visual/product layout references.
4. **Installer dissection notes in this directory** — recovered historical behavior/invariants from v0.39.0, interpreted through current Lite scope.
5. **`LITE_PLANNING_HISTORY.txt`** — historical planning/implementation transcript. It can contain claims about branches/commits that are no longer present; treat those as recovery clues, not proof of current code.
6. **Storyteller OneClick v0.39.0 installer itself** — behavioral reference only. It predates the Rust + Slint Lite rebuild and is not the Lite codebase.

## Preserved materials

### Lite planning transcript

- Repository file: `docs/recovery/LITE_PLANNING_HISTORY.txt`
- Source SHA-256: `8c3784ff4ebd7ec0b54993cc72bab54e11a019d6f10666d66d34d8e127856a0c`
- Purpose: preserve the recovered sequence of Lite scope decisions and prior implementation claims so another agent can understand the intended direction.

Important discrepancy: the transcript repeatedly refers to `refactor/rust-slint` and historical commit SHAs. The live repository branch recovered in this chat is `recovery/rust-slint`. Always inspect the current repository before assuming a historical commit still exists.

### UI mockups

See `docs/ui-guides/README.md` and the two WebP files stored next to it. They are the current layout references for the main screen and the reduced manual-audio-allocation experience.

### Old installer recovery

Start with:

- `LEGACY_INSTALLER_REFERENCE.md` — provenance, exact installer identity, and scope rules.
- `INSTALLER_DISSECTION.md` — NSIS payload structure, recovered desktop/helper payloads, and first-pass behavior clues.
- `FRONTEND_AND_CONCURRENCY_RECOVERY.md` — recovered Tauri frontend behavior, real old Settings defaults, queue snapshots, allocator autosave/state-machine behavior, and persistence architecture.
- `WORKER_SEMANTICS.md` — evidence that old Parallel Whisper jobs meant concurrent processed audio files/chunks, not whisper.cpp processor count, plus the preferred Lite worker architecture.
- `UNMATCHED_AUDIO_RECOVERY.md` — reconciles the Lite planning transcript, mockup, and old helper around Smart / ReviewAll, automatic edge handling, lazy OCR, Graphic Readout/image placement, and the deliberately reduced allocator surface.
- `ALLOCATOR_OUTPUT_RECOVERY.md` — downstream EPUB meaning of reviewed Introduction/Credits/Graphic Readout assignments, including supplemental XHTML+SMIL pages and image-bound narration.
- `ALLOCATOR_PREVIEW_RECOVERY.md` — shows that manual-review preview clips were disposable cache while original/global segment timing remained authoritative.
- `QUEUE_FAILURE_RECOVERY.md` — resolves the old failure policy: a failed book remains retryable and the queue advances to the next pending book unless explicitly stopped/paused.
- `PUBLICATION_RECOVERY.md` — recovered staged-audit and atomic publish/rollback semantics, plus how Lite deliberately simplifies them by refusing overwrite.
- `storyteller-readaloud-v039.css` — exact old built-in generated-page stylesheet preserved as a visual regression reference, not a mandatory Lite theme.

Reproducible static recovery tools live under `tools/recovery/`:

- `extract_legacy_nsis.py` — hash-locked extraction of the exact v0.39.0 NSIS payload.
- `extract_tauri_assets.py` — hash-locked recovery/verification of the embedded Tauri web assets from the old desktop executable. It requires Python's `brotli` package and is recovery-only tooling, not a Lite runtime dependency.

The old installer and desktop executable are intentionally not committed to this source repository. Their hashes in the recovery docs identify the exact reference samples.

## Key recovered distinctions

Do not lose these when implementing Lite:

- `parallelTranscribes` historically meant concurrent transcription **files/chunks**. It was separate from per-job CPU threads and separate from Whisper `processors`.
- The Lite plan removes the **permanent OCR toggle**, not lazy OCR itself. Smart classification, automatic edge handling, and bounded/on-demand OCR were part of the intended reduced pipeline.
- Manual allocator **decisions/drafts are durable state**; preview audio/images and temporary inspection workspaces are rebuildable artifacts.
- Review decisions imply downstream rendering semantics: normal text assignment, supplemental synchronized page, image-bound narration, or explicit exclusion are not interchangeable.
- The old failure policy is no longer unknown: failures auto-advanced while keeping the failed book retryable. Cancellation auto-advance remains less firmly proven historically and should not be asserted from the installer without further evidence.
- Publication safety means build/stage first, audit independently, then expose the final output atomically; never publish a partial book.
- Old helper/source code is Sigil-derived GPLv3-or-later. Recover behavior/invariants/tests, not implementation code, unless licensing is deliberately resolved.
- The old edge-only unmatched-audio discovery model is historical behavior, not a replacement for Lite's current general segment-level alignment review.

## Recovery principle

The pre-Rust application is a **behavioral regression reference, not a porting target**. Lite intentionally cuts features. The migration rule remains:

> Replace → regression-test → delete. Never delete → hope we remembered everything.

Do not reintroduce historical features simply because they are discoverable in the installer or planning transcript. Only restore behavior that is part of the current Lite roadmap or is necessary to implement those behaviors safely.
