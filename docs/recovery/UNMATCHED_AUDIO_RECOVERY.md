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

The old automatic player path attempted to detect when narration was reading text contained in an image.

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

The old helper's automatic unmatched discovery was primarily an **edge-audio** system:

- before the first safely aligned audio range → Introduction candidate;
- after the last safely aligned audio range → Credits candidate;
- whole processed tracks before/after the matched track range could also become edge candidates;
- edge regions totaling under roughly 2 seconds were ignored;
- the helper checked SMIL/chapter alignment evidence before deciding an edge boundary was safe to preserve;
- some boundaries were refined using transcript/audio evidence.

That model should not replace Lite's newer general unmatched-segment alignment review. It is useful for understanding edge trimming/classification and read-aloud page behavior only.

## Manual inspection remained lazy

When opening the old manual allocator, the helper returned:

- unmatched segment timing/transcript/silence information; and
- eligible image/document pairs near the edge.

It did **not** need to pre-run full-book OCR merely to open the allocator. Image preview and classification work could be done only for the relevant candidate resources.

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
