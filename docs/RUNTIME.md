# Runtime tooling

Analyze uses FFmpeg and Whistle through the native Needle 3.1.0 executable. The application has no Python, whisper.cpp, ggml model, CUDA package or Whisper archive import requirement.

## Owned assets

| Asset | Pinned source | SHA-256 |
|---|---|---|
| Whistle model | `Cactus-Compute/whistle`, revision `d3ea19e0fe4f99fa7dfb9afa63070b1c6eacaff1`, `whistle.cact` | `b6e02f048568ac5d01a2042556c658061e699acbc0aa2a1439f52f3d461dffeb` |
| Windows x64 engine | `Cactus-Compute/needle3`, revision `f84005f8992caf37f17b0d64a4b5b31a84ce0d2a`, `windows-x86_64/needle.exe` | `c70ca998f6c542c862c06c22352046e303ef5c769384069ee25cef9f4667e4cf` |
| Linux x64 engine for adapter checks | Same engine revision, `linux-x86_64/needle` | `b197ceaef3b300a0b14c3a4fde92305527e43f9256c53d2a53d2a2fe8fe69678` |
| FFmpeg Windows archive | Gyan/Codex `9.0.1`, `ffmpeg-9.0.1-essentials_build.zip` | `fec81ae03971d9dd4be3ebe02e263bd2ec1d789483f931bdba5f5715e65da2e9` |

A moving Hugging Face `main` reference is not used for automatic installation. Model and native engine files are hash-checked during discovery as well as download. Explicit overrides must match these supported assets. A working existing FFmpeg can be reused after its version probe.

## Discovery and installation

Settings reports FFmpeg, the Whistle engine and the Whistle model. Re-scan checks configured paths, the managed per-user folders, portable folders and PATH for executables. The model is discovered in configured or managed/portable model folders. Broad historical Whisper directory scanning has been removed.

Advanced overrides:

```text
STORYTELLER_FFMPEG=<path to ffmpeg.exe>
STORYTELLER_WHISTLE=<path to the pinned needle.exe>
STORYTELLER_WHISTLE_MODEL=<path to the pinned whistle.cact>
```

**Download missing** remains a user-initiated Windows x64 operation. Assets are stored below `%LOCALAPPDATA%\Storyteller OneClick Lite`:

```text
tools/ffmpeg.exe
tools/whistle/needle.exe
models/whistle.cact
```

Downloads first write a temporary sibling and verify its checksum before promotion. FFmpeg extraction uses a temporary directory and requires exactly one executable. Failure removes temporary downloads/extraction. Processing rechecks supported assets before Analyze. Runtime and recovery share the same per-user root; the executable directory need not be writable.

Automatic acquisition is Windows x64 only. Core and the transcription adapter can be tested on Linux x64 with the pinned Linux executable supplied explicitly. Other native targets require a separate tested asset pin before support is claimed.

## Transcription

The adapter creates 16 kHz mono PCM WAVs and invokes the native engine with its Whistle model, audio file, word-timestamp option and `--audio-language en`. English is the only application-supported language; there is no adapter language option or automatic detection. Non-English speech results are rejected by the parser. Engine telemetry is disabled for application-owned invocations.

The published model stores shared multilingual weights in one 16.9 MB file. English-only integration removes application language selection and routing, but does not shrink that pinned file or establish a runtime memory/speed improvement. Model and engine checksums remain unchanged.

Default chunk target is 25 seconds. Chapter and silence adjustments must keep every chunk at or below 30 seconds. Every source interval appears exactly once in the validated contiguous plan. Workers use separate processes; cancellation stops owned subprocesses and cleanup removes temporary WAVs. Full-book transcript timestamps are restored using chunk offsets.

The normalized transcript contains phrase-level millisecond intervals. Native attention words may overlap and are grouped rather than discarded. Speech without complete usable timestamps is an error; silence is an empty chunk. An entirely silent input fails Analyze.

Analyze resume identity contains the engine/model contents and the English adapter semantics. Older automatic/multilingual Analyze checkpoints cannot be reused under the new adapter profile. Schema-1 recovered Whisper jobs restart from Analyze with Whistle. Absent/auto language settings migrate to English. Explicit foreign-language requests remain in the recovered paused queue with only Prepare eligible for reuse; worker preflight rejects them clearly before processing and allows the queue's existing failure handling to continue to other books. Recovery remains paused until the user resumes.

See [REBUILD.md](REBUILD.md) for acceptance checks and remaining rebuild work. The released v0.1.0 runtime is historical and is documented in Git history.
