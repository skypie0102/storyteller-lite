# Unmatched-audio / Smart classification recovery

This note reconciles three sources for the Lite unmatched-audio design:

1. the preserved Lite planning transcript;
2. the supplied Lite manual-allocation mockup;
3. static recovery of the pre-Lite v0.39.0 installer/helper.

It exists to avoid two opposite mistakes: rebuilding the entire old allocator, or trimming away capabilities that the Lite plan actually intended to keep.

## Recovered Lite intent

The preserved Lite planning explicitly called for an audio/unmatched pipeline that would:

- move silence detection and unmatched-audio inspection into Rust;
- add **automatic edge trimming**;
- add **Smart classification**;
- use **lazy OCR**;
- build the **streamlined allocator** around those APIs.

The same planning separately marked these old surfaces for removal:

- the old unmatched-audio mode selector;
- the **permanent OCR toggle**;
- the old full-complexity allocator presentation.

An early recovered Lite settings slice also described `Smart / ReviewAll` as the intended reduced unmatched-audio policy choices.

Therefore Lite should not interpret “remove permanent OCR” as “remove OCR entirely.” OCR is intended to become an internal, on-demand capability rather than a persistent user toggle.

Likewise, “streamlined allocator” means remove editor complexity, not necessarily remove the ability to recognize/assign Introduction, Credits, Graphic Readout, or other extra audio when useful.

## Exact old `edgeMode` semantics

The old serialized enum and recovered serializer jump table establish these discriminants and labels exactly:

- `0` → `manual` — UI: **Manual allocation — recommended**
- `1` → `automatic_player` — UI: **Automatic audio-player pages**
- `2` → `automatic_transcript` — UI: **Automatic synchronized transcript**
- `3` → `off` — UI: **Do not preserve**

The finishing command builder behaved as follows:

- every mode except `off` added `--preserve-unmatched-audio`;
- `automatic_transcript` selected `--unmatched-page-mode transcript`;
- the other preserving paths selected `--unmatched-page-mode player`;
- manual review additionally supplied `--manual-allocations <json>` once the user had made decisions.

This old four-mode selector is historical evidence only. Lite's recovered plan intentionally reduces it to **Smart / ReviewAll** plus a streamlined review screen.

### Old OCR wiring mismatch

Disassembly also confirms that the v0.39.0 desktop backend forwarded `--graphics-ocr` only when:

- `useGraphicsOcr == true`; **and**
- `edgeMode == automatic_transcript`.

However, the bundled helper itself contains OCR-based Graphic Readout separation for `player` mode too. The old UI presented the OCR toggle generically rather than only for transcript mode.

That is evidence of an old wiring mismatch/unused path, not a behavior Lite should copy. Lite should wire lazy OCR according to the current Smart classifier's needs, independent of this historical quirk.

## What the old installer reveals about lazy OCR

The bundled Sigil-derived helper did not OCR the entire book indiscriminately.

### Candidate narrowing before OCR

For edge audio it first examined a bounded set of EPUB documents near the aligned boundary:

- up to 12 candidate documents;
- up to 24 candidate raster images;
- Introduction candidates could include the OPF cover image;
- image references were collected from XHTML/SVG/object/image resources;
- embedded `alt`, `title`, and SVG `<text>` content were collected as text hints before invoking OCR.

Only those candidate images were handed to OCR.

### OCR engines and fallback

Historical implementation:

1. RapidOCR / PP-OCRv6-small was attempted first.
2. Images with no accepted RapidOCR result fell back to bundled English Tesseract.
3. Images larger than 25 MiB were skipped.
4. RapidOCR text required confidence at least `0.45` and at least two alphanumeric characters.
5. Tesseract output required mean confidence at least `20` and at least two alphanumeric characters.
6. OCR lines were normalized/deduplicated and rejected if they contained fewer than 2 or more than 240 alphanumeric characters.

If no image needed OCR but embedded graphic text hints existed, the helper could operate from those hints alone.

This is genuinely **lazy/bounded OCR**, even though the old UI exposed a permanent on/off toggle around it.

## Graphic-readout classification evidence

The old helper had an OCR-driven automatic-player path capable of detecting when narration was reading text contained in an image.

High-level behavior:

1. OCR/text hints were converted into phrases.
2. Timed transcription words were searched for a plausible matching window.
3. A match required multiple distinctive anchors and minimum lexical coverage/similarity.
4. Nearby matching phrases could be joined into one graphic-readout audio block.
5. The matched audio block was attached to the EPUB document/image that supplied the OCR text.
6. That audio interval was removed from the remaining unmatched Introduction/Credits pool so it would not be allocated twice.

Historical test-vector thresholds included:

- OCR phrase groups normally 5–80 normalized words; aggregate phrase up to 120 words;
- lexical coverage at least `0.36`;
- at least 2 distinctive matching words;
- similarity score threshold `0.77` for short phrases and `0.69` for longer phrases;
- related matches could join when within 8 seconds;
- combined graphic-readout block capped at 120 seconds;
- final graphic clip needed to be at least 1.25 seconds;
- competing graphic candidates were rejected when overlap exceeded roughly 20% of the shorter candidate.

These constants are **historical test vectors**, not mandatory Lite tuning values.

## Old edge discovery behavior

The old helper's unmatched discovery was primarily an **edge-audio** system:

- before the first safely aligned audio range → Introduction candidate;
- after the last safely aligned audio range → Credits candidate;
- whole processed tracks before/after the matched track range could also become edge candidates;
- edge regions totaling under roughly 2 seconds were ignored;
- the helper checked SMIL/chapter alignment evidence before deciding an edge boundary was safe to preserve.

A first partial track was considered safe when existing SMIL coverage existed or the corresponding chapter's first matched sentence was sentence 0. A final partial track was considered safe when existing SMIL coverage existed or the final matched sentence reached the end of the chapter.

That model should not replace Lite's newer general unmatched-segment alignment review. It is useful for understanding edge trimming/classification and read-aloud page behavior only.

## Exact old edge-boundary refinement

The helper did more than trust the raw alignment boundary.

Historical constants:

- transcript-boundary search radius: **6.0 s**
- acceptable inter-cue silence for transcript refinement: **0.65–8.0 s**
- physical audio search after a final boundary: **15.0 s**
- physical transition silence: **2.0 s**

### Transcript refinement

Around a nominal boundary, the helper inspected neighboring timed transcript spans. It preferred the midpoint of a qualifying silence gap within 6 seconds of the nominal point:

- Introduction preferred a gap midpoint at or before the nominal boundary.
- Credits preferred a gap midpoint at or after the nominal boundary.

If no qualifying gap existed, it fell back to nearby cue starts/ends/overlap edges rather than arbitrarily cutting through a spoken cue.

### Final physical-audio refinement

For a safely aligned final chapter boundary, a second path scanned from roughly `nominal - 0.5 s` through `nominal + 15 s` with FFmpeg `silencedetect` at approximately `-40 dB` and a minimum **2.0 s** silence.

If a qualifying silence started no earlier than about `nominal - 0.10 s`, the helper moved the credits boundary to the midpoint of the first such silence, never before the nominal boundary. If detection failed, transcript refinement was the fallback.

This is useful evidence for Lite's planned “automatic edge trimming”: choose a conservative silence/cue boundary near an already-safe alignment edge, rather than treating silence detection alone as proof that narration may be deleted.

## Existing-overlay/audio trimming behavior

When preserving edge audio separately, the helper prevented the existing Media Overlay from also claiming that edge region:

- SMIL audio clips completely before the Introduction boundary or after the Credits boundary were removed;
- clips straddling a boundary were shortened to that boundary;
- the tolerance around these comparisons was roughly **0.0005 s**;
- empty nested SMIL sequences were cleaned up afterward.

The manual-allocation path additionally physically trimmed the packaged aligned **tail audio file** at the credits boundary when FFmpeg was available, then audited that the resulting file did not extend more than roughly **0.12 s** beyond the requested cut.

The automatic preserve path did not use that same physical-tail trim in every case, so Lite should preserve the invariant (no duplicated/ambiguous referenced timing) rather than copying the exact old file-rewrite asymmetry.

## Manual inspection remained lazy

When opening the old manual allocator, the helper returned:

- unmatched segment timing/transcript/silence information; and
- eligible image/document pairs near the edge.

It did **not** need to pre-run full-book OCR merely to open the allocator. Image preview and classification work could be done only for the relevant candidate resources.

Manual preview silence markers used FFmpeg `silencedetect` at approximately **-38 dB** with a minimum **0.35 s** silence.

This is a good model for Lite: keep review startup cheap and perform image/OCR work only when Smart classification or the current segment actually needs it.

## Recommended Lite Smart policy

The precise classifier should be implemented/tested in Rust rather than copied from the GPL helper, but the recovered product intent suggests this shape:

### `Smart` (default)

Automatically resolve cases that are high-confidence and safe, while sending ambiguous cases to Review Audio.

Examples of safe automatic handling may include:

- trivial leading/trailing silence/noise;
- clearly bounded edge audio that can be identified as Introduction/Credits according to tested rules;
- high-confidence graphic-readout matches where transcript + EPUB image text establish a safe destination;
- other cases for which the system has a deterministic, auditable placement rule.

Anything weak/ambiguous remains pending for the allocator rather than being silently discarded.

### `ReviewAll`

Surface all unmatched/unallocated regions that would otherwise be handled by Smart, so the user can inspect/override them.

This is a reduced two-mode policy and should **not** resurrect the old `manual / automatic_player / automatic_transcript / off` selector.

## Streamlined allocator scope

The supplied mockup remains the visual/product guide. The recovered evidence supports retaining these capabilities in the streamlined flow:

- segment audio preview and seeking;
- transcript/timing/silence context;
- previous/next unresolved navigation;
- Smart suggested classification/destination where available;
- explicit user override;
- Introduction / Credits / Graphic Readout / Extra Audio concepts where the classifier/destination actually needs them;
- attach Graphic Readout audio to a valid image page/document candidate;
- explicit Exclude/Discard only under a clearly validated policy;
- durable Apply & Next decisions;
- autosave/retry-safe review state;
- final output validation/audit.

What remains out of initial Lite scope:

- arbitrary split/merge editor;
- manual waveform trim handles as a general editing surface;
- permanent OCR enable/disable setting;
- broad `Apply to similar` rule system;
- unrestricted old-app destination/category editor;
- old Sigil/Python implementation itself.

## Data-model implication

A pure `Pending / Assigned / Excluded` decision model is a useful base but may be too lossy for the planned Smart/graphic-readout flow.

Prefer a model that can preserve both **disposition** and an optional **classification/suggestion**, for example conceptually:

```text
ReviewDecision
  Pending
  Assigned {
      destination,
      classification?,
      source = Automatic | Manual
  }
  Excluded {
      reason,
      source = Automatic | Manual
  }
```

Possible Lite classifications can stay intentionally small (e.g. Introduction, Credits, GraphicReadout, ExtraAudio) and should not become a general-purpose taxonomy unless the product needs it.

Automatic decisions should remain auditable and reversible before publication.

## Implementation warning

The old helper source is GPLv3-or-later/Sigil-derived. Recover behavior, thresholds, invariants, and test cases; do not copy the implementation wholesale into Lite without an explicit licensing decision.
