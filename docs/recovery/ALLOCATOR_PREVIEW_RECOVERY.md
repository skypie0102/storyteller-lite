# Manual allocator preview semantics recovery — v0.39.0

This note records how the old app produced manual-allocation audio previews. It is behavioral evidence for the reduced Lite allocator, not an implementation requirement.

## Preview files were disposable

The old unmatched-audio inspector preserved both:

- original source identity/timing; and
- optional temporary preview identity/timing.

For each review segment the payload included fields equivalent to:

- stable `segment_id`;
- `original_source_path` / `original_source_name`;
- `original_source_begin` / `original_source_end`;
- preview `source_path` / `source_name`;
- preview `source_begin` / `source_end`;
- duration;
- `spliced` flag;
- silence markers;
- grouped transcript cues.

If FFmpeg was unavailable, preview used the underlying source directly and retained the real non-zero start/end offsets.

If FFmpeg was available, the helper rendered a temporary segment clip under the processing workspace's `.storyteller-manual-clips` directory. That clip started at 0 seconds and was marked `spliced=true` while the original source/timing fields were retained separately.

Stale preview clips were deleted/rebuilt when inspection ran again.

## Review truth versus cache

The durable review decision was therefore **not** the preview file itself.

Authoritative identity remained the original logical segment and timing. Temporary preview audio was a convenience that could be regenerated after restart/retry.

This matches the recovered persistence model:

- manual decisions/drafts = durable state;
- preview clips/workspaces = rebuildable artifacts.

## Silence markers

The old inspector ran FFmpeg `silencedetect` over the preview representation when available, historically using roughly:

- noise threshold `-38 dB`;
- minimum silence duration `0.35 s`.

It returned bounded markers with start/end/midpoint/duration for allocator display and split suggestions.

These are historical test vectors, not mandatory Lite thresholds.

## Transcript cues

Transcript timeline entries overlapping the segment were converted to local segment time and grouped for readability. A group flushed when roughly one of these conditions occurred:

- punctuation ended the current text;
- around 12 source entries had accumulated;
- about 8 seconds had accumulated.

The payload was capped to avoid unbounded UI data.

This is useful precedent for Lite's allocator transcript display: the review UI does not need to render every raw Whisper token/word individually.

## Processed-tail special case

For trailing unmatched audio that occupied the tail of a processed track, the old helper could split the processed track and keep a backup under the manual-clips workspace. Review payload still retained both original and aligned-source timing so applying decisions could avoid double-owning the same audio interval.

The key invariant is more important than the exact mechanism:

> Preview/edit preparation must never destroy the source-of-truth audio interval or make the same interval belong to two final destinations.

## Recommended Lite interpretation

Lite can keep preview architecture simpler:

1. Prefer seeking directly into the staged audiobook/chunk using explicit start/end bounds when the playback backend supports the codec reliably.
2. Generate a temporary preview clip only when required for playback compatibility, waveform generation, or isolation.
3. Treat generated preview media as cache/workspace data that can be deleted and reconstructed.
4. Persist review decisions against stable segment IDs plus original/global audiobook timing, not against temporary preview filenames.
5. Keep transcript/silence display data bounded and regenerate it if needed.

This design naturally supports restart/retry without making large preview files part of durable application state.
