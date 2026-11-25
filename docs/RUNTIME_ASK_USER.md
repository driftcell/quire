# RUNTIME ASK_USER CARDS

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-session-protocol`, `quire-runtime`
- **Related Notes:** `../notes/runtime_rig_046_lifecycle_consolidation.md`, `../notes/runtime_rig_046_native_migration.md`, `../notes/runtime_ask_user_suspend_resume.md`, `../notes/ask_user_run_decoupling.md`

## 1. Scope & Non-Goals

This document governs one-shot `ask_user` questionnaires, mixed-tool batch policy, durable pending
cards, structured answer/skip payloads, dismissal and continuation through a new AgentRunner turn.
It does not use a serialized `AgentRun`, an inline hook wait or same-run `agent.resume`.

## 2. Canonical Design & Contract

### 2.1 Protocol and storage

The session protocol retains:

- `QuestionOption`, `QuestionItem`, `QuestionAnswer`, `PendingQuestion` and `ResolvedQuestion`;
- `MessageRole::Question` and `SessionMessage.question_id` for first-class question timeline receipts;
- `SessionDetail.pending_question` and `resolved_questions`;
- non-terminal, non-run-scoped `questionnaire_requested`, `questionnaire_answered` and `questionnaire_dismissed` events;
- `ActiveRunStatus::AwaitingInput` (session input gate only; `runtime.runs` never enters `awaiting_input`).

`runtime.questions` remains the durable card store with pending/answered/skipped/dismissed states,
optional originating `tool_call_id`, JSON questions/answers and timestamps.

Answers sent to the model use only `AskUserAnswerPayload`:

```json
{
  "kind": "ask_user_response",
  "question_id": "q_01JXYZ",
  "tool_call_id": "run_01JABC:call_99",
  "status": "answered",
  "skipped_all": false,
  "answers": [
    {
      "question_id": "benchmark_scope",
      "question_title": "请选择对标高校范围",
      "selected_option_ids": ["q3"],
      "selected_option_labels": ["Q3"],
      "skipped": false
    }
  ]
}
```

`QuestionnaireResolutionStatus` is `answered` or `skipped`; `skipped_all: true` uses an empty answer
list. Labels and titles are resolved from the stored question definition. The payload is exported in
`contracts/session-protocol.schema.json`.

### 2.2 One-shot tool and mixed batches

`ask_user` validates non-empty question IDs/titles and options, publishes `AskUserRequestedMeta`
through `ToolContext::insert_result`, and immediately returns an `awaiting_input` receipt. It never
waits for a user response inside the tool or hook.

`ToolLifecycleHook::on_model_turn_finished` records whether the accepted assistant turn contains
`ask_user` and clears provisional preview streams. Every
sibling tool call—before or after the question—is handled before preview or execution:

1. `on_dispatch` emits only its `ToolCallStarted` and returns a structured skipped action;
2. Rig resolves the denied dispatch as a skipped tool result and still presents its outcome;
3. `on_outcome` emits the sole `ToolCallFinished` with `status = "skipped"`.

This policy is position-independent and avoids duplicate finish events. Normal question and sibling
outcomes proceed rather than stopping in `on_outcome`: Rig's fail-fast would otherwise drop the
current tool result and never dispatch later siblings. Once the question executes successfully, the
hook creates the questionnaire event and emits `ToolCallFinished { status: "awaiting_input" }`. Rig
commits the complete result batch, then `TurnControlHook` stops the following completion call before
any provider request. The driver treats that cancellation as normal suspension, commits canonical
history containing every question/skipped result and does not mark the run failed. Tool
validation/execution failures without question metadata receive a failed finish instead.

### 2.3 Run Closure & Question Anchoring

`EventSink` persists the `runtime.questions` row before publishing `questionnaire_requested` (`liveRunStatus -> awaiting_input`). When the turn stops with a pending question:

1. Canonical conversation history (any assistant prose and accepted tool calls) is already committed in `rig-memory`;
2. `ConversationMemory::append_question_message` attaches a visible `MessageRole::Question` receipt (`question_id`, `run_id`) onto the canonical conversation tail strictly after any `Assistant` receipt for that run;
3. `finish_run_for_question` closes the run cleanly as `runtime.runs.status = 'completed'`, keeps `runtime.sessions.active_run_status = 'awaiting_input'`, appends `turn_finished`, and immediately materializes the settled `completed` cold `trace_summary`.

Startup recovery rewrites only `running` runs. A durable `MessageRole::Question` receipt and pending `runtime.questions` card remain visible in canonical timeline order after refresh or restart.

### 2.4 Answer, skip, retry, and dismissal

`POST /api/sessions/{session_id}/questions/{question_id}/answer` validates that the card is pending, validates question/option IDs, stores `answered` or `skipped`, clears `runtime.sessions.active_run_status` from `awaiting_input` to `idle`, emits `questionnaire_answered`, and starts the continuation turn (`Run 2`) cleanly without modifying `Run 1`'s finalized run or trace.

The answer payload is serialized as JSON and appended to canonical memory as an **internal, invisible
prompt**. It is the only model input for the continuation turn. There is no
`The user answered questionnaire...` prose, synthetic tool-role text, or second
`Continue the task...` user message. The assistant response starts a new run with the standard
lifecycle and visible assistant projection.

A normal user message (or `retry`) may bypass the card. Pending cards become `dismissed` (`superseded_by_user_message` or `retried_by_user`) and `clear_session_awaiting_input` releases the session input gate before starting the next turn. On `post_message`, canonical memory receives an invisible `ask_user_response` JSON prompt with `status = "dismissed"` before the new visible prompt. On `retry`, rewinding to a prompt cleans any suffix entries as well as any non-`User` receipt asides anchored to that prompt. Pending queue contents are not consumed while the session is in `awaiting_input`.

## 3. Invariants & Verification

- A pending or resolved card is anchored as a `MessageRole::Question` receipt strictly after its originating run's assistant prose (if any) and before any later run's output.
- Non-failed `ask_user` calls stay hidden in trace tool accordions (`isVisibleTraceTool`); if a pre-question run produced tools/subagents without assistant prose, the run's `HistoricalRunTrace` renders immediately above its `QuestionCard` rather than as a bottom orphan trace.
- `Run 1` always closes as `completed` with a finalized cold trace at questionnaire time, never remaining `running` or `awaiting_input`.
- Mixed batches execute no sibling tool and every sibling has exactly one terminal skipped finish,
  regardless of whether it appears before or after `ask_user`.
- `ask_user` never holds a task while waiting for a person.
- Answers and whole-card skips use one pure JSON payload with resolved labels and no prose continuation.
- Continuation uses a new AgentRunner turn, not an unexposed Runner checkpoint or same-run resume.
- Internal answer prompts remain canonical but do not render as user bubbles.
- A new user message appends an invisible canonical dismissal before its visible prompt and wins;
  the dismissal never becomes a UI bubble, and a resolved card cannot be answered again.
- Failed question calls remain visible in trace tool lists and do not leave running tool rows.

Coverage includes:

- `ask_user_suspends_until_answers_resume_the_run`
- `ask_user_anchors_question_after_assistant_prose_and_before_resumed_run`
- `ask_user_skip_marks_skipped_and_continues`
- `new_message_dismisses_pending_question_and_starts_a_new_turn`
- `ask_user_suspension_keeps_pending_queue_untouched`
- `question_closure_completes_run_and_materializes_completed_trace`
- `HookTestHarness` mixed-batch sibling skipping in both call orders with exactly-once finishes
- canonical `memory` `question_receipt_anchors_after_assistant_and_rewinds_cleanly` unit test
- session-protocol structured payload/label serialization tests
- frontend `EditorialChat.test.tsx`, `sessionEventReducer.test.ts`, `traceSegments.test.ts`, `toolMeta.test.ts`

Full gate:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
just schema
bun run --cwd web gen:types
bun test --cwd web
bun run --cwd web typecheck
bun run --cwd web lint
```
