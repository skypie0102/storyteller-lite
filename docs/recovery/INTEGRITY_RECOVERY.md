# Final EPUB integrity / zero-length SMIL repair recovery — v0.39.0

This note records the exact high-value behavior recovered statically from the user-provided Storyteller One-Click v0.39.0 desktop executable. It is a behavioral regression reference for Lite, not code to port verbatim.

## Recovered native integrity boundary

The old desktop binary contains a native Rust module path `src\integrity.rs` and final-audit/repair strings for:

- readable EPUB/ZIP validation;
- duplicate ZIP members;
- broken SMIL text/audio references;
- invalid SMIL clocks;
- missing `clipEnd`;
- reversed/invalid clips;
- overlapping SMIL clips;
- non-UTF-8 structural files/paths;
- empty/playability checks;
- a deliberately narrow zero-length SMIL repair pass;
- staged repair installation with temporary/backup publication files.

The old UI report explicitly surfaced a counter named `Zero-length SMIL clips repaired` while reporting overlapping clips as an audit condition, which confirms that zero-length repair was a special-case structural fix rather than a broad timing normalizer.

## Exact zero-length repair increment

Disassembly of the v0.39.0 executable identifies the floating-point constant used by the zero-length repair path as exactly:

```text
0.001 seconds
```

The relevant machine-code path detects an invalid/non-positive SMIL interval where the parsed `clipEnd` is not greater than `clipBegin`, then forms a repair candidate equivalent to:

```text
new_clip_end = clip_begin + 0.001
```

The executable contains the failure string:

> `Could not repair a zero-length SMIL clip because clipEnd could not be rewritten.`

so the repair requires the existing SMIL audio element to be structurally writable; failure to rewrite is fatal to the repair pass rather than silently ignored.

## Important scope of the repair

Do **not** interpret this as permission to repair arbitrary timing errors.

Recovered behavior supports this narrow interpretation:

1. Parse/validate SMIL timing.
2. When a clip is reversed or zero-length (`clipEnd <= clipBegin`), attempt the minimal 1 ms `clipEnd` extension.
3. Rewrite the SMIL structurally.
4. Run the independent final audit afterward.
5. The final audit still rejects invalid references, non-positive clips, and overlapping SMIL clips.

Therefore the 1 ms mutation was a compatibility repair for degenerate SMIL, not an overlap solver, alignment correction, or automatic transcript retiming system.

## Relationship to current Lite

Current recovered Lite already has a stricter native `Validate` stage that rejects non-positive clip ranges and audits text/audio manifest targets and media duration consistency.

For the rebuilt Lite project, preserve the stronger architectural rule:

> Build output deterministically, audit it independently, and only apply narrowly specified structural compatibility repairs when there is a demonstrated interoperability reason.

Do not automatically add the legacy 1 ms mutation to Lite's normal builder merely because the old app had it. The current native builder should normally never emit a zero-length clip in the first place.

If interoperability testing later finds a legitimate external EPUB/SMIL edge case that needs this repair, the historical regression behavior is now known exactly: **extend `clipEnd` by 1 ms, then re-audit the whole candidate**.

## Testing implication

A future compatibility test can preserve the legacy rule without making it default behavior:

- fixture with `clipBegin == clipEnd`;
- repair produces `clipEnd = clipBegin + 0.001s`;
- repaired EPUB passes all other structural/media-overlay audits;
- repair must not hide or permit an overlap with another clip;
- malformed/unwritable timing remains an explicit failure.

## Evidence provenance

- Installer SHA-256: `417bce5a6e95bfac497bde4b1bbe48c3fb3ec7834909d0195f93741e9acf8eb9`
- Recovered desktop executable SHA-256: `fbfb10664dd127227bf95e881c2f30125d841baaa83a64518c848adcce38dbb1`
- Analysis was static; the installer/executable was not run.
- The `0.001` value was recovered from the native `integrity.rs` machine-code path, not inferred from the Python/Sigil helper.
