# Output publication recovery — v0.39.0 and Lite

This note records the final-output safety behavior recovered from the pre-Lite Storyteller One-Click v0.39.0 executable and compares it with the current recovered Lite implementation.

## Recovered v0.39.0 contract

The old backend exposes an explicit final sequence:

1. **Final EPUB audit** — `Auditing staged EPUB`
2. **Output publication** — `Publishing audited EPUB`
3. success only after publication — `Audited staged EPUB published successfully; any previous output was preserved until this point.`

The compiled executable contains dedicated sibling publication names:

- `.publish.tmp`
- `.publish.bak`

and explicit error/rollback branches including:

- stale publication temp removal;
- stale publication backup removal;
- preserving the previous output before replacement;
- restoring the previous output after an interrupted publish;
- removing a cancelled publication temp file;
- restoring the previous output after cancellation;
- removing an unexpected backup after cancellation;
- removing a newly published EPUB during rollback;
- checking that a publication copy completed to the expected byte size.

This establishes the important old invariant:

> **An audited candidate was prepared first; publication used temporary/backup siblings so a partial/cancelled replacement would not destroy the previous complete output.**

The old app supported an `overwriteExisting` path, which explains why it needed a backup/rollback branch.

## Current recovered Lite behavior

Current Lite is intentionally stricter: `publish_validated_epub()` refuses to overwrite an existing destination at all.

Its current flow is:

1. candidate and destination must differ;
2. fail if destination already exists;
3. copy the validated candidate to a sibling temporary publication path;
4. honor cancellation during that copy;
5. rename the completed temp file to the final destination;
6. remove the temp on failure.

So Lite already preserves the most important publication invariant while avoiding the old overwrite complexity.

## Product implication

Keep the current Lite policy unless an explicit overwrite feature is added later:

- never overwrite the source EPUB;
- do not expose partially copied output at the final pathname;
- Validate must succeed before publication starts;
- an existing output should cause an explicit conflict instead of silent replacement;
- stale temp files can be cleaned safely;
- cancellation before the final rename leaves no final output.

If overwrite support is ever intentionally introduced, recover the old stronger pattern rather than doing a naive remove-and-rename:

1. preserve existing destination to a sibling backup;
2. publish the fully copied/audited temp candidate;
3. remove backup only after success;
4. on interruption/cancellation, restore the preserved destination;
5. test every rollback branch.

## Why this belongs in the recovery packet

The old implementation had more overwrite machinery than Lite needs, but the underlying safety rule is still a core Storyteller behavior:

> **Build into workspace → independently audit → publish atomically → never leave the user's previous valid book replaced by a partial result.**

That rule should survive future native EPUB refactors and the planned single-pass/raw-copy finishing path.
