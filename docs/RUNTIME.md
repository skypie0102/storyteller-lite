# Runtime tooling

Analyze uses FFmpeg and Whistle through the native Needle 3.1.0 executable. The default installation has no Python, whisper.cpp, ggml model or CUDA requirement. Whisper GPU is an explicit optional download; it does not replace Whistle.

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

Manual selection supports 1–16. Sixteen is an application safety ceiling, not a Whistle restriction. Manual settings override the recommendation; more workers may be slower or consume more memory. Automatic is resolved when enqueueing, and recovery schema 3 stores the resulting integer (schema 2 remains readable). Older 1–4 records remain valid; changing the scan or preference does not retarget queued or restored books. The adapter reduces the running count when fewer chunks exist, and limits each chunk's FFmpeg decoding/PCM encoding to one thread to reduce nested CPU contention.

The adapter example accepts an explicit worker count or `auto`, with Automatic as the omitted-argument default. Each Whistle chunk still launches a fresh native CLI process; persistent Whistle models and hardware throughput benchmarks remain future optimization work.

Windows checkpoint [37203054403](https://github.com/skypie0102/storyteller-lite/actions/runs/37203054403), source `e59482318b661621309179f918dc5056ad48c912`, passed all 171 workspace tests and the real native adapter checks. The hosted runner reported four available CPU threads and about 13 GiB available RAM, yielding a two-worker recommendation. Short Automatic input used one effective worker because it contained one chunk; explicit eight-worker selection on 135 seconds used six workers for six chunks. Native speech/silence, English enforcement, global timestamps, contiguous 30-second-capped coverage, silent-book rejection and temporary PCM cleanup also passed. These fixtures do not measure the fastest worker count or full-book recognition accuracy.

Default chunk target is 25 seconds. Chapter and silence adjustments must keep every chunk at or below 30 seconds. Every source interval appears exactly once in the validated contiguous plan. Workers use separate processes; cancellation stops owned subprocesses and cleanup removes temporary WAVs. Full-book transcript timestamps are restored using chunk offsets.

The current inputs do not overlap. If no suitable silence exists, a planned cut can split speech. Context overlap and boundary-word reconciliation remain full-book hardening work; continuous source coverage alone does not establish recognition accuracy at cuts. The final audio is encoded from the full source, not concatenated transcription chunks.

The normalized transcript contains phrase-level millisecond intervals. Native attention words may overlap and are grouped rather than discarded. Speech without complete usable timestamps is an error; silence is an empty chunk. An entirely silent input fails Analyze.

Analyze resume identity contains the engine/model contents and the English adapter semantics. Older automatic/multilingual Analyze checkpoints cannot be reused under the new adapter profile. Schema-1 recovered Whisper jobs restart from Analyze with Whistle. Absent/auto language settings migrate to English. Explicit foreign-language requests remain in the recovered paused queue with only Prepare eligible for reuse; worker preflight rejects them clearly before processing and allows the queue's existing failure handling to continue to other books. Recovery remains paused until the user resumes.

See [REBUILD.md](REBUILD.md) for acceptance checks and remaining rebuild work. The released v0.1.0 runtime is historical and is documented in Git history.

## Optional Whisper Turbo GPU

Choose **Whisper Turbo · NVIDIA GPU** in Settings, check setup, then select **Download Whisper tools**. The installer downloads FFmpeg only if needed, plus the separate CUDA runtime and model; it does not download Whistle for a Whisper job. Whistle's normal installation never acquires Whisper assets. Runtime installation is disabled while a book is active. Both backends process local audio offline after setup.

This first GPU package supports **Windows x64 NVIDIA only**, using the first GPU in PCI bus order. `nvidia-smi` must report at least 4096 MiB free VRAM. This is a conservative initial application policy, not a measured minimum for all devices. Custom `CUDA_VISIBLE_DEVICES` mappings are not supported. A working NVIDIA driver is required; a separately installed CUDA toolkit is not required by the pinned archive. AMD/Intel/Vulkan support and multiple-GPU selection are pending. There is no automatic CPU fallback and no automatic switch away from Whistle.

| Asset | Pinned source | SHA-256 |
|---|---|---|
| whisper.cpp CUDA 11.8 runtime | Official release `b5130`, commit `927cfce34f31707e17f2bff35c349632fb9e2c3a` (v1.9.4), `whisper-cublas-11.8.0-bin-x64.zip` | `0b29b2175bb17ec26da29677cbc7c467c57d103245144d62a49a703f6bc3fdae` |
| Turbo Q5 model | `ggerganov/whisper.cpp`, revision `5359861c739e955e79d9a303bcbc70fb988958b1`, `ggml-large-v3-turbo-q5_0.bin` | `394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2` |

The optional download is about 847 MB, with about 1.2 GB installed; allow 3 GB free disk space for staging/replacement. The archive is verified first, then only the expected individually hash-verified CLI/DLL files are staged and published as a bundle. Discovery rejects extra executables or DLLs in that bundle. The upstream MIT notice accompanies the runtime. Model installation uses the same verified sibling-file publication as Whistle. No Cargo dependencies or base executable CUDA linkage are added.

Managed paths and optional advanced overrides:

```text
tools/whisper-cuda/whisper-cli.exe
models/ggml-large-v3-turbo-q5_0.bin
STORYTELLER_WHISPER=<path to the pinned CLI with the complete supported DLL bundle>
STORYTELLER_WHISPER_MODEL=<path to the pinned Turbo Q5 model>
```

Whisper uses `--language en`, disables non-speech tokens, and emits native JSON offsets in milliseconds. The shared adapter uses 25-second targets refined around chapters/silence, merges global offsets, and cleans temporary WAV/JSON files on success, failure and cancellation. Every invocation must show actual CUDA backend initialization and CUDA model-weight allocation. A GPU request flag or a CUDA-compiled executable alone does not count as offload evidence. Initialization failure stops CPU fallback before its result can be accepted. Progress only counts completed audio.

English-only routing does not remove multilingual weights from Turbo; there is no official Turbo `.en` variant in this pinned set. The smaller Q5 checkpoint reduces the optional model download relative to unquantized Turbo. No accuracy or speed improvement is claimed without a representative comparison.

Schema 3 saves `transcription_backend` alongside model and workers. Whisper requires exactly one worker to avoid repeated VRAM copies; Whistle retains Automatic/1–16 CPU workers. Restored GPU jobs retain their backend even if the GPU is unavailable. Schema 2 defaults to Whistle, while schema 1 retains the earlier migration behavior. Engine/model profiles distinguish cached Whisper and Whistle analysis.

### Hardware acceptance still required

The bounded-reuse follow-up gives Whisper up to eight independent chunks per CLI invocation. The pinned CLI loads one model before its file loop and resets text context for every input; inference remains sequential in one GPU context. Temporary mono PCM is bounded to about 7.7 MB and paired WAV/JSON files are removed before the next group. Every paired output must parse successfully before any chunk in the group advances progress. Cancellation or GPU initialization failure stops the owned invocation and cleans staged data. Whole-book residency, concurrent GPU inference and measured backend recommendations remain future optimizations. Hosted Windows checks have no GPU: they can test native English JSON using the pinned Turbo Q5 model on CPU, fallback rejection, recovery and UI, but cannot validate Turbo GPU execution or throughput.

The bounded-reuse [Windows checkpoint](https://github.com/skypie0102/storyteller-lite/actions/runs/37231217153) passed formatting, strict locked core/application Clippy, all 181 core/application tests, native Whistle with six workers, native Turbo with two inputs and one model load, and stopped multi-window GPU fallback/cleanup. [Retained evidence](runtime-validation/whisper-reuse.json) records the verified source and artifact hashes. This is CPU-hosted integration evidence; real NVIDIA and full-book acceptance remain open.

The [Windows checkpoint](https://github.com/skypie0102/storyteller-lite/actions/runs/37211560379) passed those native checks, all 183 workspace tests and strict Clippy. Its only failure was formatting, corrected by a formatting-only change and a successful [formatting check](https://github.com/skypie0102/storyteller-lite/actions/runs/37215449790). [REBUILD.md](REBUILD.md) records the exact source commits and evidence limits.

On a compatible Windows NVIDIA system, compare the same representative English audiobook with both backends, inspect transcript/alignment quality, record elapsed time and peak GPU memory, and verify that later chunks keep global timing and temporary data is removed:

```text
cargo run --locked -p storyteller-application --example transcribe_whisper -- AUDIO OUTPUT_DIRECTORY FFMPEG WHISPER_CLI TURBO_Q5_MODEL
cargo run --locked -p storyteller-application --example transcribe_whistle -- AUDIO OUTPUT_DIRECTORY FFMPEG NEEDLE WHISTLE_MODEL auto
```

Use distinct output directories. This helper checks the pinned full bundle/model and the same real-offload guard as the desktop; CPU test fixtures never satisfy product GPU readiness.

## Contextual input follow-up

The next R5 draft, `feature/contextual-audio-windows`, is stacked on the bounded-reuse draft. It separates recognition limits from application memory bounds:

| Backend | Ownership target | Largest ownership after refinement | Largest inference input | Context per side |
| --- | --- | --- | --- | --- |
| Whistle | 25 seconds | 27 seconds | 30 seconds, the engine cap | Up to 2.5 seconds, at least 1.5 at the largest ownership |
| Whisper Turbo | 5 minutes | 302.5 seconds | 307.5 seconds, an application bound | 2.5 seconds |

Whisper's native CLI accepts longer files and performs its own internal model windows. It does not inherit Whistle's 30-second input-file limit. Up to eight larger Whisper inputs still share one CLI/model context, bounding temporary mono PCM to about 78.8 MB per group. Only one GPU worker runs, and progress counts completed ownership rather than repeated context audio.

Both paths keep contiguous ownership and separately record overlapping inference windows in `transcription-plan.json`. Only native English word times are used. Whistle attention words are read directly; Whisper full JSON tokens are joined into whole words, with no segment-level timestamp guesses. Near a cut, ordered one-to-one matching compares both word identity and time, preserving intentional repetitions. A wider timing match requires a unique word and at least two nearby ordered neighbors with reliable times; its native report farthest from the input edges supplies timing. Otherwise nearby native reports are averaged. Native words wholly in Whisper's final padded region are excluded and straddling intervals are clipped to actual audio after validating complete native text and ordering. No missing word times are invented. Joint native timing decides ownership; unmatched text from another input's context cannot introduce a truncated edge fragment. Alignment still receives complete normalized phrases, and encoding uses the full source audio.

The adapter profile changes to Whistle v3 and Whisper v2, invalidating older Analyze and dependent checkpoints. Saved backend/model selection and recovery schema remain unchanged. Windows checkpoint [37249921451](https://github.com/skypie0102/storyteller-lite/actions/runs/37249921451), source `12c697890345d1d25ab3a36d897574de444f7e67`, passed formatting, strict locked core/application Clippy, all 195 tests and both native engines. Each forced 24.5-second cut retained its native baseline word once. Turbo accepted one 35-second input and returned native words after thirty seconds; two short inputs and a separate three-input context/long group each used one model load. The product generated two larger bounded inputs for 335 seconds, stopped GPU fallback and removed temporary data. Expanded Whistle used six real workers for six ownership ranges over 135 seconds. The artifact ZIP hash, paired native JSON, reported model loads, ownership/input plans and cleanup were independently inspected. [contextual-inputs.json](runtime-validation/contextual-inputs.json) retains the evidence and capture hashes. Temporary CI is removed; compiled code and scripts are unchanged by final cleanup. Progress still advances after every paired output in an eight-file group validates; GPU acceptance must include group latency. No real-GPU speed or full-book accuracy claim is made.

## Live Whisper progress draft

The next follow-up, `feature/whisper-live-progress`, reads the pinned `--print-progress` stderr protocol for known paired inputs. Percentages reset for each file, so the application maps each update to that file's owned audio and aggregates it once. Unknown, skipped, repeated or out-of-order file markers and malformed/regressing percentages cannot credit another input. The output-saving marker means inference finished; it does not validate the JSON. All paired JSON and actual CUDA model allocation still must pass before the checked-input count advances. The live bar remains below 100% until Analyze completes merging, saving and temporary cleanup. Cancellation and observer failure stop the owned process and join workers before cleanup.

CUDA failure detection and final offload proof now read anchored native stderr diagnostics only. Spoken stdout containing GPU failure phrases or diagnostic-looking text cannot stop inference or authorize CPU results. CUDA initialization and positive model allocation must refer to the same device. Recognition/cache profiles and public configuration types are unchanged. Progress adds an optional native-input measurement to keep the existing activity text live while the overall integer percentage remains unchanged. Final Windows [checkpoint 37278920610](https://github.com/skypie0102/storyteller-lite/actions/runs/37278920610), source `061a4263e3b4615411ce4128ee8653cb7a01c548`, passed strict locked checks and all 204 tests, including a two-hour book whose native part updates differ while its whole-book percent stays zero. Native [checkpoint 37277637190](https://github.com/skypie0102/storyteller-lite/actions/runs/37277637190), source `64076313f362a2c11c53bcc8fc9d89e871a61f51`, passed the pinned CLI/helper code unchanged by the final activity addition: three native input resets, an 82% update 41.61 seconds before completion on CPU, one model load, word-cut/tail coverage, common Whistle workers and stopped GPU fallback/cleanup. Full logs and guarded outcomes, artifact CRC/SHA and native JSON/events/plans were inspected and retained in [whisper-progress.json](runtime-validation/whisper-progress.json). Draft [PR #34](https://github.com/skypie0102/storyteller-lite/pull/34) remains unmerged; temporary CI is removed with compiled source unchanged. This does not establish GPU performance or full-book accuracy.
