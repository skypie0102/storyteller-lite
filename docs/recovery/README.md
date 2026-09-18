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

Start with these high-value notes:

- `LEGACY_INSTALLER_REFERENCE.md` — provenance, exact installer identity, and scope rules.
- `INSTALLER_DISSECTION.md` — NSIS payload structure, recovered desktop/helper payloads, and first-pass behavior clues.
- `FRONTEND_AND_CONCURRENCY_RECOVERY.md` — recovered Tauri frontend behavior, real old Settings defaults, queue snapshots, allocator autosave/state-machine behavior, and persistence architecture.
- `WORKER_SEMANTICS.md` — evidence that old Parallel Whisper jobs meant concurrent processed audio files/chunks, not whisper.cpp processor count, plus the preferred Lite worker architecture.
- `UNMATCHED_AUDIO_RECOVERY.md` — reconciles the Lite planning transcript, mockup, and old helper around Smart / ReviewAll, automatic edge handling, lazy OCR, Graphic Readout/image placement, and the deliberately reduced allocator surface.
- `ALLOCATOR_OUTPUT_RECOVERY.md` — downstream EPUB meaning of reviewed Introduction/Credits/Graphic Readout assignments, including supplemental XHTML+SMIL pages and image-bound narration.
- `ALLOCATOR_PREVIEW_RECOVERY.md` — shows that manual-review preview clips were disposable cache while original/global segment timing remained authoritative.
- `ALLOCATOR_CANDIDATE_RECOVERY.md` — bounded spine/document/image candidate generation and path-safety rules; the UI should not submit arbitrary EPUB/filesystem paths.
- `WAVEFORM_RECOVERY.md` — recovered lightweight waveform model: a small peak envelope rather than a heavyweight audio editor.
- `OCR_PACKAGING_RECOVERY.md` — old RapidOCR/ONNX/Tesseract dependency footprint and why Lite should keep OCR bounded/lazy rather than inherit the whole frozen Python stack.
- `QUEUE_FAILURE_RECOVERY.md` — recovered terminal queue policy: completed, failed, and cancelled books advance to the next pending book unless Stop After/Pause-after-current is active.
- `RETRY_REPROCESS_RECOVERY.md` — historical distinction between Retry and deliberate completed-book Reprocess/overwrite; this is reference only because current Lite intentionally refuses overwrite.
- `PUBLICATION_RECOVERY.md` — recovered staged-audit and atomic publish/rollback semantics, plus how Lite deliberately simplifies them by refusing overwrite.
- `INTEGRITY_RECOVERY.md` — native final-audit behavior and exact narrow zero-length SMIL repair: `clipEnd = clipBegin + 0.001s`, followed by full re-audit.
- `RUNTIME_PACKAGING_RECOVERY.md` — what v0.39 bundled versus installed/discovered at runtime; notably the old app used an unpinned `@storyteller-platform/align@latest` path that Lite should not recreate.
- `BACKEND_ARCHITECTURE_RECOVERY.md` — recovered high-level module boundaries: thin commands, service-layer queue/manual/settings/tools logic, and separate pipeline/report/integrity/state code.
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
- Destination selection is constrained to validated EPUB candidates derived from reading/spine context; the UI should not be a free-form path editor.
- Terminal books normally **do not stall the queue**. Completed, failed, and cancelled current books advance to the next waiting item; Stop After/Pause-after-current is the explicit queue-level stop mechanism.
- Publication safety means build/stage first, audit independently, then expose the final output atomically; never publish a partial book.
- The old native zero-length SMIL compatibility repair was exactly **+1 ms to `clipEnd`**, followed by full final audit. It was not a general timing repair algorithm.
- The old runtime's `@storyteller-platform/align@latest` resolution is historical nondeterminism, not a design to restore.
- Old helper/source code is Sigil-derived GPLv3-or-later. Recover behavior/invariants/tests, not implementation code, unless licensing is deliberately resolved.
- The old edge-only unmatched-audio discovery model is historical behavior, not a replacement for Lite's current general segment-level alignment review.

## Recovery status

The installer archaeology has recovered the main product-level contracts needed for the Lite rebuild: worker semantics, queue behavior, durable review, allocator destinations/previews, Smart/lazy-OCR intent, downstream EPUB rendering, publication safety, runtime packaging boundaries, and the final integrity repair rule.

Further EXE work should now be **demand-driven**. Prefer validating and rebuilding the current Rust + Slint Lite implementation unless a concrete behavior remains ambiguous.

## Recovery principle

The pre-Rust application is a **behavioral regression reference, not a porting target**. Lite intentionally cuts features. The migration rule remains:

> Replace → regression-test → delete. Never delete → hope we remembered everything.

Do not reintroduce historical features simply because they are discoverable in the installer or planning transcript. Only restore behavior that is part of the current Lite roadmap or is necessary to implement those behaviors safely.
