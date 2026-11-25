# DECOUPLE ASK_USER FROM RUN LIFECYCLE AND ANCHOR AS QUESTION MESSAGES

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-25
- **Promoted To:** `../docs/RUNTIME_ASK_USER.md`, `../docs/RUNTIME_SESSION_EVENTS.md`

## 1. Context & Problem

In the previous suspend/resume design, `ask_user` transitioned `agent_runs.status` into `awaiting_input` without emitting `turn_finished` or closing `Run 1`'s trace summary. Later, answering or dismissing the questionnaire resolved `Run 1` out-of-band before `questionnaire_answered` was written, leaving stale `"running"` `trace_summary` records in PostgreSQL and forcing the frontend to compensate via:

- `questionSlots` pinned onto historical/live trace tool lists by `tool_call_id`;
- `orphanTraces` appended unconditionally to the bottom of the chat timeline when `Run 1` had no assistant prose receipt;
- `normalizeTerminalStatus` silently mapping `"running"` to `"completed"`;
- `msg.role === "tool"` workarounds rather than a canonical question node in `rig-memory`.

As a result, once `Run 2` completed and produced the final assistant message, the historical `Run 1` question card and trace receipt floated at the very bottom below `Run 2`'s answer.

## 2. Rejected Options & Why

1. **Patching `resolve_suspended_run` ordering plus timestamp sorting in `orphanTraces`**:
   - Keeps `agent_runs.status = 'awaiting_input'` coupled to the UI questionnaire lifecycle.
   - Still leaves `ask_user` cards hanging off `Run 1`'s tool call trace (`questionSlots`) instead of living as first-class structured conversation entries, so pure-question turns without assistant prose still depend on orphan trace reconstruction and special-case UI plumbing.
2. **Reading/reconstructing question cards solely from `agent_questions` merged by `run_id` / `created_at` on the frontend**:
   - Breaks canonical timeline ordering whenever a run has both tool activity, assistant prose, and a follow-up question (`Assistant` -> `Question` -> resumed `Run 2`), because `created_at` timestamps between `agent_runs`, `agent_questions`, and canonical memory receipts must be stitched heuristically across two stores.
   - Retains dual positioning code paths (`questionSlots` vs `orphanQuestions`).

## 3. Chosen Approach & Trade-offs

Decouple `ask_user` completely from `agent_runs` lifecycle and promote question cards to structured canonical conversation messages:

1. **Clean run closure on `ask_user`**:
   - `Run 1` always finishes cleanly as `agent_runs.status = 'completed'`, emits `turn_finished`, and immediately materializes its cold `trace_summary` (whose settled status is `completed`).
   - Only `agent_sessions.active_run_status` (`awaiting_input`) and `agent_questions.status = 'pending'` gate user follow-ups and queued drains until `answer_question`, `post_message` dismissal, or `retry` clears `awaiting_input` to `idle` and resumes in `Run 2`.
2. **First-class `MessageRole::Question` in `quire-session-protocol` and `rig-memory`**:
   - Add `MessageRole::Question` and `SessionMessage.question_id: Option<String>`.
   - Extend `rig-memory` `ConversationMemory` receipts (`HashMap<MessageId, Vec<MessageReceipt>>`) so a `Question` receipt is attached onto the tail entry of `Run 1` strictly after any `Assistant` receipt for `Run 1`, and strictly before `AskUserAnswerPayload` / `Run 2`.
3. **Zero-legacy frontend rendering**:
   - Remove `questionSlots`, `compactQuestions`, `activityHasQuestion`, `msg.role === "tool"`, and `orphanQuestions` hacks.
   - Hide non-failed `ask_user` calls in `isVisibleTraceTool` (matching `spawn_subagents`) so a pure-question turn renders `QuestionCard` directly as a structured agent message item, while a tool-gathering turn without prose attaches `Run 1`'s `HistoricalRunTrace` immediately above its `QuestionCard`.
   - Keep `normalizeTerminalStatus` truthful (`running` stays `running` rather than being masked as `completed`).

Accepted trade-off: pre-cutover sessions created with legacy `awaiting_input` run rows are not migrated via backwards-compatibility shims.
