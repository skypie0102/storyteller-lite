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

### CPU workers and automatic recommendation

Whistle's pinned Needle runtime performs transcription on CPU, as described in the [Whistle announcement](https://cactuscompute.com/blog/whistle). It has no supported GPU backend in this app; a GPU does not raise the transcription worker count. Slint may use graphics acceleration to draw the window, which is separate from transcription.

The desktop defaults to **Automatic**. The application scans logical CPU threads available to the process and currently available physical RAM in the background during startup and Check setup. Windows uses `GlobalMemoryStatusEx`; Linux reads `MemAvailable`. Other platforms or failed probes fall back to a one-worker recommendation. No hardware-probing dependency was added.

The starting recommendation is the smaller of about half the available logical CPU threads and a memory allowance, bounded to 1–16. Reserve one quarter of available RAM, clamped to 512–2048 MiB, then allow 512 MiB per worker from the remainder. This is a conservative planning allowance for engine/conversion working memory, not measured per-worker consumption or a fastest-worker benchmark. A low-memory system still needs enough resources to run one worker.

| Available logical CPU threads | Available RAM | Automatic recommendation |
|---|---|---|
| 2 | 4 GiB | 1 |
| 8 | 8 GiB | 4 |
| 16 | 8 GiB | 8 |
| 32 | 16 GiB | 16 |
| 16 | 1.5 GiB | 2 |
| CPU or RAM unknown | — | 1 |

Manual selection supports 1–16. Sixteen is an application safety ceiling, not a Whistle restriction. Manual settings override the recommendation; more workers may be slower or consume more memory. Automatic is resolved when enqueueing, and recovery schema 2 stores the resulting integer. Older 1–4 records remain valid; changing the scan or preference does not retarget queued or restored books. The adapter reduces the running count when fewer chunks exist, and limits each chunk's FFmpeg decoding/PCM encoding to one thread to reduce nested CPU contention.

The adapter example accepts an explicit worker count or `auto`, with Automatic as the omitted-argument default. Each chunk still launches a fresh native CLI process; persistent loaded models and hardware throughput benchmarks remain future optimization work.

Windows checkpoint [37203054403](https://github.com/skypie0102/storyteller-lite/actions/runs/37203054403), source `e59482318b661621309179f918dc5056ad48c912`, passed all 171 workspace tests and the real native adapter checks. The hosted runner reported four available CPU threads and about 13 GiB available RAM, yielding a two-worker recommendation. Short Automatic input used one effective worker because it contained one chunk; explicit eight-worker selection on 135 seconds used six workers for six chunks. Native speech/silence, English enforcement, global timestamps, contiguous 30-second-capped coverage, silent-book rejection and temporary PCM cleanup also passed. These fixtures do not measure the fastest worker count or full-book recognition accuracy.

Default chunk target is 25 seconds. Chapter and silence adjustments must keep every chunk at or below 30 seconds. Every source interval appears exactly once in the validated contiguous plan. Workers use separate processes; cancellation stops owned subprocesses and cleanup removes temporary WAVs. Full-book transcript timestamps are restored using chunk offsets.

The current inputs do not overlap. If no suitable silence exists, a planned cut can split speech. Context overlap and boundary-word reconciliation remain full-book hardening work; continuous source coverage alone does not establish recognition accuracy at cuts. The final audio is encoded from the full source, not concatenated transcription chunks.

The normalized transcript contains phrase-level millisecond intervals. Native attention words may overlap and are grouped rather than discarded. Speech without complete usable timestamps is an error; silence is an empty chunk. An entirely silent input fails Analyze.

Analyze resume identity contains the engine/model contents and the English adapter semantics. Older automatic/multilingual Analyze checkpoints cannot be reused under the new adapter profile. Schema-1 recovered Whisper jobs restart from Analyze with Whistle. Absent/auto language settings migrate to English. Explicit foreign-language requests remain in the recovered paused queue with only Prepare eligible for reuse; worker preflight rejects them clearly before processing and allows the queue's existing failure handling to continue to other books. Recovery remains paused until the user resumes.

See [REBUILD.md](REBUILD.md) for acceptance checks and remaining rebuild work. The released v0.1.0 runtime is historical and is documented in Git history.
