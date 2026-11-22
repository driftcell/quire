# RUNTIME PENDING FAILURE RECOVERY

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-22
- **Promoted To:** `../docs/RUNTIME_RIG_STREAMING.md`, `../docs/RUNTIME_SESSION_EVENTS.md`

## 1. Context & Problem

A long real generation received a queued editorial correction while the main run and multiple chapter
writers were active. The original main run ended with a canonical-history mismatch. Its pending item
remained unclaimed, `active_run_status` became `failed`, and the session showed only the initial user and
assistant messages even though later assistant text existed in the hot trace.

Two independent recovery defects were visible:

- the pending drain accepted only `idle`, so a terminal `failed` or `interrupted` session could never
  release its queue even when no run remained active;
- a provider stream error returned before the ordinary assistant-receipt fallback, losing the failed
  run's visible response when canonical suffix persistence had not completed.

Canonical commit failures also needed lengths and mismatch positions for diagnosis; the previous message
only said that the conversation changed, without identifying whether the live log or returned history
broke the loaded prefix.

## 2. Rejected Options & Why

- **Force queue claiming while a run is active.** This can change canonical history under the active
  runner and reproduce the prefix conflict. Recovery proceeds only after running/awaiting-input states
  have ended and no `agent_runs.status = 'running'` row remains.
- **Drop failed assistant text because the provider normally commits its own receipts.** Normal Rig runs
  still own canonical receipts. On a stream error, the existing idempotent assistant fallback preserves
  the visible response; if a receipt already exists it is a no-op.
- **Fabricate or manually rewrite the missing old history.** Recovery must use the same canonical
  assistant-receipt path as normal fallback persistence and never invent earlier progress.
- **Hide the failed run or keep the queue pending indefinitely.** Failed/interrupted sessions are valid
  queue-drain states once no run is active; the queued user message must execute exactly once.

## 3. Chosen Approach & Trade-offs

`drain_pending_locked` now blocks only `running` and `awaiting_input`, while retaining the independent
latest-running-run guard. Failed and interrupted sessions can therefore release their queue through the
same idempotent claim path as idle sessions.

The stream-error branch preserves non-empty main response text with `insert_assistant_message` before
returning the error. This uses canonical history and visible asides, does not create a second receipt
when the provider already committed one, and keeps the failed terminal event and recoverable error.

Canonical prefix errors now include expected/current/full-history lengths and the first mismatch index,
distinguishing a live-log prefix change from a returned history that no longer preserves the loaded
prefix.

The regression `failed_turn_preserves_text_and_releases_pending_queue` uses a fail-once provider and a
second queued message. It verifies the failed assistant text, queued user text and recovered assistant
text are all visible, the queue empties, the session returns to idle, exactly two runs exist and the
provider is called exactly twice. Five consecutive targeted runs and the full workspace suite passed.
