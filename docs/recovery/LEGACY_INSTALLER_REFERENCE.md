# Legacy installer behavioral reference

The user supplied an old Storyteller OneClick installer during recovery:

- Filename: `Storyteller-OneClick-v0.39.0-Windows-x64-Setup.exe`
- Size: `101,861,218` bytes (~97.1 MiB)
- SHA-256: `417bce5a6e95bfac497bde4b1bbe48c3fb3ec7834909d0195f93741e9acf8eb9`
- Container identification: PE32 Windows GUI executable, Nullsoft/NSIS self-extracting archive.
- Embedded NSIS manifest identifies **Nullsoft Install System v3.11**.

## Critical scope warning

This installer is **pre Rust + Slint refactor and is not Storyteller Lite**. Never treat its implementation architecture, dependency choices, UI complexity, settings surface, or feature count as requirements for the current project.

Use it only when current Lite behavior is ambiguous and an old behavior needs to be reconstructed.

## What it is useful for

Potentially recoverable reference information includes:

- terminology and control labels;
- manual unmatched/unaligned audio allocation behavior;
- review sequencing and decision semantics;
- old default values/settings behavior;
- audio preview and trim/review interactions;
- how historical unmatched segments were represented/persisted;
- any useful Whisper process/worker defaults;
- icons/assets that clarify behavior.

## What must not be copied by default

Do not automatically restore:

- the old framework/runtime architecture;
- full old Settings complexity;
- manual CPU/thread allocation controls;
- Activity/console-first UI;
- Runtime Health screens;
- Process-now flow;
- word-level synchronization;
- engine/runtime-path tuning UI;
- standardize-EPUB toggle;
- CSS editor;
- permanent OCR UI;
- the full-complexity historical allocator/editor.

Current user decision: **one simple Whisper worker-count setting is desired** even though the recovered planning transcript once listed worker/thread controls as removable. This newer decision overrides that older planning note. Manual CPU allocation remains out of scope.

## Installer preservation

The 97 MiB installer binary itself is not committed to Git history in this recovery packet because it would materially bloat the repository and this connector cannot safely stream the local binary as a normal repository file. The identifying filename, exact byte size, SHA-256, packaging format, and intended use are preserved here so an independently retained copy can be verified later.

If a future agent has access to the original installer, verify the SHA-256 above before using it. Prefer extraction/static inspection over executing the installer. Record any recovered behavior in this directory and update `docs/ROADMAP.md`; do not directly port old code without an explicit Lite product decision.
