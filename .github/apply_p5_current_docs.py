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
    "- P5 Windows packaging/runtime hardening now includes per-user owned runtime storage (`234064e2a70f4fafbab7ee34be5bc5ffbc7fbc05`), a self-describing/verified manual developer-test package (`503ada36a0f000abf9dfd581f4941d6242f7654d`), and one shared app-data-root contract across runtime setup/import/recovery (`b236c503b73fc2829ad8830d4084cd93cb0d1a22`). Automatic FFmpeg/whisper/model installs no longer require the executable directory to be writable. Final shared-root Windows run `35076642104` passed strict UI Clippy, UI tests, and the native Slint build.\n",
    "- P5 Windows packaging/runtime hardening now includes per-user owned runtime storage (`234064e2a70f4fafbab7ee34be5bc5ffbc7fbc05`), a self-describing/verified manual developer-test package (`503ada36a0f000abf9dfd581f4941d6242f7654d`), and one shared app-data-root contract across runtime setup/import/recovery (`b236c503b73fc2829ad8830d4084cd93cb0d1a22`). Automatic FFmpeg/whisper/model installs no longer require the executable directory to be writable. Final shared-root Windows run `35076642104` passed strict UI Clippy, UI tests, and the native Slint build.\n"
    "- Relaunch recovery durability now also includes unreadable-snapshot quarantine (`893bfcfa7079883e135d974f46a16720c8354087`, Windows run `35078267500`), crash-safe temp/backup publication (`b2717c08aa52477afb0e7a638bdab43a9947544d`, core run `35086646029`), and backup-aware quarantine when only an unreadable fallback remains (`5100c90ecb992b09f25751225caab55191fd5f9e`). Malformed recovery state is preserved instead of being silently overwritten or deleted.\n"
    "- StoryTeller-owned whisper.cpp acquisition is reproducible and verified: automatic Windows x64 CPU/CUDA downloads are pinned to upstream binary build `b5130` with fixed asset names and SHA-256 digests (`e34a1197c60024706e6ad862d4a508d42fecbcda`, Windows run `35087149145`), and automatic Download missing no longer scans for or silently imports unverified legacy whisper archives (`e30068e07a2e170dda689e58503fd69399cc636f`, Windows run `35097393779`). Explicit local/import/PATH runtimes remain supported.\n"
    "- Automatic FFmpeg acquisition is now pinned to Gyan/Codex FFmpeg `9.0.1` Essentials with a compiled-in SHA-256 (`b0ef2665afbedb53c5093d86acec5d6e15014cb7`, Windows run `35098920875`) rather than the moving `ffmpeg-release-essentials.zip` alias.\n"
    "- The permanent packaged relaunch smoke is wired into the developer-test Windows build. `2bd99a0f145f3c5669d8202719fdc5949d84aed6` covers interrupted Running → Waiting recovery in a paused queue (Windows run `35081323895`), and `fee9ada9dbf4b46ffe9d03796617eee6ac8ecebb` extends the same packaged smoke to prove NeedsReview rewinds before Review Audio while preserving explicit Resume queue behavior (Windows run `35179721389`).\n",
)
replace_exact(
    handoff,
    "- Temporary validation PRs #3 through #17 were closed without merge and their temporary feature workflows/scaffolding were removed after validation.\n",
    "- Temporary validation PRs #1 through #25 are closed without merge. Validated product/docs changes are copied into `recovery/rust-slint`; temporary validation workflows, patch helpers, and merge commits are not treated as implementation truth.\n",
)

roadmap = "docs/ROADMAP.md"
replace_exact(
    roadmap,
    "Two related P5 hardening slices are now integrated:\n",
    "Five related P5 hardening slices are now integrated:\n",
)
replace_exact(
    roadmap,
    "- `8c0c3d35dff44e0d616077d627a3a4e1e5c478fe` adds a versioned durable recoverable-queue snapshot and Slint-shell lifecycle integration. Interrupted Running jobs restore as Waiting; recoverable work always restores with the queue paused; terminal jobs are omitted and remove the recovery file when no work remains; malformed versions, duplicate IDs, non-contiguous checkpoints, bad stage ordering, and blank fingerprints are rejected. A job that was waiting at Review Audio is rewound before Review Audio so unresolved human review cannot be bypassed after relaunch; the existing durable review draft remains the source for manual decisions when that stage reruns.\n",
    "- `8c0c3d35dff44e0d616077d627a3a4e1e5c478fe` adds a versioned durable recoverable-queue snapshot and Slint-shell lifecycle integration. Interrupted Running jobs restore as Waiting; recoverable work always restores with the queue paused; terminal jobs are omitted and remove the recovery file when no work remains; malformed versions, duplicate IDs, non-contiguous checkpoints, bad stage ordering, and blank fingerprints are rejected. A job that was waiting at Review Audio is rewound before Review Audio so unresolved human review cannot be bypassed after relaunch; the existing durable review draft remains the source for manual decisions when that stage reruns.\n"
    "- `893bfcfa7079883e135d974f46a16720c8354087` preserves an unreadable or unsupported recovery snapshot under an `.invalid-<timestamp>` quarantine name instead of allowing the next persistence cycle to delete it. If preservation itself fails, automatic recovery writes are disabled for that app session. Windows run `35078267500` passed rustfmt, strict UI Clippy, UI tests, and the native Slint build.\n"
    "- `b2717c08aa52477afb0e7a638bdab43a9947544d` makes queue-recovery publication crash-safe: new JSON is flushed to a temp file, the prior primary is rotated to `.bak`, failed publication restores that prior snapshot when possible, and reads fall back to `.bak` when the primary is absent. Core run `35086646029` passed strict core Clippy and core tests.\n"
    "- `5100c90ecb992b09f25751225caab55191fd5f9e` closes the backup/quarantine interaction: when the primary is absent and the fallback backup is unreadable, that backup is quarantined rather than later being removed by an empty-queue save.\n",
)
replace_exact(
    roadmap,
    "Three related P5 release-hardening slices are integrated:\n",
    "Six related P5 release-hardening slices are integrated:\n",
)
replace_exact(
    roadmap,
    "- `b236c503b73fc2829ad8830d4084cd93cb0d1a22` centralizes the per-user application-data root used by automatic runtime setup, local whisper archive import, and relaunch recovery. Missing or empty `LOCALAPPDATA` no longer produces an accidental relative runtime/recovery directory. Persistent runtime installation/import fails explicitly when the per-user root is unavailable; queue recovery deliberately retains its temp-directory fallback. Final Windows run `35076642104` passed exact patch application, whitespace/rustfmt, strict `storyteller-ui` Clippy, UI tests, and the native Slint build. The preceding run `35076520019` stopped in patch scaffolding before Rust because one textual replacement count included the helper definition; the full log was inspected before the single corrected retry.\n",
    "- `b236c503b73fc2829ad8830d4084cd93cb0d1a22` centralizes the per-user application-data root used by automatic runtime setup, local whisper archive import, and relaunch recovery. Missing or empty `LOCALAPPDATA` no longer produces an accidental relative runtime/recovery directory. Persistent runtime installation/import fails explicitly when the per-user root is unavailable; queue recovery deliberately retains its temp-directory fallback. Final Windows run `35076642104` passed exact patch application, whitespace/rustfmt, strict `storyteller-ui` Clippy, UI tests, and the native Slint build. The preceding run `35076520019` stopped in patch scaffolding before Rust because one textual replacement count included the helper definition; the full log was inspected before the single corrected retry.\n"
    "- The whisper.cpp automatic-download path is pinned to upstream binary build `b5130` with fixed CPU/CUDA asset names and compiled-in SHA-256 digests (`e34a1197c60024706e6ad862d4a508d42fecbcda`, Windows run `35087149145`). `e30068e07a2e170dda689e58503fd69399cc636f` then removes automatic reuse of unverified legacy whisper archives while preserving already-extracted runtimes plus explicit Import whisper archive/PATH discovery; Windows run `35097393779` passed strict UI validation.\n"
    "- `b0ef2665afbedb53c5093d86acec5d6e15014cb7` pins automatic FFmpeg acquisition to the Gyan/Codex FFmpeg `9.0.1` Essentials ZIP and SHA-256 `fec81ae03971d9dd4be3ebe02e263bd2ec1d789483f931bdba5f5715e65da2e9`; Windows run `35098920875` passed the targeted UI/runtime checkpoint.\n"
    "- Packaged relaunch recovery is now exercised by the permanent developer-test smoke script: `2bd99a0f145f3c5669d8202719fdc5949d84aed6` proves interrupted Running work restores as Waiting and stays paused (run `35081323895`), while `fee9ada9dbf4b46ffe9d03796617eee6ac8ecebb` adds a NeedsReview seed with checkpoints through Review Audio and proves the packaged app rewinds it to Prepare/Analyze/Align before persisting Waiting state (run `35179721389`).\n",
)
replace_exact(
    roadmap,
    "Temporary validation PRs #16 and #17 were closed without merge and their temporary validation scaffolding was removed.\n",
    "Temporary validation PRs #1 through #25 are closed without merge; validated changes live on `recovery/rust-slint`, while validation-only workflows/helpers remain off the implementation branch.\n",
)
replace_exact(
    roadmap,
    "- exercise the paused relaunch-recovery contract through the developer-test package/installed-like launch conditions, including an interrupted Waiting/Running job and a NeedsReview rewind, while preserving explicit Resume queue behavior;\n",
    "- keep the packaged relaunch smoke in the manual Windows developer-test build as regression coverage for Running → Waiting recovery and NeedsReview rewind; extend it only when a new recoverable status or persistence rule changes that contract;\n",
)

for path in [handoff, roadmap]:
    text = Path(path).read_text(encoding="utf-8")
    if "Temporary validation PRs #1 through #25" not in text:
        raise SystemExit(f"{path}: validation PR checkpoint was not written")

print("P5 current documentation checkpoint applied")
