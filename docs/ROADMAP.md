# Storyteller Lite roadmap

> **Canonical roadmap.** Read `docs/HANDOFF.md` before substantial work. Live Rust + Slint source on `main` is technical truth; this file records current product scope and pending work.

Storyteller Lite is the Rust + Slint successor to Storyteller OneClick. The legacy app is a behavioral reference where useful, but Lite is intentionally smaller and is not a line-for-line port.

## Product contracts

- Queue-first workflow with one foreground book pipeline at a time.
- Fixed seven-stage flow: Prepare → Analyze → Align → Review Audio → Encode → Build EPUB → Validate.
- Analyze may internally run bounded Whisper chunks concurrently.
- Failed books remain Failed/retryable and the queue continues unless explicitly paused.
- Long-running work never runs on the Slint UI thread.
- Exactly one weighted overall progress bar, backed only by real structured measurements.
- Source files are never intentionally overwritten; output defaults next to the source EPUB.
- Resume reuses only a validated contiguous prefix of checkpoints; stale semantics or invalid stage artifacts invalidate downstream stages before execution begins.
- Relaunch recovery never auto-runs recovered work. Recoverable jobs are restored into a paused queue and require an explicit Resume queue action.
- Human review decisions are explicit and durable.
- Smart decisions are conservative, auditable, and reversible before publication.
- Weak or ambiguous evidence stays unresolved instead of being forced.
- Publication happens only after independent structural validation.
- EPUB destinations are bounded/revalidated; arbitrary paths from UI state are never authoritative.
- Recovery Actions are intentionally opt-in/manual-only. GitHub-hosted runners may be used as deliberate validation checkpoints; after a failure, inspect the full log/failure surface, batch fixes, and avoid repeated one-error-at-a-time runner cycles.

## Scope kept in Lite

Keep queue-first processing, native Rust + Slint, automatic CPU-thread selection, one simple Whisper worker count (1–4, default 1), Smart/ReviewAll unmatched-audio policy, conservative edge handling, bounded/lazy OCR, reduced manual allocation, and only the destination/classification types that materially change output behavior.

Do not restore manual CPU allocation, word-level synchronization, the old activity-console-first UX, Runtime Health, process-now, broad runtime-path controls, EPUB standardization/CSS editor toggles, permanent OCR controls, or the legacy general-purpose split/merge/trim/rules allocator.

## Current implementation status

### P0/P1 foundations — complete

The Rust workspace, queue/progress/resume/scheduler/worker architecture, cancellation, source fingerprinting/staging, and seven-stage runner are implemented. Queue auto-advance after failures matches the recovered product behavior.

Analyze uses deterministic bounded audio chunks rather than a permanent full-book PCM artifact. Chapter boundaries are preferred, synthetic boundaries are silence-refined, at most the selected 1–4 Whisper processes run concurrently, available logical CPU threads are divided across active workers, and chunk-local timestamps are merged into one validated global transcript. Temporary PCM chunks are removed after use.

Windows validation for P1: run `34732299289`.

### Align — implemented

The conservative monotonic block-safe aligner uses Whisper timestamps as timing authority, accepts strong monotonic EPUB matches, prevents accepted matches from crossing normalized XHTML block boundaries, and leaves weak evidence unmatched.

### P2 Review Audio — functionally complete

Implemented:

1. Durable per-segment Pending / Assigned / Excluded decisions with Automatic / Manual provenance and restart-safe `review-draft.json` persistence.
2. Reduced Smart / ReviewAll policy wiring.
3. Native leading/trailing edge identity and bounded silence evidence.
4. Monotonic bounded EPUB text candidates plus manual text assignment/exclusion.
5. Reduced allocator navigation, preview/seek, transcript/timing/context, assign/exclude, and completion gating.
6. Effective reviewed text alignment materialization while retaining original automatic alignment as source truth.
7. Manual Introduction/Credits preservation through generated supplemental XHTML + SMIL pages.
8. Conservative Smart edge preservation: anchored leading/trailing narration becomes supplemental Introduction/Credits in Smart; ReviewAll leaves it Pending; Smart never auto-discards unmatched audio.
9. Dedicated supplemental/Smart regression coverage through final validation.
10. Bounded EPUB image candidate discovery, including image-only spine documents, monotonic neighboring bounds, default 12-document / 24-image caps, manifest resource validation, 25 MiB ceiling, and embedded `alt` / `title` / SVG text hints.
11. Lazy per-candidate image text evidence with embedded hints first and optional per-call Tesseract fallback; no permanent OCR setting.
12. Deterministic transcript-to-image evidence scoring with conservative phrase/coverage/similarity/distinctive-word thresholds and ambiguity margin.
13. Smart high-confidence Graphic Readout assignment for pending non-edge items only, with manual decisions preserved, ReviewAll kept non-automatic, and duplicate target ownership rejected.
14. Graphic Readout EPUB rendering to the existing image target using native Media Overlay cues. Text and image narration on the same XHTML share one SMIL sequence; cues are audio-time ordered and overlap/duplicate targets are rejected.
15. Manual Graphic Readout allocation for unresolved/ambiguous non-edge narration. The allocator exposes only bounded discovered image candidates; core reruns discovery before persistence, rejects edge narration and duplicate image ownership, and records Manual provenance.
16. Job-specific allocator candidate caching so image discovery is not repeated by the UI refresh loop or leaked across books; image discovery remains advisory and cannot erase text choices.
17. End-to-end Graphic Readout coverage for both automatic mixed text/image overlays and manual bounded-image rediscovery → durable decision → EPUB build → final structural validation.

Validation history:

- supplemental rendering: `34738027167`;
- Smart edge preservation: `34745516558`;
- supplemental/Smart regression: `34746062156`;
- bounded image discovery: `34811582552`;
- lazy image evidence: `34815433905`;
- deterministic image scoring: `34820045285`;
- Smart Graphic Readout assignment + rendering: `34826426511`;
- manual Graphic Readout allocation: final green run `34918789594`.

The automatic Graphic Readout slice was integrated as `9673dc7a6f41493e13b24724b3123659fcbaa271`. Manual bounded Graphic Readout allocation is integrated as `a27df630a8626d1c0ba846deca8f44acde125fe8`.

### Encode — implemented

Copy mode performs cancellable byte-preserving copy only for safely mappable EPUB audio types. Opus/AAC use FFmpeg structured progress. `encoded-audio.json` records the packaged audio identity.

### Build EPUB — text, supplemental edge pages, and Graphic Readout implemented

For supported EPUB 3 sources without pre-existing Media Overlays, the builder preserves unrelated resources, creates Media Overlay/package relationships, injects deterministic XHTML/image anchors as needed, writes real audiobook clip times, and embeds the encoded audio.

Build consumes:

- effective reviewed text alignment;
- supplemental Introduction/Credits review destinations;
- Graphic Readout image destinations directly from review data.

Graphic Readout can share an XHTML/SMIL sequence with normal text cues or create a section overlay for an image-only page. Duplicate image assignment and overlapping audio ownership fail explicitly.

### Validate — implemented

Validate independently reopens the candidate EPUB and audits package/SMIL/text-or-image/audio relationships, clip ranges, duration consistency, duplicates, resource resolution, Media Overlay sequence text references, and mimetype rules before publication. New rendering paths must extend regression coverage rather than weakening this audit.

## Extra Audio — no renderer planned by default

`ExtraAudio` exists in the small classification enum, but recovered evidence does not establish a distinct required Lite output contract. A historical standalone audio-player page is only a possible product/interoperability fallback, not compatibility debt. Do **not** add a player-page/Extra Audio renderer merely because the old application had an `other` category. Add behavior only when there is a concrete user-facing requirement and a validation strategy.

P2 should now be treated as functionally complete unless real books expose another narrowly scoped unmatched-audio gap.

## P3 — main Slint UI alignment — functionally complete

The supported 820×620 window now implements the intended Lite hierarchy without restoring the historical editor complexity:

- review presentation clearly represents mixed bounded text/image destinations and assign/exclude-and-advance behavior;
- seven-stage visualization shows real elapsed time per stage when available;
- creation source pickers/options reflow below 900px while preserving the wide-window hierarchy;
- active `Queue Another Book`, review headings/actions, bottom active-job actions, and queue/recent entries also reflow below 900px;
- `NeedsReview` is promoted to a dedicated `REVIEW AUDIO` surface and suppresses unrelated processing/queue-another noise;
- review content scrolls locally at short window heights, and queue/recent is hidden while review is active so bounded decisions remain usable at the 620px minimum height;
- queue/recent uses a local `ListView` for unbounded waiting/history rows;
- Settings keeps its title/back and action footer fixed while its long runtime body scrolls, and worker/runtime/action controls reflow below 900px;
- long top-level status text is bounded/elided instead of competing with the app title;
- only real timing/backend/model/match/current-activity data is shown; no synthetic metrics were added.

Validation runs:

- review presentation: `34919543203`;
- stage elapsed timing: `34926648718`;
- responsive creation layout: `34927154367`;
- responsive active/review/queue layout: `34927928639`;
- dedicated review presentation: `34952813936`;
- queue/recent local scrolling: `34953863269`;
- Settings local scrolling: `35058804437`;
- compact review/settings/header polish: `35059503255`.

Integrated recovery commits for those slices are:

- `1a645685549a0796b960d18723bb0cd42799c79d`;
- `c79c834328edabc4fef72d0d0cd5fb8343499411`;
- `c8a6461eaeb4d2535af26172876b729cf0d2abf5`;
- `17ef8d7948a04d74fd1f0ea3a60a0e8da140e8b8`;
- `1602021a9c52eadb66fa7bc73937e56dacfd4c57`;
- `26d11580e328da36d7a6169be22ec91e2ef3a157`;
- `e98adae2dc0fff77923fd62d02768c9b01609e8f`;
- `fe5b140b2319229b4083c6aa4d1c7d386a604bbd`.

These UI-only slices were validated with the relevant native gate, `cargo build -p storyteller-ui`, rather than repeatedly spending full workspace test runs when no Rust/backend behavior changed.

Keep the current 820px minimum width unless real use justifies and validates a narrower contract. Lowering it is not required to complete P3.

## P4 — installer archaeology only when needed

Inspect the supplied v0.39.0 installer only when a live Lite behavior is still ambiguous. Verify documented provenance/hash first and record evidence before implementation. Do not broaden scope simply because a legacy feature exists.

## P5 — interoperability and release polish — active

### External EPUB interoperability baseline — implemented

The first P5 slice is integrated as `ef7900ceb5efd3541d8b33f056d3f3ff8d9920e8` and validated on Linux in run `35065877026`.

It adds a development-only fixture exporter that produces three representative books through the real builder and internal validator:

- normal text Media Overlay;
- supplemental Introduction/Credits pages;
- mixed text + Graphic Readout Media Overlay.

A manual-only `.github/workflows/epubcheck-validation.yml` gate runs rustfmt, strict `storyteller-core` Clippy, core tests, fixture export, a pinned EPUBCheck 5.4.0 download, SHA-256 verification (`33350c61038e71dfb3d45a76aed04bf5481e6d5500cb780f6e98db8bbd15a28c`), and EPUBCheck across all three exported books. Java/EPUBCheck are development tooling only and are not runtime dependencies or substitutes for Lite's internal publication validator.

The first external run exposed a real conformance defect: Storyteller-generated SMIL `<seq>` elements lacked required `epub:textref`. All text, supplemental, and mixed Graphic Readout generation paths now emit the required reference, and the internal validator independently requires/resolves it so the same class of defect is caught before publication. The final run passed EPUBCheck 5.4.0 on all three fixtures.

### Thorium reading-system import/open baseline — implemented

A manual-only `.github/workflows/thorium-validation.yml` gate now exercises the same three exported fixtures in a real EPUB reading system. It downloads the official Thorium Reader 3.5.1 Linux amd64 Debian package and verifies SHA-256 `72ab951d4963500b68c91a6496c662f30698dc27422a1b1af720b1cfbdfd327e`, installs Thorium plus Xvfb, and runs `.github/scripts/thorium-reader-smoke.sh` with a fresh isolated profile per fixture.

The smoke requires Thorium to remain alive through the observation window and to persist an imported publication for each text-overlay, supplemental-edge, and Graphic Readout fixture. Validation run `35220207088` passed all three cases. This is intentionally narrower than playback validation: it proves real-reader import/open compatibility, but it does **not** claim automated audible Media Overlay playback, timing, or synchronized-highlight verification.

A subsequent manual check of the three published interoperability EPUB artifacts reported that all three work normally. Reader/version details were not recorded, so this is manual acceptance evidence rather than a reproducible reader-specific gate.

### Validated checkpoint resume and relaunch recovery — implemented

Five related P5 hardening slices are now integrated:

- `19ecde9b5082393a9ccb056a3abe189bfa89393a` makes worker startup validate checkpoint fingerprints and each stage's artifact manifest before cached stages are reused. The first invalid point truncates downstream checkpoints and resets execution from that stage. Core validation run `35067743420` passed rustfmt, strict `storyteller-core` Clippy, and all core tests.
- `8c0c3d35dff44e0d616077d627a3a4e1e5c478fe` adds a versioned durable recoverable-queue snapshot and Slint-shell lifecycle integration. Interrupted Running jobs restore as Waiting; recoverable work always restores with the queue paused; terminal jobs are omitted and remove the recovery file when no work remains; malformed versions, duplicate IDs, non-contiguous checkpoints, bad stage ordering, and blank fingerprints are rejected. A job that was waiting at Review Audio is rewound before Review Audio so unresolved human review cannot be bypassed after relaunch; the existing durable review draft remains the source for manual decisions when that stage reruns.
- `893bfcfa7079883e135d974f46a16720c8354087` preserves an unreadable or unsupported recovery snapshot under an `.invalid-<timestamp>` quarantine name instead of allowing the next persistence cycle to delete it. If preservation itself fails, automatic recovery writes are disabled for that app session. Windows run `35078267500` passed rustfmt, strict UI Clippy, UI tests, and the native Slint build.
- `b2717c08aa52477afb0e7a638bdab43a9947544d` makes queue-recovery publication crash-safe: new JSON is flushed to a temp file, the prior primary is rotated to `.bak`, failed publication restores that prior snapshot when possible, and reads fall back to `.bak` when the primary is absent. Core run `35086646029` passed strict core Clippy and core tests.
- `5100c90ecb992b09f25751225caab55191fd5f9e` closes the backup/quarantine interaction: when the primary is absent and the fallback backup is unreadable, that backup is quarantined rather than later being removed by an empty-queue save.

The UI bridge loads recovery once and checkpoints recoverable queue state at a throttled one-second cadence rather than every 100 ms UI poll. The recovery file is stored under the existing app data root as `queue-recovery.json` (`%LOCALAPPDATA%\Storyteller OneClick Lite` on Windows, with the existing temp-directory fallback where persistent app data is unavailable).

Final Windows validation run `35068910791` passed rustfmt, strict workspace Clippy, all workspace tests, and the native Slint build. Temporary validation PRs #14 and #15 were closed without merge and their temporary validation scaffolding was removed.

### Windows runtime storage and developer-test packaging — hardened

Six related P5 release-hardening slices are integrated:

- `234064e2a70f4fafbab7ee34be5bc5ffbc7fbc05` moves StoryTeller-owned automatic FFmpeg, whisper.cpp, and Whisper-model installs out of the executable directory and into `%LOCALAPPDATA%\Storyteller OneClick Lite\tools` / `models`. Portable-adjacent runtimes, explicit environment overrides, PATH discovery, imported whisper builds, and bounded legacy discovery remain supported. The substantive Windows gates in run `35074193551` passed; its final bookkeeping push failed only because the Actions token could not update a workflow file. Bookkeeping-only persistence run `35075247777` then stored the exact validated Rust/docs without recompiling.
- `503ada36a0f000abf9dfd581f4941d6242f7654d` strengthens the manual Windows developer-test package without creating a public installer/update channel. Packaging uses `cargo build --locked --release`, emits `BUILD.json` with product/package/version/commit/actual Rust host target/executable/SHA-256, independently checks `SHA256SUMS.txt`, rejects unexpected package files, and publishes the short-lived artifact as `StoryTeller-Lite-Windows-x64-Developer-Test`. `Cargo.lock` is explicitly part of the application build contract. This workflow/docs-only slice was statically reviewed and did not spend another hosted compile run.
- `b236c503b73fc2829ad8830d4084cd93cb0d1a22` centralizes the per-user application-data root used by automatic runtime setup, local whisper archive import, and relaunch recovery. Missing or empty `LOCALAPPDATA` no longer produces an accidental relative runtime/recovery directory. Persistent runtime installation/import fails explicitly when the per-user root is unavailable; queue recovery deliberately retains its temp-directory fallback. Final Windows run `35076642104` passed exact patch application, whitespace/rustfmt, strict `storyteller-ui` Clippy, UI tests, and the native Slint build. The preceding run `35076520019` stopped in patch scaffolding before Rust because one textual replacement count included the helper definition; the full log was inspected before the single corrected retry.
- The whisper.cpp automatic-download path is pinned to upstream binary build `b5130` with fixed CPU/CUDA asset names and compiled-in SHA-256 digests (`e34a1197c60024706e6ad862d4a508d42fecbcda`, Windows run `35087149145`). `e30068e07a2e170dda689e58503fd69399cc636f` then removes automatic reuse of unverified legacy whisper archives while preserving already-extracted runtimes plus explicit Import whisper archive/PATH discovery; Windows run `35097393779` passed strict UI validation.
- `b0ef2665afbedb53c5093d86acec5d6e15014cb7` pins automatic FFmpeg acquisition to the Gyan/Codex FFmpeg `9.0.1` Essentials ZIP and SHA-256 `fec81ae03971d9dd4be3ebe02e263bd2ec1d789483f931bdba5f5715e65da2e9`; Windows run `35098920875` passed the targeted UI/runtime checkpoint.
- Packaged relaunch recovery is now exercised by the permanent developer-test smoke script: `2bd99a0f145f3c5669d8202719fdc5949d84aed6` proves interrupted Running work restores as Waiting and stays paused (run `35081323895`), while `fee9ada9dbf4b46ffe9d03796617eee6ac8ecebb` adds a NeedsReview seed with checkpoints through Review Audio and proves the packaged app rewinds it to Prepare/Analyze/Align before persisting Waiting state (run `35179721389`).

All temporary validation PRs through this checkpoint are closed without merge; validated changes live on `main`, while validation-only workflows/helpers remain off the implementation branch.

### Public portable Windows release path — implemented

A version-gated `.github/workflows/release.yml` path publishes a portable Windows x64 release from an explicit `release/vMAJOR.MINOR.PATCH` branch only. The workflow requires the branch/tag version to match both Cargo packages, reruns formatting, strict workspace Clippy, all workspace tests, a locked release build, package-provenance/hash checks, and the packaged relaunch-recovery smoke before publication. It produces a flat ZIP containing the executable, `BUILD.json`, README, and executable checksum plus a separate ZIP SHA-256 asset. The executable is currently unsigned; no installer or auto-update channel is part of this contract.

### P5 next work

Continue with reading-system and packaged-build interoperability rather than adding more P2/P3 feature scope:

- keep the existing Thorium 3.5.1 import/open smoke and the successful three-artifact manual acceptance check as sufficient interoperability evidence for now; do not add more reader-test infrastructure unless a concrete compatibility problem or release requirement appears;
- keep the packaged relaunch smoke in the manual Windows developer-test build as regression coverage for Running → Waiting recovery and NeedsReview rewind; extend it only when a new recoverable status or persistence rule changes that contract;
- keep the Windows developer-test package manual/short-lived for ad hoc checks; the concrete public-release requirement is now satisfied by a separate version-gated portable Windows x64 release workflow, without adding an installer or auto-update channel;
- the public portable release path must preserve the per-user runtime root and package provenance/integrity contract; owned mutable runtime/model data must not move back beside the executable.

Later evidence work:

- benchmark 1–4 Whisper workers across representative CPU/CUDA systems before changing defaults or adding VRAM/hardware-aware clamping.

## Engineering migration rule

Preserve old material until replacement behavior is implemented and regression-covered.

> **Replace → regression-test → delete. Never delete → hope we remembered everything.**
