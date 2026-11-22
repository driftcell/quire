# RUNTIME ASK_USER CARDS

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-session-protocol`, `quire-runtime`
- **Related Notes:** `../notes/runtime_rig_046_lifecycle_consolidation.md`, `../notes/runtime_rig_046_native_migration.md`, `../notes/runtime_ask_user_suspend_resume.md`

## 1. Scope & Non-Goals

This document governs one-shot `ask_user` questionnaires, mixed-tool batch policy, durable pending
cards, structured answer/skip payloads, dismissal and continuation through a new AgentRunner turn.
It does not use a serialized `AgentRun`, an inline hook wait or same-run `agent.resume`.

## 2. Canonical Design & Contract

### 2.1 Protocol and storage

The session protocol retains:

- `QuestionOption`, `QuestionItem`, `QuestionAnswer`, `PendingQuestion` and `ResolvedQuestion`;
- `SessionDetail.pending_question` and `resolved_questions`;
- `questionnaire_requested`, `questionnaire_answered` and `questionnaire_dismissed` events;
- `ActiveRunStatus::AwaitingInput`.

`agent_questions` remains the durable card store with pending/answered/skipped/dismissed states,
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

### 2.3 Suspension

The service's existing `EventSink` creates the pending question before publishing its request event,
then the run/session become `awaiting_input`. No model connection or task remains open. Any assistant
text and accepted tool results are already represented in canonical rig-memory history and the hot
trace.

Startup recovery rewrites only `running` runs. A durable pending card remains visible after refresh or
restart even if the final status transition was interrupted.

### 2.4 Answer, skip and dismissal

`POST /api/sessions/{session_id}/questions/{question_id}/answer` validates that the card is pending,
validates question/option IDs, stores `answered` or `skipped`, emits `questionnaire_answered`, and
finalizes the suspended run's cold trace.

The answer payload is serialized as JSON and appended to canonical memory as an **internal, invisible
prompt**. It is the only model input for the continuation turn. There is no
`The user answered questionnaire...` prose, synthetic tool-role text, or second
`Continue the task...` user message. The assistant response starts a new run with the standard
lifecycle and visible assistant projection.

A normal user message may bypass the card. Pending cards become `dismissed` with reason
`superseded_by_user_message`. Before the new visible prompt is appended, canonical memory receives
an internal invisible `ask_user_response` JSON prompt with `status = "dismissed"`, the question/tool
IDs and that reason. The model therefore never sees an unexplained dangling `awaiting_input` result.
The suspended runs are finalized, the user's standard new turn wins, and pending queue contents are
not consumed by question suspension.

## 3. Invariants & Verification

- A pending card survives refresh/restart and resolved cards remain anchored to the original call.
- Mixed batches execute no sibling tool and every sibling has exactly one terminal skipped finish,
  regardless of whether it appears before or after `ask_user`.
- `ask_user` never holds a task while waiting for a person.
- Answers and whole-card skips use one pure JSON payload with resolved labels and no prose continuation.
- Continuation uses a new AgentRunner turn, not an unexposed Runner checkpoint or same-run resume.
- Internal answer prompts remain canonical but do not render as user bubbles.
- A new user message appends an invisible canonical dismissal before its visible prompt and wins;
  the dismissal never becomes a UI bubble, and a resolved card cannot be answered again.
- Failed question calls do not leave running tool rows.

Coverage includes:

- `ask_user_suspends_until_answers_resume_the_run`
- `ask_user_skip_marks_skipped_and_continues`
- `new_message_dismisses_pending_question_and_starts_a_new_turn`
- `ask_user_suspension_keeps_pending_queue_untouched`
- `HookTestHarness` mixed-batch sibling skipping in both call orders with exactly-once finishes
- session-protocol structured payload/label serialization tests

Full gate:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
just schema
```
