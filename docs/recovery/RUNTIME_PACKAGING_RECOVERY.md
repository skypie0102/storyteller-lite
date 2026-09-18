# Runtime/dependency packaging recovery — Storyteller OneClick v0.39.0

This note records which processing dependencies were physically bundled in the old installer versus discovered/installed at runtime. It is historical evidence for Lite's runtime strategy, not a requirement to restore the old Node/npx architecture.

## What the installer actually bundled

The NSIS file table clearly contains:

- the main `storyteller-oneclick.exe` desktop application;
- the `tools/sigil-repair/` frozen helper tree;
- the helper's Python/OCR/native libraries and models;
- helper license/source notices.

The installer file table does **not** contain normal installed-file entries for:

- `node.exe`
- `npx.cmd`
- `ffmpeg.exe`
- `ffprobe.exe`
- `whisper-cli`
- a `ggml-*.bin` Whisper model

Those dependencies were runtime-managed instead.

## Node / npx

Recovered dependency scanner strings show the old app expected:

- **Node.js 20 or newer**;
- npm/npx available beside Node or on `PATH`;
- specifically, the npm launcher `node_modules/npm/bin/npx-cli.js` could be resolved beside an installed Node/npm tree.

The old normal alignment launcher was described as:

```text
@storyteller-platform/align@latest via npx
```

When processing began it invoked npx with the unpinned package spec:

```text
@storyteller-platform/align@latest
```

So the actual alignment/transcription package version could change independently of the desktop installer.

## FFmpeg / FFprobe

The dependency scanner looked for:

- `ffmpeg.exe` / `ffmpeg`
- `ffprobe.exe` / `ffprobe`

and accepted configured paths or executables on `PATH`.

FFprobe was expected either beside FFmpeg or otherwise discoverable.

## Automatic dependency installation

The compiled backend contains a Windows Package Manager installer path.

If `winget.exe` was available, the old app could install missing dependencies using package IDs:

- `OpenJS.NodeJS.LTS` — Node.js LTS
- `Gyan.FFmpeg` — FFmpeg + FFprobe

The winget command included non-interactive agreement flags similar to:

```text
winget install
  --id <package>
  --exact
  --source winget
  --silent
  --accept-source-agreements
  --accept-package-agreements
  --disable-interactivity
```

If winget was unavailable the app told the user to install/repair Microsoft App Installer.

Explicit custom paths, custom alignment engines, Whisper selection and bundled Storyteller files were not automatically repairable by this path.

## Whisper runtime behavior

Old settings exposed a Whisper build/variant selection and the backend set `STORYTELLER_WHISPER_VARIANT` for the external alignment process.

Recovered scanner text explicitly says the selected Whisper backend's packaged/downloaded binary is validated **by the alignment engine when transcription starts**.

Therefore the old desktop app did not itself own a fixed whisper.cpp executable/model bundle. That responsibility was delegated to the externally resolved Storyteller alignment package/runtime.

## Determinism problem in the old architecture

The v0.39 desktop installer pinned its own application version but launched `@storyteller-platform/align@latest` at processing time.

This means behavior such as:

- audio preprocessing defaults;
- chunk length;
- whisper.cpp wrapper behavior;
- downloaded Whisper runtime version;
- possibly model/runtime compatibility

could drift after the desktop app was installed.

That is why some historical details cannot be recovered as one exact v0.39 behavior from the installer alone.

## Lite implication

Do **not** restore Node/npx merely because old OneClick depended on it.

The Rust + Slint Lite rebuild should prefer:

1. a runtime contract owned/versioned by Lite;
2. pinned or compatibility-checked ffmpeg/whisper.cpp expectations;
3. deterministic Analyze behavior independent of an `@latest` package;
4. simple runtime discovery/install/import UX rather than exposing old engine/path tuning by default;
5. reproducible model/runtime identity in checkpoints/diagnostics;
6. no permanent dependency on the old Sigil/Python helper once native Rust EPUB/review behavior replaces it.

This aligns with the current recovered Lite code, which already calls ffmpeg and whisper.cpp directly instead of launching the old npm align pipeline.

## Packaging contrast with OCR

There was a notable asymmetry in v0.39:

- Node/FFmpeg/Whisper alignment runtime was discovered/downloaded externally;
- the Sigil/OCR finishing helper was bundled wholesale inside the installer.

Lite is free to make a different packaging decision for lazy OCR. See `OCR_PACKAGING_RECOVERY.md`; the old frozen Python OCR footprint is behavioral evidence, not a packaging template.
