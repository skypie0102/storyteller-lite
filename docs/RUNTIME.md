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

The runtime checks an explicit `STORYTELLER_FFMPEG` override first, then the portable application/tool folders and `PATH`. Settings probes the resolved executable with `ffmpeg -version` before reporting it ready.

### whisper.cpp CLI and CUDA builds

`STORYTELLER_WHISPER` is an explicit override and wins when it points to a working CLI. Automatic discovery accepts both modern `whisper-cli.exe` and the older `main.exe` naming used by previous whisper.cpp packages.

On Windows, automatic discovery searches:

- the portable StoryTeller application and `tools/` folders;
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

If a compatible cached archive is found but no runnable CLI is available, **Download missing** tries to reuse/extract that archive into the current portable `tools/` folder before downloading a replacement. Both `whisper-cli.exe` and legacy `main.exe` archive layouts are supported, and sibling runtime DLLs/resources are copied with the executable.

Immediately before a processing worker starts, StoryTeller Lite re-runs runtime discovery and binds the exact resolved ffmpeg, whisper.cpp, and model paths into the backend environment. Therefore the CUDA/CPU executable shown by Settings is the executable Analyze will launch, unless the user supplied an explicit override.

### Whisper model

`STORYTELLER_WHISPER_MODEL` may point directly to a non-empty model file. Otherwise discovery checks the current StoryTeller model folders and the same bounded historical runtime locations for `ggml-<model>.bin`.

A model is reported ready only when the candidate is a non-empty regular file.

## Settings runtime manager

Opening Settings triggers a runtime scan. The page reports the resolved path for each dependency and exposes **Re-scan**. The whisper row identifies a detected CUDA build explicitly. If anything is missing, **Download missing** offers an explicit, user-initiated portable install on Windows. Processing never silently starts a multi-gigabyte model download.

Downloads are performed on a background thread so the Slint UI remains responsive. Missing dependencies are installed beside the portable application:

```text
tools/ffmpeg.exe
tools/whisper-cli.exe
models/ggml-large-v3-turbo.bin
```

Current download behavior and integrity checks:

- ffmpeg: Gyan Windows Essentials ZIP plus the provider's published `.sha256`; the archive hash must match before extraction.
- whisper.cpp: official `ggml-org/whisper.cpp` GitHub Windows x64 release assets; the GitHub asset must publish a `sha256:` digest and the downloaded archive must match it.
- If `nvidia-smi` confirms an NVIDIA GPU and whisper.cpp is missing, the downloader prefers an official CUDA/cuBLAS-enabled Windows x64 asset, falling back to the CPU x64 asset only if a compatible CUDA asset is unavailable.
- `large-v3-turbo`: the canonical whisper.cpp model download; the staged file must match the pinned SHA-256 before it is moved into `models/`.

After installation, StoryTeller Lite probes ffmpeg and whisper.cpp again and re-runs dependency detection before displaying the final state. If one dependency succeeds and a later dependency fails, the successful portable file is retained and the next attempt downloads only what is still missing.

Automatic installation is currently Windows-focused and uses PowerShell. The portable application directory must be writable; a build placed under a protected directory such as `Program Files` may need to be moved to a user-writable folder before installing dependencies. The current downloader does not expose mid-download cancellation yet.

Missing dependencies still produce explicit Analyze/setup errors if the user chooses not to install them from Settings.

## Analyze artifacts

Analyze creates its own stage workspace and records:

- `audio.wav` — 16 kHz, mono, signed 16-bit PCM generated by ffmpeg
- `transcript.json` — whisper.cpp JSON-full transcription output

Both must exist and be non-empty before the stage can be checkpointed.

## Progress policy

The Analyze progress percentage comes from whisper.cpp's own progress callback output. Storyteller Lite does not estimate a transcription percentage from wall-clock time. Backend/model labels are shown only when the backend has actually started, and a GPU backend label is only adopted when whisper.cpp reports one.

## Cancellation

External processing commands are launched with stdin disabled and stdout/stderr drained on dedicated reader threads. The worker polls its cancellation token; cancellation kills and waits for the child process before the stage returns as cancelled. This prevents a cancelled job from leaving ffmpeg or whisper-cli running behind the UI.
