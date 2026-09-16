# Runtime tooling

Storyteller Lite keeps heavyweight media/ML tools outside the Rust UI process. The current Analyze backend expects `ffmpeg`, a whisper.cpp CLI (`whisper-cli` or the legacy `main` executable), and a ggml Whisper model.

## Default Whisper model

The current job default is `large-v3-turbo`, so model discovery looks for:

```text
ggml-large-v3-turbo.bin
```

A different model name may be supplied by `JobSettings` as model selection is expanded in the Settings UI.

## Tool discovery

### ffmpeg

The runtime checks an explicit `STORYTELLER_FFMPEG` override first, then the portable application/tool folders, the managed per-user `tools/` folder under `%LOCALAPPDATA%\Storyteller OneClick Lite`, and `PATH`. Settings probes the resolved executable with `ffmpeg -version` before reporting it ready.

### whisper.cpp CLI and CUDA builds

`STORYTELLER_WHISPER` is an explicit override and wins when it points to a working CLI. Automatic discovery accepts both modern `whisper-cli.exe` and the older `main.exe` naming used by previous whisper.cpp packages.

On Windows, automatic discovery searches:

- the portable StoryTeller application and adjacent `tools/` folders;
- the managed per-user `%LOCALAPPDATA%\Storyteller OneClick Lite\tools\` folder;
- `PATH`;
- bounded persistent-runtime searches under `%LOCALAPPDATA%`, `%APPDATA%`, `%PROGRAMDATA%`, the user's profile, and `.cache`;
- known StoryTeller/whisper folder names and direct child folders whose names contain StoryTeller, whisper, or historical project/vendor hints.

The persistent search is intentionally bounded (depth and entry count) rather than scanning the entire PC. Candidates must successfully run with `--help` before they are considered usable.

When multiple working auto-discovered whisper.cpp executables exist, a CUDA-capable build is preferred. CUDA capability is recognized from CUDA/cuBLAS path names or neighboring runtime libraries such as `ggml-cuda`, `cublas64`, `cublasLt64`, and `cudart64`.

The runtime also recognizes the historical package naming pattern used by the former StoryTeller app, including:

```text
whisper-cpp-windows-x64-cuda-13.1.0.tar.gz
whisper-cpp-windows-x64-cuda-*.tar.gz
whisper-cpp-windows-x64-cuda-*.tgz
whisper-cpp-windows-x64-cuda-*.zip
```

If a compatible cached archive is found but no runnable CLI is available, **Download missing** tries to reuse/extract that archive into the managed per-user `tools/` folder before downloading a replacement. Both `whisper-cli.exe` and legacy `main.exe` archive layouts are supported, and sibling runtime DLLs/resources are copied with the executable.

Immediately before a processing worker starts, StoryTeller Lite re-runs runtime discovery and binds the exact resolved ffmpeg, whisper.cpp, and model paths into the backend environment. Therefore the CUDA/CPU executable shown by Settings is the executable Analyze will launch, unless the user supplied an explicit override.

### Whisper model

`STORYTELLER_WHISPER_MODEL` may point directly to a non-empty model file. Otherwise discovery checks the current StoryTeller model folders and the same bounded historical runtime locations for `ggml-<model>.bin`.

A model is reported ready only when the candidate is a non-empty regular file.

## Settings runtime manager

Opening Settings triggers a runtime scan. The page reports the resolved path for each dependency and exposes **Re-scan**. The whisper row identifies a detected CUDA build explicitly. If anything is missing, **Download missing** offers an explicit, user-initiated per-user install on Windows. Owned downloads do not require the application/EXE directory itself to be writable. Processing never silently starts a multi-gigabyte model download.

### Importing an existing whisper.cpp archive

Settings also exposes **Import whisper archive…** for users who already have a Windows whisper.cpp build. This is the preferred deterministic path when automatic discovery cannot locate an existing runtime.

The picker accepts `.zip`, `.tgz`, and `.tar.gz` files, including the former StoryTeller CUDA package shape:

```text
whisper-cpp-windows-x64-cuda-13.1.0.tar.gz
```

The selected archive stays local; StoryTeller Lite does not upload it or redownload whisper.cpp. The archive is extracted into a persistent per-user runtime location:

```text
%LOCALAPPDATA%\Storyteller OneClick Lite\runtime\whisper\<archive-name>-<timestamp>\
```

Lite searches the extracted tree for `whisper-cli.exe`, falling back to legacy `main.exe`. It then launches the discovered CLI with `--help`; the import is rejected and its extracted directory is removed if the executable cannot start, which catches missing/incompatible runtime DLLs before a book is queued.

On successful import, the verified executable is selected immediately for the current app session and a runtime re-scan updates Settings. Future launches rediscover the persistent extracted copy automatically. The original archive is not modified and may be moved or deleted after a successful import.

Downloads are performed on a background thread so the Slint UI remains responsive. Missing dependencies are installed into the managed per-user application-data root:

```text
%LOCALAPPDATA%\Storyteller OneClick Lite\tools\ffmpeg.exe
%LOCALAPPDATA%\Storyteller OneClick Lite\tools\whisper-cli.exe
%LOCALAPPDATA%\Storyteller OneClick Lite\models\ggml-large-v3-turbo.bin
```

Portable adjacent `tools/` / `models/` resources remain valid discovery inputs for deliberately self-contained bundles, but automatic downloads no longer mutate the application directory.

Current download behavior and integrity checks:

- ffmpeg: Gyan Windows Essentials ZIP plus the provider's published `.sha256`; the archive hash must match before extraction.
- whisper.cpp: official `ggml-org/whisper.cpp` GitHub Windows x64 release assets; the GitHub asset must publish a `sha256:` digest and the downloaded archive must match it.
- If `nvidia-smi` confirms an NVIDIA GPU and whisper.cpp is missing, the downloader prefers an official CUDA/cuBLAS-enabled Windows x64 asset, falling back to the CPU x64 asset only if a compatible CUDA asset is unavailable.
- `large-v3-turbo`: the canonical whisper.cpp model download; the staged file must match the pinned SHA-256 before it is moved into `models/`.

After installation, StoryTeller Lite probes ffmpeg and whisper.cpp again and re-runs dependency detection before displaying the final state. If one dependency succeeds and a later dependency fails, the successful per-user managed file is retained and the next attempt downloads only what is still missing.

Automatic installation is currently Windows-focused and uses PowerShell. `%LOCALAPPDATA%` must be available and writable for managed downloads, so an installed EXE may live under a protected directory such as `Program Files` without requiring elevation merely to install StoryTeller-owned runtime dependencies. The current downloader does not expose mid-download cancellation yet.

Missing dependencies still produce explicit Analyze/setup errors if the user chooses not to install them from Settings.

## Analyze artifacts

Analyze no longer creates a whole-book PCM checkpoint. It plans deterministic bounded transcription chunks, preferring nearby chapter boundaries and refining synthetic cuts around detected silence when useful. Each active worker converts only its current range to temporary 16 kHz mono signed 16-bit PCM, runs whisper.cpp on that bounded WAV, then deletes the temporary PCM after the chunk result is collected.

Analyze records these durable stage artifacts:

- `book-corpus.json` — extracted EPUB reading-order corpus
- `transcription-plan.json` — audiobook duration and deterministic chunk boundaries
- `transcript.json` — normalized merged Whisper transcript with global audiobook timestamps

The worker count is an execution-only setting (1–4 simultaneous chunks, default 1). Available logical CPU threads are divided across the workers that are actually active. Changing worker count does not invalidate semantic stage checkpoints.

Both ffmpeg and whisper.cpp identities participate in the Analyze backend fingerprint because ffmpeg is part of chunk planning/conversion as well as Whisper input preparation.
## Progress policy

The Analyze progress percentage comes from whisper.cpp's own progress callback output. StoryTeller Lite does not estimate a transcription percentage from wall-clock time. Backend/model labels are shown only when the backend has actually started, and a GPU backend label is only adopted when whisper.cpp reports one.

## Cancellation

External processing commands are launched with stdin disabled and stdout/stderr drained on dedicated reader threads. The worker polls its cancellation token; cancellation kills and waits for the child process before the stage returns as cancelled. This prevents a cancelled job from leaving ffmpeg or whisper-cli running behind the UI.
