# Queue failure behavior recovery — v0.39.0 and Lite

This note resolves the previously unknown historical policy for what happens when one queued book fails while later books are still waiting.

## Recovered v0.39.0 behavior

Static analysis of the user-provided `Storyteller-OneClick-v0.39.0-Windows-x64-Setup.exe` recovered the old desktop executable and its embedded frontend.

The compiled backend contains this explicit runtime message:

> `Queue continuing after this failure; <N> pending book(s) remain.`

The old frontend separately exposed **Stop After** / `stop_after_current`, described as stopping the queue after the current book finishes. Failed/cancelled jobs remained individually retryable, while pending jobs were left alone.

Together, that establishes the historical v0.39.0 policy:

- a processing failure marks the current book failed;
- the queue itself remains running;
- if another unblocked pending book exists, processing continues to the next book automatically;
- `Stop After` is the explicit user mechanism that prevents auto-advance after the current terminal transition.

This is stronger evidence than the older recovery note that said the v1 policy was unknown.

## Current Lite behavior

The recovered Rust + Slint implementation already matches that policy.

`WorkerBridge::poll()` reconciles the terminal worker result and then calls `start_next_worker()` regardless of whether the completed worker outcome was Completed, Cancelled, or Failed. `start_next_worker()` delegates to `JobQueue::start_next()`.

`JobQueue` remains `Running` across terminal transitions unless `pause_after_current` had been requested. When that flag is set, the first terminal transition clears the flag and moves the queue to `Paused`.

Therefore current Lite behavior is consistent with the recovered old app:

- **failure normally auto-advances** to the next waiting book;
- **Pause after current** suppresses that advance after the active book reaches any terminal state.

## Product decision

Treat this as recovered behavior, not an open policy question.

Keep the current Lite policy unless the product requirement explicitly changes:

> A failed book should not stall the whole queue. Mark it failed, preserve diagnostics/retry state, and continue with the next waiting book unless the queue was explicitly paused or `Pause after current` was requested.

## Implementation/test implication

Add or retain regression coverage for at least:

1. Completed current job -> next waiting job starts automatically.
2. Failed current job -> next waiting job starts automatically.
3. Cancelled current job -> next waiting job starts automatically unless product cancellation semantics deliberately pause the queue.
4. `Pause after current` + terminal current job -> queue becomes paused and the next waiting job does not start.
5. Failed job remains available for explicit Retry without disturbing other waiting jobs.
