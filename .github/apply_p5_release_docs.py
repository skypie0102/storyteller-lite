from pathlib import Path


def replace_exact(path: str, old: str, new: str, expected: int = 1) -> None:
    file_path = Path(path)
    text = file_path.read_text(encoding="utf-8")
    count = text.count(old)
    if count != expected:
        raise SystemExit(f"{path}: expected {expected} occurrences, found {count}: {old!r}")
    file_path.write_text(text.replace(old, new), encoding="utf-8")


handoff = "docs/HANDOFF.md"
replace_exact(
    handoff,
    '''- P5 Windows packaging/runtime hardening now includes per-user owned runtime storage (`234064e2a70f4fafbab7ee34be5bc5ffbc7fbc05`), a self-describing/verified manual developer-test package (`503ada36a0f000abf9dfd581f4941d6242f7654d`), and one shared app-data-root contract across runtime setup/import/recovery (`b236c503b73fc2829ad8830d4084cd93cb0d1a22`). Automatic FFmpeg/whisper/model installs no longer require the executable directory to be writable. Final shared-root Windows run `35076642104` passed strict UI Clippy, UI tests, and the native Slint build.
''',
    '''- P5 Windows packaging/runtime hardening now includes per-user owned runtime storage (`234064e2a70f4fafbab7ee34be5bc5ffbc7fbc05`), a self-describing/verified manual developer-test package (`503ada36a0f000abf9dfd581f4941d6242f7654d`), and one shared app-data-root contract across runtime setup/import/recovery (`b236c503b73fc2829ad8830d4084cd93cb0d1a22`). Automatic FFmpeg/whisper/model installs no longer require the executable directory to be writable. Final shared-root Windows run `35076642104` passed strict UI Clippy, UI tests, and the native Slint build.
- Relaunch recovery durability is hardened further by unreadable-snapshot quarantine (`893bfcfa7079883e135d974f46a16720c8354087`, Windows run `35078267500`), crash-safe temp/backup publication (`b2717c08aa52477afb0e7a638bdab43a9947544d`, core run `35086646029`), and backup-aware quarantine when the primary is missing (`5100c90ecb992b09f25751225caab55191fd5f9e`). A malformed primary or fallback backup is preserved rather than silently overwritten/deleted, and publication keeps the previous valid snapshot recoverable until the new flushed snapshot is in place.
- StoryTeller-owned automatic whisper.cpp acquisition is reproducible as of `e34a1197c60024706e6ad862d4a508d42fecbcda`: Windows x64 CPU/CUDA downloads are pinned to upstream binary build `b5130` with fixed asset names and SHA-256 digests. Build `b5130` is the upstream binary build for the same source commit as stable whisper.cpp `v1.9.4`; local/imported/PATH runtimes remain allowed. Windows run `35087149145` passed strict UI Clippy, UI tests, and the native Slint build.
''',
)
replace_exact(
    handoff,
    '''- Temporary validation PRs #3 through #17 were closed without merge and their temporary feature workflows/scaffolding were removed after validation.
''',
    '''- Temporary validation PRs through #17 were closed without merge after their checkpoints; later P5 validation PRs #18, #20, and #21 were likewise closed without merge and their temporary validation scaffolding was removed.
''',
)

roadmap = "docs/ROADMAP.md"
replace_exact(
    roadmap,
    '''Two related P5 hardening slices are now integrated:

- `19ecde9b5082393a9ccb056a3abe189bfa89393a` makes worker startup validate checkpoint fingerprints and each stage's artifact manifest before cached stages are reused. The first invalid point truncates downstream checkpoints and resets execution from that stage. Core validation run `35067743420` passed rustfmt, strict `storyteller-core` Clippy, and all core tests.
- `8c0c3d35dff44e0d616077d627a3a4e1e5c478fe` adds a versioned durable recoverable-queue snapshot and Slint-shell lifecycle integration. Interrupted Running jobs restore as Waiting; recoverable work always restores with the queue paused; terminal jobs are omitted and remove the recovery file when no work remains; malformed versions, duplicate IDs, non-contiguous checkpoints, bad stage ordering, and blank fingerprints are rejected. A job that was waiting at Review Audio is rewound before Review Audio so unresolved human review cannot be bypassed after relaunch; the existing durable review draft remains the source for manual decisions when that stage reruns.
''',
    '''Five related P5 hardening slices are now integrated:

- `19ecde9b5082393a9ccb056a3abe189bfa89393a` makes worker startup validate checkpoint fingerprints and each stage's artifact manifest before cached stages are reused. The first invalid point truncates downstream checkpoints and resets execution from that stage. Core validation run `35067743420` passed rustfmt, strict `storyteller-core` Clippy, and all core tests.
- `8c0c3d35dff44e0d616077d627a3a4e1e5c478fe` adds a versioned durable recoverable-queue snapshot and Slint-shell lifecycle integration. Interrupted Running jobs restore as Waiting; recoverable work always restores with the queue paused; terminal jobs are omitted and remove the recovery file when no work remains; malformed versions, duplicate IDs, non-contiguous checkpoints, bad stage ordering, and blank fingerprints are rejected. A job that was waiting at Review Audio is rewound before Review Audio so unresolved human review cannot be bypassed after relaunch; the existing durable review draft remains the source for manual decisions when that stage reruns.
- `893bfcfa7079883e135d974f46a16720c8354087` preserves an unreadable or unsupported recovery snapshot instead of allowing the next automatic persistence cycle to delete it. The UI quarantines the file under an `.invalid-<timestamp>` name; if preservation fails, automatic recovery writes are disabled for that app session. Windows run `35078267500` passed rustfmt, strict UI Clippy, UI tests, and the native Slint build.
- `b2717c08aa52477afb0e7a638bdab43a9947544d` makes queue-recovery publication crash-safe: the new JSON is fully written and flushed to a temp file, the previous primary is rotated to `.bak`, failed publication restores that prior snapshot when possible, and reads fall back to `.bak` when the primary is absent. Core run `35086646029` passed strict core Clippy and all core tests, including interrupted-publication fallback and successful-replacement cleanup.
- `5100c90ecb992b09f25751225caab55191fd5f9e` closes the backup/quarantine interaction: when the primary is absent and the fallback `.bak` itself is unreadable, that backup is quarantined rather than later being deleted by an empty-queue save. This isolated one-file fix is statically reviewed and is covered by the accumulated P5 Windows release checkpoint rather than a separate runner.
''',
)
replace_exact(
    roadmap,
    '''Three related P5 release-hardening slices are integrated:
''',
    '''Four related P5 release-hardening slices are integrated:
''',
)
replace_exact(
    roadmap,
    '''- `b236c503b73fc2829ad8830d4084cd93cb0d1a22` centralizes the per-user application-data root used by automatic runtime setup, local whisper archive import, and relaunch recovery. Missing or empty `LOCALAPPDATA` no longer produces an accidental relative runtime/recovery directory. Persistent runtime installation/import fails explicitly when the per-user root is unavailable; queue recovery deliberately retains its temp-directory fallback. Final Windows run `35076642104` passed exact patch application, whitespace/rustfmt, strict `storyteller-ui` Clippy, UI tests, and the native Slint build. The preceding run `35076520019` stopped in patch scaffolding before Rust because one textual replacement count included the helper definition; the full log was inspected before the single corrected retry.
''',
    '''- `b236c503b73fc2829ad8830d4084cd93cb0d1a22` centralizes the per-user application-data root used by automatic runtime setup, local whisper archive import, and relaunch recovery. Missing or empty `LOCALAPPDATA` no longer produces an accidental relative runtime/recovery directory. Persistent runtime installation/import fails explicitly when the per-user root is unavailable; queue recovery deliberately retains its temp-directory fallback. Final Windows run `35076642104` passed exact patch application, whitespace/rustfmt, strict `storyteller-ui` Clippy, UI tests, and the native Slint build. The preceding run `35076520019` stopped in patch scaffolding before Rust because one textual replacement count included the helper definition; the full log was inspected before the single corrected retry.
- `e34a1197c60024706e6ad862d4a508d42fecbcda` removes moving GitHub-release selection from StoryTeller-owned whisper.cpp downloads. Automatic Windows x64 installs are pinned to upstream binary build `b5130`, using fixed CPU (`whisper-bin-x64.zip`) and CUDA 12.4 (`whisper-cublas-12.4.0-bin-x64.zip`) asset names plus their published SHA-256 digests. That binary build targets the same upstream source commit as stable whisper.cpp `v1.9.4`. Local/imported/portable/PATH discovery is unchanged. Windows run `35087149145` passed strict UI Clippy, UI tests, and the native Slint build.
''',
)
replace_exact(
    roadmap,
    '''Temporary validation PRs #16 and #17 were closed without merge and their temporary validation scaffolding was removed.
''',
    '''Temporary validation PRs #16, #17, #18, #20, and #21 were closed without merge and their temporary validation scaffolding was removed.
''',
)

runtime = "docs/RUNTIME.md"
replace_exact(
    runtime,
    '''Current download behavior and integrity checks:

- ffmpeg: Gyan Windows Essentials ZIP plus the provider's published `.sha256`; the archive hash must match before extraction.
- whisper.cpp: official `ggml-org/whisper.cpp` GitHub Windows x64 release assets; the GitHub asset must publish a `sha256:` digest and the downloaded archive must match it.
- If `nvidia-smi` confirms an NVIDIA GPU and whisper.cpp is missing, the downloader prefers an official CUDA/cuBLAS-enabled Windows x64 asset, falling back to the CPU x64 asset only if a compatible CUDA asset is unavailable.
- `large-v3-turbo`: the canonical whisper.cpp model download; the staged file must match the pinned SHA-256 before it is moved into `models/`.
''',
    '''Current download behavior and integrity checks:

- ffmpeg: Gyan Windows release-essentials ZIP plus the provider's published `.sha256`; the archive hash must match before extraction. Gyan's `ffmpeg-release-essentials.zip` is a rolling release alias, so this path is integrity-checked but not bit-reproducible across upstream rotations. Do not describe it as pinned unless Lite adopts a fixed archive identity/digest policy.
- whisper.cpp: StoryTeller-owned automatic Windows x64 downloads are pinned to upstream binary build `b5130`, which was produced from the same whisper.cpp source commit as stable `v1.9.4`. CPU uses `whisper-bin-x64.zip` with SHA-256 `f9ec6c52a2e949b62ab51fa21d0d497958f9e41c3010c157c4e42932d5316f3c`; CUDA uses `whisper-cublas-12.4.0-bin-x64.zip` with SHA-256 `af520ddd034d985b55dfeea3e465ed93653ba2aee1a55e865033edc548c272a7`. The downloaded archive must match the compiled-in digest before extraction.
- If `nvidia-smi` confirms an NVIDIA GPU and whisper.cpp is missing, automatic installation selects the pinned CUDA 12.4 asset; otherwise it selects the pinned CPU x64 asset. Local/imported/portable/PATH runtimes are still discovered independently and are not forced to this build.
- `large-v3-turbo`: the canonical whisper.cpp model download; the staged file must match the pinned SHA-256 before it is moved into `models/`.
''',
)
