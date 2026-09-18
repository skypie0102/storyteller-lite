# Retry, Reprocess, and output-conflict recovery — v0.39.0

This note records the old Storyteller OneClick distinction between **Retry** and **Reprocess**. It is historical behavior/reference for Lite; it does not by itself restore overwrite support.

## Retry was for interrupted/unsuccessful jobs

The recovered frontend/backend only allowed Retry for jobs in these states:

- `failed`
- `cancelled`
- `allocation_paused`

The old UI described ordinary retry as:

> `Retry started for this book only. Other pending books remain untouched.`

For a paused allocation, Retry instead returned the job to `waiting_for_allocation` and reopened/rebuilt the review session so the autosaved draft could be restored.

For failed/cancelled jobs, Retry reset transient execution fields such as:

- progress
- error
- runtime
- statistics
- phase timing data

while keeping the same queue/book identity and captured settings snapshot.

## Reprocess was for completed jobs

The old frontend exposed a distinct action:

> `Reprocess this finished book`

Only a `completed` queue entry could use it.

Reprocess reset the completed job to a new pending/run state, but unlike Retry it represented a deliberate new production pass over a book that had already successfully produced output.

The old UI reported:

> `Reprocessing started for this completed book only.`

Other queued books were not implicitly changed.

## Existing output blocked normal pending processing

The old queue tracked an output-conflict condition separately from failure.

UI text for a pending conflict:

> `Output already exists — rename/remove it or use Reprocess.`

Backend strings likewise say pending books whose output EPUB already exists remain blocked until the conflict is resolved.

This prevented a normal queue start from silently overwriting an existing finished book.

## Reprocess required explicit overwrite approval

When a completed job's output still existed, the frontend showed an explicit warning similar to:

> `Reprocess “<title>”?`
>
> `The existing finished EPUB will be overwritten:`
> `<path>`

with actions:

- `Overwrite and Reprocess`
- `Keep Existing File`

Only after confirmation did the frontend call `reprocess_job` with `overwriteExisting=true`.

The old backend separately validates that a finished EPUB exists and overwrite approval is required before entering that path.

## Single-book immediate processing used the same overwrite gate

The old pre-Lite UI also had a one-off `Process` flow separate from the queue. If its target output already existed, it required the same kind of explicit overwrite confirmation before calling the backend with `overwriteExisting=true`.

That immediate Process-now workflow is intentionally not part of the recovered Lite scope, but its existence reinforces the old safety invariant:

> **Output replacement required an explicit dedicated action/approval; normal queue processing did not overwrite.**

## Current Lite comparison

Current recovered Lite is stricter:

- source output paths are safety-checked;
- publication refuses an existing destination;
- completed jobs cannot simply be restarted through the ordinary retry path;
- there is no restored old-style overwrite/Reprocess product flow in the current roadmap.

That is a valid Lite simplification.

## Product decision for Lite

Preserve these concepts even if Lite never restores Reprocess:

1. **Retry is recovery, not overwrite.** Failed/cancelled/review-interrupted work may be retried/resumed without treating it as a successful book replacement.
2. **Existing output is an explicit conflict.** Never silently replace it during ordinary queue execution.
3. **Completed-output replacement, if ever added, must be a deliberate separate action with explicit confirmation and atomic rollback-safe publication.**

Do not add the old Reprocess/overwrite workflow solely for historical parity. It should return only if the reduced Lite product actually needs it.

## Potential Lite regression tests

Regardless of whether Reprocess returns:

- Retry cannot be invoked on a completed job through the ordinary recovery path.
- A pending job whose destination already exists never silently overwrites that file.
- Retrying one failed/cancelled job does not reset or reorder unrelated waiting jobs.
- Review-interrupted retry restores durable review decisions while rebuilding disposable preview/workspace state.
- Any future overwrite path requires explicit opt-in and preserves/restores the previous valid output on failed/cancelled publication.
