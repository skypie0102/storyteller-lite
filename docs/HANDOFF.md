# Agent handoff — Storyteller OneClick Lite recovery

Read this file before making substantial changes.

## Current repository truth

- Repository: `skypie0102/storyteller-lite`
- Active recovered code branch: `recovery/rust-slint`
- Default branch `main` is **not** the Rust + Slint implementation branch and must not be used as current code truth.
- P1 Whisper chunk workers are integrated and Windows-validated.
- P2 now includes durable per-segment review decisions, the reduced allocator foundation, native edge/silence evidence, real supplemental Introduction/Credits rendering, behaviorally distinct Smart/ReviewAll edge handling, dedicated supplemental regression coverage, bounded EPUB image candidate discovery, lazy per-candidate image text evidence, and deterministic transcript-to-image evidence scoring.
- Supplemental edge-page rendering passed Windows validation on run `34738027167`; validated source commit `cea590ada02c7942a8fda7226631f22d503e55b1` is included in recovery.
- Smart edge preservation passed Windows validation on run `34745516558`; validated source commit `b868eb0cae9c5916ae1048c18cd2c66bf8f6ee8c` is included in recovery.
- Supplemental/Smart end-to-end regression coverage passed Windows validation on run `34746062156`.
- Bounded image candidate discovery passed Windows validation on run `34811582552`; validated source commit `20954ba2c128e7c396e870b2fb5a9f382490450e` is included in recovery.
- Lazy image text evidence passed check-only Windows validation on run `34815433905`; validated feature source commit `734ef8461e0b0245d4a6700c3fed471003226d4a` is integrated in recovery as commits `4f25486ccdc46dbd73a0924a8f224a0c69ecec2c` and `10f0b8308f9fa6b04dcfd681c9792802804bdcbb`.
- Deterministic image-evidence scoring passed Windows validation on run `34820045285`; validated feature source commit `40c5172c35c43a1a5eb9fa2e07fabbc8e68f4ed6` is integrated in recovery as commits `fa22c918157c7f18ed8b718d62a0c183b4959310` and `d8d728d2b73db793e668c62a4c733f893e61ae57`.

Historical branch names and SHAs in recovered transcripts are clues only. Inspect the live branch before relying on them.

## Product identity

Storyteller Lite is the Rust + Slint successor to Storyteller OneClick. The pre-Rust application is a behavioral reference where useful, but Lite is deliberately smaller and must not become a line-for-line port.

Primary references:

1. `docs/ROADMAP.md` — canonical current roadmap/product scope.
2. `docs/ui-guides/README.md` + mockups — current visual/product references.
3. `docs/RUNTIME.md` — owned external runtime behavior.
4. `docs/recovery/README.md` — provenance and authority order.
5. `docs/recovery/WORKER_SEMANTICS.md` — recovered transcription concurrency semantics.
6. `docs/recovery/UNMATCHED_AUDIO_RECOVERY.md` — Smart/edge/lazy-OCR recovery.
7. `docs/recovery/ALLOCATOR_OUTPUT_RECOVERY.md` — downstream Introduction/Credits/Graphic Readout semantics.
8. `docs/recovery/QUEUE_FAILURE_RECOVERY.md` — queue auto-advance behavior after failure.
9. `docs/recovery/INTEGRITY_RECOVERY.md` — legacy finishing/integrity behavior, including the 1 ms zero-length SMIL repair.
10. `docs/recovery/FRONTEND_AND_CONCURRENCY_RECOVERY.md` — old frontend state, persistence, and allocator invariants.
11. `docs/recovery/INSTALLER_DISSECTION.md` — static findings from v0.39.0.
12. `tools/recovery/extract_legacy_nsis.py` and `tools/recovery/extract_tauri_assets.py` — reproducible recovery tools.

Installer archaeology is **demand-driven**. Do not spend time recovering unrelated EXE details unless a live Lite behavior remains ambiguous.

## User decisions that remain authoritative

### Keep / restore

- Queue-first workflow.
- Fixed seven-stage pipeline: Prepare → Analyze → Align → Review Audio → Encode → Build EPUB → Validate.
- Exactly one overall progress bar backed by real metrics.
- Automatic CPU-thread selection for Whisper.
- A simple user-facing **Whisper worker count**, default `1`, range `1–4`.
- Worker count means **concurrent bounded transcription chunks**, not manual CPU allocation and not whisper.cpp `-p`.
- **Smart / ReviewAll** unmatched-audio policy.
- Conservative automatic edge handling for safe high-confidence cases only.
- Lazy/on-demand OCR of bounded EPUB image candidates; no permanent OCR toggle.
- Reduced manual allocation for unresolved audio.
- Limited useful dispositions such as Introduction, Credits, Graphic Readout, and Extra Audio only where they affect build behavior.
- Failed books remain failed/retryable but do not stall the queue unless the queue was explicitly paused or Pause-after-current was requested.
- Supplied mockups as UI hierarchy guides.

### Do not restore by default

- Manual CPU/thread allocation UI.
- Full old OneClick settings complexity.
- Word-level synchronization.
- Activity-console-first UI.
- Runtime Health page.
- Process-now flow.
- Engine/runtime path tuning UI.
- Standardize-EPUB toggle.
- CSS editor.
- Permanent OCR toggle.
- Arbitrary split/merge/trim/rules/general-purpose allocator complexity.

## Current code behavior

### P0/P1 — complete and Windows-validated

Analyze uses bounded ordered audio chunks instead of a permanent full-book PCM artifact.

Current Analyze behavior:

- `JobSettings` carries `whisper_workers`, validated to `1..=4`, default `1`.
- The Settings UI exposes only that worker selector; there is no manual CPU allocation control.
- Worker count is snapshotted per queued job and excluded from semantic resume fingerprints.
- ffmpeg provides duration/chapter metadata; no ffprobe dependency was added.
- Chapter boundaries are preferred; synthetic boundaries are refined around nearby silence.
- Each active chunk is temporarily decoded to 16 kHz mono signed-16-bit PCM and passed to its own whisper.cpp CLI process.
- At most `whisper_workers` chunks are transcribed concurrently.
- Each Whisper invocation receives an automatically divided `-t` CPU-thread budget. Worker count is not mapped to `-p`.
- Chunk-local timestamps are converted to global audiobook time, merged, ordered, and validated.
- Temporary PCM chunks are deleted after Analyze; durable artifacts are `book-corpus.json`, `transcription-plan.json`, and `transcript.json`.
- User cancellation and internal sibling cancellation remain distinct.
- Analyze fingerprinting includes ffmpeg/whisper executable identity plus language/model identity.

P1 Windows validation run: `34732299289`.

Known caveat: multiple GPU-backed Whisper processes may each load the model. Keep the default at 1 until real CPU/CUDA benchmarking justifies another default or hardware-aware clamping.

### Review Audio — substantial P2 foundation is implemented

The old handoff statement that Review Audio was global-exclusion-only is obsolete.

Current review data model:

- `AudioReviewPolicy`: `Smart` / `ReviewAll`.
- `AudioReviewDecision`: `Pending`, `Assigned`, `Excluded`.
- Assigned/excluded decisions preserve `Automatic` / `Manual` provenance.
- Optional classifications: Introduction, Credits, Graphic Readout, Extra Audio.
- Stable per-segment review IDs are derived from alignment index/time/transcript.
- `review.json` stores transcript/timing, suggestion, edge identity, optional silence evidence, and decision.
- `review-draft.json` persists decisions outside the disposable Review Audio stage directory and is restored when review data is regenerated.
- The legacy global exclusion bit remains only for compatibility; new work should use explicit per-segment decisions.

Current allocator/core behavior:

- Previous/next segment navigation.
- Audio preview/stop and ±5 s seek.
- Transcript/timing plus edge/silence context.
- Bounded EPUB text candidates ranked by lexical overlap.
- Candidate range is constrained between nearest accepted alignment neighbors so manual assignment cannot break monotonic reading order.
- Explicit text Assign and Exclude actions; after a decision the controller selects the next Pending item.
- Review cannot finish while any item is Pending.
- Effective alignment materialization converts validated manual text assignments into matched blocks while retaining the original automatic alignment as source truth.
- Bounded image candidate discovery reads the EPUB package/spine directly, so image-only XHTML pages remain visible even when the text corpus omits them. Candidate work is bounded between neighboring accepted alignment anchors, defaults to at most 12 nearby spine documents / 24 images, requires manifest-declared image resources, skips empty/missing/images over 25 MiB, and gathers `alt`, `title`, SVG `title`/`desc`/`text` hints before OCR.
- Lazy image text evidence is available for one already-bounded candidate at a time. Embedded EPUB hints are normalized and preferred without opening the image or invoking OCR. If hints are absent or normalize to no usable evidence, the caller may explicitly supply a Tesseract executable; otherwise the API returns no OCR evidence instead of turning Tesseract into a permanent runtime dependency or setting.
- The OCR fallback extracts only the selected manifest image, rechecks its discovered byte size, enforces the 25 MiB bound, copies with cancellation support, invokes Tesseract TSV output, and returns normalized text lines plus mean confidence when evidence clears conservative minimums.
- Deterministic image-evidence scoring is now available through `review_image_matches`. It normalizes the unmatched transcript once, evaluates embedded evidence across the already-bounded image candidates first, and avoids opening the EPUB image or launching OCR when a strong unambiguous embedded winner already exists.
- If embedded evidence does not produce a recommendation and the caller supplies optional Tesseract, OCR is attempted only for candidates that produced no usable embedded evidence. Empty transcripts short-circuit without OCR.
- Evidence phrase bounds use actual word count (`5..=120`) while lexical overlap uses distinct normalized words. Qualification requires conservative coverage/aggregate score thresholds plus at least two distinctive word matches; exact source/spine/image tie-breaks are deterministic and embedded evidence wins an exact source tie.
- A winner is recommended only when it qualifies and is separated from the next ranked candidate by the configured margin; a close runner-up can therefore keep the result ambiguous even when that runner-up falls just below qualification.
- The scorer returns ranked evidence and an optional recommendation only. It is **not yet wired into Smart review decisions**, and no Graphic Readout destination is created automatically yet.

Current edge evidence and Smart behavior:

- Unmatched segments before the first matched segment are marked Introduction candidates; those after the final matched segment are marked Credits candidates.
- Only those already-bounded edge segments are probed with FFmpeg `silencedetect`.
- Review evidence currently uses approximately `-38 dB` and `0.35 s`, matching the recovered manual-inspection evidence.
- Silence percentage is advisory. Probe failure does not erase review work and silence alone does not authorize deletion.
- `Smart` now automatically preserves leading/trailing unmatched narration as supplemental Introduction/Credits pages when there is a concrete matched EPUB anchor.
- `ReviewAll` leaves those same edge regions Pending for user inspection/override.
- Smart is deliberately non-destructive here: it never auto-excludes these unmatched edge segments.
- Existing **manual** draft decisions always win over Smart. Existing **automatic** decisions are recomputed according to the active policy, so a ReviewAll rebuild cannot silently retain a previous Smart assignment.
- Internal/non-edge unmatched segments remain Pending; Smart does not invent a destination for them.

### Supplemental Introduction/Credits rendering — implemented and Windows-validated

A leading/trailing review item can now be preserved as a real read-aloud page instead of being forced onto an existing paragraph. Preservation can be manual or, in Smart mode, automatic for anchored edge narration.

Behavior:

- Introduction creates a supplemental destination anchored **before** the first matched EPUB spine document.
- Credits creates a supplemental destination anchored **after** the last matched EPUB spine document.
- The durable destination model records `BeforeAnchor` / `AfterAnchor` separately from normal text/image destinations.
- Supplemental decisions stay out of ordinary text alignment materialization; the EPUB builder consumes them separately from `review.json`.
- Build EPUB generates one XHTML page and one SMIL overlay for each supplemental item, using the real transcript text and real audiobook clip interval.
- The generated XHTML and SMIL are added to the OPF manifest, the page receives `media-overlay`, a spine itemref is inserted around the chosen anchor, and per-overlay duration metadata is written.
- Total Media Overlay duration includes supplemental clips.
- Build EPUB semantic fingerprint is `storyteller:epub-media-overlay-v2-supplemental-edge`, so older build checkpoints are not reused silently.
- The allocator still exposes **Preserve as Introduction** / **Preserve as Credits** for corresponding edge candidates so ReviewAll/manual override remains available.

Supplemental rendering validation run `34738027167` passed:

- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo build -p storyteller-ui`

Validated supplemental source commit: `cea590ada02c7942a8fda7226631f22d503e55b1`.

Smart edge-policy validation run `34745516558` passed the same three gates. Validated Smart source commit: `b868eb0cae9c5916ae1048c18cd2c66bf8f6ee8c`.

Dedicated supplemental/Smart regression coverage passed Windows validation on run `34746062156`. It verifies OPF manifest/spine ordering, generated XHTML+SMIL relationships, real clip timing and duration metadata, durable review-draft restoration, a mixed automatic Introduction/manual Credits decision set, and final independent EPUB validation.

Bounded image candidate discovery passed Windows validation on run `34811582552` (format check, strict Clippy, full workspace tests, native Slint build). Validated image-discovery source commit: `20954ba2c128e7c396e870b2fb5a9f382490450e`.

Lazy image text evidence passed Windows validation on run `34815433905` using committed-source `cargo fmt --all -- --check`, strict Clippy, full workspace tests, and the native Slint build. The validated implementation prefers embedded hints and makes optional Tesseract a per-call fallback rather than a permanent Lite runtime setting.

Deterministic image-evidence scoring passed Windows validation on run `34820045285` using committed-source `cargo fmt --all -- --check`, strict Clippy, full workspace tests, and the native Slint build. The validated implementation prefers embedded evidence, invokes optional OCR only after embedded evidence fails to produce a strong unambiguous recommendation, keeps close competing candidates ambiguous, and does not create a review decision by itself.

### Build/Validate boundaries that remain

- Image destinations intentionally still fail with an explicit error: Graphic Readout rendering is not implemented yet.
- Extra Audio classification exists but has no separate Lite rendering rule yet.
- Final Validate already independently reopens the candidate and audits package/SMIL/text/audio relationships, timing, duration consistency, duplicates, and mimetype rules. Extend regression coverage as new destination types land rather than weakening this audit.

Queue failure behavior already matches recovered OneClick: terminal worker results advance to the next waiting job unless pause-after-current has paused the queue.

## Installer recovery facts worth preserving

- Old OneClick had separate `threads`, `parallelTranscribes`, and `parallelTranscodes` settings.
- Historical validation allowed 1–32 CPU threads, 1–4 parallel transcription jobs, and 1–8 parallel FFmpeg jobs; historical backend defaults are not Lite compatibility requirements.
- Old alignment launched Whisper with `--processors 1` separately from `--parallel-transcribes`, proving parallel transcribes was higher-level work concurrency rather than whisper.cpp processor count.
- Old manual allocation persisted `manualDrafts`; durable review decisions were separate from disposable/rebuildable preview workspaces. Lite preserves this boundary with `review-draft.json`.
- Historical OCR was bounded/lazy: narrow nearby candidates first, embedded text hints where possible, OCR only when needed.
- Old edge handling refined already-safe boundaries around transcript/silence evidence; silence detection alone was not proof that narration could be discarded.
- Introduction/Credits could become supplemental XHTML+SMIL pages; Lite now implements both manual and conservative Smart preservation through that reduced rendering path.
- Graphic Readout narration could attach to an existing image page when validated; deterministic image matching now exists in Lite, while the assignment/rendering path remains pending.
- Old finishing logic required complete non-overlapping coverage and explicit targets, followed by a final independent audit.
- Old zero-length SMIL repair was narrowly `clipEnd = clipBegin + 0.001s`; the final audit could still reject invalid/overlapping output.
- The recovered GPL/Sigil-derived helper is a behavioral/test-vector reference unless licensing for direct reuse is deliberately resolved.

## Immediate implementation order

### P2 — finish Smart unmatched-audio behavior

Do these next, in order unless a concrete failure requires a smaller prerequisite:

1. **Wire the validated image scorer into high-confidence Graphic Readout assignment**. Use only a strong unambiguous recommendation, create a validated image/page destination, preserve manual decisions over Smart, prevent overlapping/double allocation, and leave weak/ambiguous cases Pending.
2. **Extend Build EPUB for Graphic Readout** and add corresponding validation fixtures.
3. Decide whether **Extra Audio** needs a distinct Lite destination/rendering rule. If it does not, do not broaden the taxonomy just because the old app had more modes.
4. Keep weak/ambiguous/non-edge regions Pending unless a deterministic auditable rule is added and regression-tested.

Historical classifier thresholds in `docs/recovery/UNMATCHED_AUDIO_RECOVERY.md` are useful test vectors, not mandatory Lite constants. Reimplement/test behavior in Rust rather than copying the GPL helper.

Do **not** initially add arbitrary split/merge, a general waveform trim editor, broad Apply-to-similar rules, permanent OCR controls, or the unrestricted old allocator taxonomy.

### P3 — align Slint UI with supplied mockups

Use the main mockup for hierarchy/information density and the allocator mockup for the dedicated review experience. Treat them as responsive wide-window guides, not fixed pixel canvases.

## Engineering rule

Do not start with broad legacy deletion. Use:

> Replace → regression-test → delete.

When old behavior and current Lite scope conflict, prefer current Lite scope and document the decision.