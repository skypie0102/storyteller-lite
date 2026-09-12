# Whisper worker semantics recovered from old OneClick

This note answers a specific recovery question: what did the old **Parallel Whisper jobs** control actually mean, and what should the new Lite worker-count setting most likely control?

## Conclusion

The old setting was **not** whisper.cpp's internal processor count (`-p` / `--processors`). It controlled how many independently processed audiobook tracks/files could be transcribed concurrently.

For a faithful Lite replacement, the preferred design direction is therefore **Lite-managed parallel chunk/track transcription**, while keeping each whisper.cpp invocation at one processor unless deliberate testing proves another setting is safe and desirable.

## Evidence from the v0.39.0 installer

Static recovery of the old compiled launcher shows three independent settings:

- `threads` — old UI label: `Threads per Whisper job`
- `parallelTranscribes` — old UI label: `Parallel Whisper jobs`
- `parallelTranscodes` — old UI label: `Parallel FFmpeg jobs`

The launcher constructed arguments equivalent to:

```text
--threads <settings.threads>
--processors 1
--parallel-transcodes <settings.parallelTranscodes>
--parallel-transcribes <settings.parallelTranscribes>
```

The old compiled default constructor used:

- `threads = 6`
- `parallelTranscribes = 3`
- `parallelTranscodes = 6`

Those are historical OneClick defaults only. The current Lite product decision remains a worker-count default of `1` unless benchmarking deliberately changes it.

Because the launcher explicitly used constant `--processors 1` while separately forwarding `parallelTranscribes`, old Parallel Whisper jobs cannot have meant whisper.cpp processors.

## Cross-check against current upstream Storyteller

This section is an external source-code cross-check, **not proof of the exact npm implementation fetched by v0.39.0**, because the old installer invoked `@storyteller-platform/align@latest` rather than pinning a package version.

At upstream Storyteller commit `c13dd32a82388d8c3b5f39d248897d4050d5ba88`:

1. `libraries/align/src/cli/bin.ts` parses `--parallel-transcribes` separately and passes it into `transcribe(...)` as `parallelism`.
2. `libraries/align/src/transcribe/transcribe.ts` constructs an `AsyncSemaphore(options.parallelism ?? 1)`, enumerates the processed audio files, and runs one `transcribeFile(...)` task per file while the semaphore bounds how many are active concurrently.
3. Each individual `transcribeFile(...)` receives its own `processors` and `threads` values separately.
4. `libraries/align/src/process/parse.ts` has a default processed-track maximum length of 120 minutes.
5. `libraries/align/src/process/processAudiobook.ts` splits source audio into multiple processed files/ranges. It prefers chapter boundaries; overlong/no-chapter regions are further split at safe points.
6. `libraries/align/src/process/ranges.ts` uses voice-activity detection around candidate cut points when a chapter/range is too long.

Current stalign CLI help also describes `--processors` as a distinct whisper.cpp option and warns that values greater than one may affect timing accuracy.

Together with the installer evidence, this strongly supports the interpretation that **worker count = concurrently transcribed audio tracks/chunks**, not processor count inside one Whisper invocation.

## Why a single M4B could still use multiple workers

The old pipeline did not necessarily feed one monolithic M4B directly to one Whisper process.

Conceptually:

```text
M4B audiobook
      |
      v
safe processed ranges/tracks
  |       |       |       |
  v       v       v       v
Whisper  Whisper  Whisper  Whisper
worker   worker   worker   worker
  |       |       |       |
  +-------+-------+-------+
                  |
                  v
       ordered transcription set
                  |
                  v
              alignment
```

So a single long audiobook could benefit from `Parallel Whisper jobs = 3` because preprocessing yielded several ordered tracks/ranges.

## Difference from current Lite

Current recovered Lite Analyze behavior is approximately:

```text
M4B -> one full 16 kHz mono PCM WAV -> one whisper-cli invocation -> transcript.json
```

That architecture has only one transcription work item, so merely adding a `workers` integer cannot recreate the old behavior.

Passing `-p N` to that one whisper-cli process would implement a **different kind of parallelism** from the historical setting and may affect timing behavior.

## Preferred Lite implementation direction

When implementing the user-requested worker-count option, prefer this architecture unless benchmarks show a better safe design:

1. Decode/split the audiobook into deterministic ordered transcription chunks.
2. Keep boundaries safe for timestamp reconstruction — chapter boundaries where available, otherwise silence/VAD-aware boundaries or another tested deterministic policy.
3. Run at most `workers` whisper transcription tasks concurrently.
4. Keep each individual worker's whisper processor count at `1` initially.
5. Assign each chunk a known global audio offset.
6. Convert every local segment timestamp back into global audiobook time.
7. Merge chunk transcripts deterministically and validate chronology/non-overlap before publishing the single Analyze transcript artifact.
8. Preserve cancellation and progress aggregation across all active workers.
9. Persist enough chunk metadata/checkpoint data to make retry/resume deterministic.
10. Treat worker count as execution/performance configuration; it should not invalidate semantic transcript/alignment cache solely because N changed, provided the merged transcript contract is deterministic and equivalent.

## Thread budgeting still needs benchmarking

The old product allowed `threads per Whisper job` and `parallel Whisper jobs` to be configured independently. Lite intentionally does **not** restore manual CPU-thread tuning.

Therefore the new implementation needs an automatic resource policy for `workers > 1` instead of simply giving every concurrent worker all logical CPU threads.

Potential policy questions to benchmark before finalizing:

- CPU-only: divide available CPU capacity across concurrent workers or cap per-worker threads.
- CUDA/other GPU backends: determine whether concurrent whisper.cpp processes improve throughput or instead duplicate model/VRAM and become slower.
- Do not assume the old OneClick defaults (`6 threads × 3 workers`) are optimal for current Lite hardware or current whisper.cpp.

The user-facing control can stay simple even if the internal scheduling policy is hardware-aware.

## Timestamp/chunking warning

Current upstream Storyteller's 2026 transcription work also documents that aggressive whisper.cpp internal splitting/turbo parallelism can produce timestamp issues unless corrected. That reinforces why Lite should treat global timestamp reconstruction and merge validation as first-class requirements rather than merely spawning multiple Whisper instances and concatenating JSON.

## Scope guard

This note recovers the semantics of the worker setting. It does **not** imply that Lite should restore:

- manual CPU thread controls;
- parallel FFmpeg settings;
- the old Node/npx alignment launcher;
- the whole current Storyteller/stalign architecture.

Lite only needs the useful behavior: a simple worker count backed by safe parallel transcription of a long audiobook.
