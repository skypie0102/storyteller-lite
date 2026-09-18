# Queue terminal behavior recovery — v0.39.0 and Lite

This note resolves the historical policy for what happens when one queued book completes, fails, or is cancelled while later books are still waiting.

## Recovered v0.39.0 behavior

Static analysis of the user-provided `Storyteller-OneClick-v0.39.0-Windows-x64-Setup.exe` recovered the old desktop executable and its embedded frontend.

The compiled backend contains this explicit failure message:

> `Queue continuing after this failure; <N> pending book(s) remain.`

The old frontend separately exposed two distinct controls:

- **Stop After** — `Stop the queue after the current book finishes.`
- **Cancel** — `Cancel the current processing book.`

Failed/cancelled jobs remained individually retryable.

## Cancellation control-flow recovery

A deeper static disassembly resolves the cancellation edge that was initially uncertain.

The backend maps the processing result into distinct terminal variants/statuses for:

- Completed
- Cancelled
- Failed

The Cancelled branch emits `Processing cancelled.` and then joins the same common terminal cleanup/state-persistence path used by the other outcomes.

After terminal state is persisted, the runner checks a **separate queue-stop flag** before advancing. If that stop flag is clear, the common path pops the next pending queue entry and starts it.

The current-job cancellation flag is checked separately from the queue-stop flag. In other words, cancelling the active book does not itself set the queue-stop condition.

This matches the old UI separation between **Cancel current book** and **Stop After current book**.

Therefore recovered v0.39.0 behavior is:

- Completed current book -> queue normally advances.
- Failed current book -> current book becomes failed/retryable and queue normally advances.
- Cancelled current book -> current book becomes cancelled/retryable and queue normally advances.
- `Stop After` -> the current book is allowed to reach its terminal outcome, then the queue stops before starting the next pending book.

The failure-specific `Queue continuing after this failure...` message was merely additional feedback for the failure case, not evidence that only failures could advance.

## Current Lite behavior

The recovered Rust + Slint implementation already matches this policy.

`WorkerBridge::poll()` reconciles the terminal worker result and then calls `start_next_worker()` for Completed, Cancelled, and Failed outcomes. `start_next_worker()` delegates to `JobQueue::start_next()`.

`JobQueue` remains `Running` across terminal transitions unless `pause_after_current` had been requested. When that flag is set, the first terminal transition clears the flag and moves the queue to `Paused`.

## Product decision

Treat this as recovered behavior, not an open policy question.

Keep the current Lite policy unless an explicit product requirement changes it:

> **A terminal book result does not stall the queue.** Completed books finish normally; failed/cancelled books retain their retryable state; then the next waiting book starts. Only an explicit queue pause / Pause-after-current suppresses the next start.

This keeps a single problematic book from blocking a long batch while preserving the user's ability to inspect/retry it later.

## Implementation/test implication

Add or retain regression coverage for at least:

1. Completed current job -> next waiting job starts automatically.
2. Failed current job -> next waiting job starts automatically and failed job remains retryable.
3. Cancelled current job -> next waiting job starts automatically and cancelled job remains retryable.
4. `Pause after current` + Completed -> queue becomes paused; next waiting job does not start.
5. `Pause after current` + Failed -> queue becomes paused; next waiting job does not start.
6. `Pause after current` + Cancelled -> queue becomes paused; next waiting job does not start.
7. Retrying a failed/cancelled historical job does not disturb the ordering/state of unrelated waiting jobs.
