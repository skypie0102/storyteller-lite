# Legacy installer static dissection — Storyteller One-Click v0.39.0

This document records facts recovered from the user-supplied pre-Rust/Slint-Lite installer. It is a **behavioral/reference artifact**, not a mandate to reintroduce old OneClick features.

## Sample identity

- Filename: `Storyteller-OneClick-v0.39.0-Windows-x64-Setup.exe`
- SHA-256: `417bce5a6e95bfac497bde4b1bbe48c3fb3ec7834909d0195f93741e9acf8eb9`
- Size: ~98 MiB
- Container: NSIS 3.11 self-extracting installer
- Analysis method: static only; the installer was never executed.

The archive starts at file offset 52,736. It uses a solid raw-LZMA stream with an 8 MiB dictionary. Decompression produces 300,642,500 bytes and 761 size-prefixed payload blocks. `tools/recovery/extract_legacy_nsis.py` reproduces the extraction for the exact known installer hash using only Python's standard library.

## High-value recovered payloads

### Block 7 — old desktop application

- Size: 12,431,872 bytes
- SHA-256: `fbfb10664dd127227bf95e881c2f30125d841baaa83a64518c848adcce38dbb1`
- Type: PE32+ x86-64 Windows GUI executable
- Version strings: `Storyteller One-Click v0.39.0`
- App identifier: `cloud.shadowmonarchbooks.storyteller-oneclick`
- Framework evidence: Rust + Tauri 2.11.5

The executable still contains Rust source-path strings, serde field names, command builders, and embedded Brotli-compressed Tauri frontend assets. `tools/recovery/extract_tauri_assets.py` reproducibly extracts/validates the known web assets from this exact executable.

### Blocks 753–758 — Sigil-derived finishing helper bundle

The installer includes GPL/source notices and, critically, the **plain-text Python source** for the old finishing helper.

- Block 757: bundled `sigil-repair.exe`, 9,126,723 bytes, SHA-256 `baac1f3d6a93d587c27a47f6dce53c6413331a6d3e2306cda2f651969466d01f`
- Block 758: plain Python helper source, 178,432 bytes, SHA-256 `35f426d7dcd1932eddfef0de054db7a6db445c724ba03fc509fbbcb041d91394`
- Block 755: Sigil-derived helper notice, SHA-256 `778a6e65b636d562b9d465fd261ecdf7f2c00cd4fe2fac0faad13437eac803ed`
- Block 753: GPLv3 license text

The Python helper explicitly says it is a headless port of selected Sigil repair/save behavior and is GPLv3-or-later. **Do not copy its implementation wholesale into Lite unless licensing for the new Rust code is deliberately resolved.** Use it primarily to recover behavior, invariants, formats, and tests.

## Old settings model recovered from the desktop executable

The serialized `Settings` struct had 17 fields:

`theme`, `engine`, `nodePath`, `ffmpegPath`, `whisperVariant`, `model`, `language`, `granularity`, `codec`, `bitrate`, `threads`, `parallelTranscribes`, `parallelTranscodes`, `standardizeEpub`, `edgeMode`, `useGraphicsOcr`, `readaloudCss`.

Recovered enum/string values include:

- Granularity: `sentence`, `word`
- Audio codec: `copy`, `libopus`, `aac`
- Edge mode: `manual`, `automatic_player`, `automatic_transcript`, `off`
- Queue/job states: `pending`, `running`, `waiting_for_allocation`, `allocation_paused`, `completed`, `failed`, `cancelled`

Recovered validation messages establish these old limits:

- CPU threads: 1–32
- Parallel transcription jobs: 1–4
- Parallel FFmpeg transcodes: 1–8
- Accepted bitrates: 16K / 32K / 64K / 96K

Machine-code recovery later established old backend defaults including `threads=6`, `parallelTranscribes=3`, and `parallelTranscodes=6`. See `FRONTEND_AND_CONCURRENCY_RECOVERY.md` for the full recovered default/state details.

### Lite interpretation

Do **not** restore the old 17-field settings surface. The important concurrency clue is that the old product distinguished CPU threads from parallel transcription jobs and from Whisper processors. See `WORKER_SEMANTICS.md`: current evidence favors Lite worker count meaning concurrent ordered audio chunks/tracks, not `whisper.cpp -p`.

## Manual allocator APIs recovered from the desktop executable/frontend

Exact frontend Tauri command names include:

- `get_pending_manual_request`
- `get_manual_allocation_draft`
- `save_manual_allocation_draft`
- `manual_audio_preview_path`
- `manual_image_preview`
- `submit_manual_allocations`
- `cancel_manual_allocation`

The persisted state included `manualDrafts`. Runtime/frontend behavior explicitly shows that if Storyteller closes while allocation is active/paused, retrying can rebuild the disposable inspection workspace and restore the autosaved allocation draft. This is strong evidence that **durable draft decisions** were part of the intended UX.

Recovered manual-allocation payload fields include:

`segment_id`, `start`, `end`, `category`, `target`, `document_path`, `image_path`, `reading_order`.

The old helper accepted these allocation categories:

- `introduction`
- `graphic_readout`
- `credits`
- `other`
- `silence`

And these output targets:

- `page`
- `image`
- `discard`

Old backend validation required each reviewed segment to be covered completely and continuously: no gaps or overlaps (with a defensive ~0.05 s tolerance). The recovered frontend was stricter at ~0.002 s. `discard` was legal only for `silence`/noise. Image targets had to come from the helper-generated eligible candidate set.

## Important scope finding: the old allocator was edge-focused

The recovered `_discover_unmatched_audio` logic did **not** treat every arbitrary internal alignment miss as a manual allocation candidate. It mainly discovered unmatched audio before the first safe matched range and after the last safe matched range, classifying those regions as introduction/credits candidates. Very short edge regions under roughly 2 seconds were ignored.

The helper also tried to determine whether the first/last boundary was safe from SMIL coverage or chapter sentence coverage before exposing it for preservation and refined some boundaries using transcript/silence evidence.

This matters for Lite: the current Rust alignment review is segment-based and may surface arbitrary unmatched transcript segments. Preserve the **good old invariants** (explicit durable decisions, bounded candidates, complete accounting, monotonic ordering) without blindly copying the old edge-only discovery model.

## Old manual inspection payload

For each discovered segment, the helper produced data equivalent to:

- stable `segment_id`
- `kind` (`introduction` / `credits`)
- preview source path/name
- preview start/end
- duration
- whether a physical preview clip had been spliced
- silence markers
- grouped transcript cues
- logical/original source names and original time range

If FFmpeg was available, it physically rendered a preview clip and ran silence detection (`silencedetect`, roughly -38 dB, 0.35 s). Transcript cues were grouped into readable chunks, flushing at punctuation, about 12 source entries, or ~8 seconds.

The inspector also returned candidate EPUB image/document pairs near the first/last aligned spine anchors for graphic-readout placement.

## Deeper unmatched-audio/OCR findings

Further static analysis recovered the old edge-mode command mapping, exact boundary-refinement constants, bounded/lazy OCR candidate behavior, Graphic Readout matching thresholds, and an old desktop wiring mismatch in which `--graphics-ocr` was forwarded only for `automatic_transcript` even though the helper supported OCR in player mode.

These details are intentionally centralized in `UNMATCHED_AUDIO_RECOVERY.md`. That document also reconciles them with the **actual Lite plan**, which retains Smart classification, automatic edge handling, lazy OCR, and a streamlined allocator while removing the permanent OCR toggle and full old editor surface.

## Old application pipeline clues

Recovered stage/runtime messages show this broad path:

1. workspace preparation and zero-copy/hard-link audiobook staging when possible;
2. alignment/transcription engine;
3. alignment report validation;
4. unmatched-audio inspection;
5. optional manual-allocation wait;
6. EPUB finishing helper;
7. final EPUB audit;
8. atomic publication.

The old app used `%LOCALAPPDATA%/Storyteller-OneClick/workspaces` for rebuildable processing workspaces. Persisted application state separately contained `manualDrafts`, and the UI distinguished `waiting_for_allocation` from `allocation_paused`.

## What should influence the rebuilt Lite allocator

Keep these recovered principles:

1. **Explicit per-segment decisions** rather than a single global "ignore all" switch.
2. **Durable drafts** so closing/retrying does not destroy review work.
3. **Preview support** with transcript/timing context.
4. **Constrained destinations** derived from valid EPUB ordering/context, not arbitrary unsafe jumps.
5. **Complete accounting**: reviewed audio should not silently disappear; assignment/exclusion must cover the intended segment.
6. **Smart but conservative automation**: high-confidence edge/image/silence cases may be handled automatically, weak cases remain reviewable.
7. **Lazy OCR** as an internal bounded capability rather than a permanent settings toggle.
8. **Validation before publication**.

Do not automatically restore:

- arbitrary split/merge/general waveform editing;
- the old broad rule/configuration UI;
- permanent OCR controls;
- manual CPU/thread controls;
- Node/npx alignment launcher;
- Sigil-derived finishing path.

Limited classifications such as Introduction, Credits, Graphic Readout, and Extra Audio remain useful when they materially drive Smart/manual destination behavior; the goal is to avoid recreating the *full editor surface*, not to erase useful semantics.

## Completed recovery follow-ups

The initial reverse-engineering targets from the first pass have now been substantially completed:

- real old defaults and `parallelTranscribes`/threads/processors semantics → `FRONTEND_AND_CONCURRENCY_RECOVERY.md` and `WORKER_SEMANTICS.md`;
- embedded Tauri frontend extraction/state transitions → `tools/recovery/extract_tauri_assets.py` and `FRONTEND_AND_CONCURRENCY_RECOVERY.md`;
- `manualDrafts` persistence/retry behavior → `FRONTEND_AND_CONCURRENCY_RECOVERY.md`;
- narrow helper invariants/test vectors, Smart/edge/OCR findings → `UNMATCHED_AUDIO_RECOVERY.md`.

Further archaeology should now be demand-driven: inspect the installer only when a current Lite behavior remains ambiguous.
