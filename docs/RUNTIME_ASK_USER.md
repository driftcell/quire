# RUNTIME ASK_USER CARDS

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-session-protocol`, `quire-runtime`
- **Related Notes:** `../notes/runtime_ask_user_suspend_resume.md`

## 1. Scope & Non-Goals

This document governs structured `ask_user` questionnaire cards, durable run suspension, answer and
skip handling, dismissal when a user bypasses a card, and the Session REST/SSE contract. It does not
govern how the frontend renders freeform text or skip controls, report edits made after resumption,
or the report protocol itself.

## 2. Canonical Design & Contract

### Protocol

`ActiveRunStatus` adds `awaiting_input`. The session protocol exports:

- `QuestionOption { id, label, description? }`
- `QuestionItem { id, title, description?, multiple, required, options }` with no freeform fields
- `QuestionAnswer { question_id, selected_option_ids, freeform_text?, skipped }`
- `PendingQuestion { question_id, run_id, questions, created_at }`
- `SessionDetail.pending_question: Option<PendingQuestion>`, restored from the latest pending row

SSE additions:

- `questionnaire_requested { question_id, questions }`
- `questionnaire_answered { question_id, answers, skipped_all }`
- `questionnaire_dismissed { question_id, reason }`

`contracts/session-protocol.schema.json` includes all new types and the pending field.

### Storage

The consolidated `migrations/0001_init.sql` adds `agent_questions`:

- `question_id TEXT PRIMARY KEY`
- `session_id` FK to `agent_sessions` with cascade delete
- `run_id`, optional `tool_call_id`, `questions JSONB`, nullable `answers JSONB`
- `status` constrained to `pending`, `answered`, `skipped`, `dismissed`
- `created_at`, `resolved_at`

`RuntimeStore` creates pending questions, loads the latest pending card for `SessionDetail`, resolves
one pending card, dismisses all pending cards for a session, and transitions the suspended run.

### ask_user suspension

`ask_user` is a built-in tool in the model allow-list. Its input is only
`questions: Vec<QuestionItem>`. When the rig provider sees the call it:

1. emits `ToolCallStarted` for `ask_user`;
2. emits `QuestionnaireRequested` with the questions and tool-call ID;
3. emits `ToolCallFinished` with status `awaiting_input`;
4. ends the model loop without requesting another model turn.

The runtime service creates an `agent_questions` row, emits `questionnaire_requested`, persists any
pre-question assistant text, stores reported usage, and marks the run + session `awaiting_input`.
No long-lived connection or task is held. The suspended run keeps an open `finished_at` until it is
answered or dismissed; startup recovery only rewrites `running` runs. If a crash occurs before the
transition, the pending card is still available from `SessionDetail.pending_question`.

### Answer, skip and dismissal

`POST /api/sessions/{session_id}/questions/{question_id}/answer` accepts:

```json
{"answers": [{"question_id": "scope", "selected_option_ids": ["q3"], "freeform_text": null, "skipped": false}], "skipped": false}
```

The service requires the card to be `pending`, validates provided question/option IDs, and stores
`answered` or `skipped` with the structured answers. It emits `questionnaire_answered`, writes a
tool-role context message containing the answers or skip notice, finalizes the suspended run, and
starts a new continuation run. `skipped: true` skips the whole card; per-answer `skipped` supports
single-question skips.

`POST /api/sessions/{session_id}/messages` is never blocked by `awaiting_input`. After a new user
message is inserted, every pending card is marked `dismissed`, `questionnaire_dismissed` is emitted
with reason `superseded_by_user_message`, the suspended run is finalized, and a new turn starts with
the user's message.

## 3. Invariants & Verification

- A pending card is durable and appears in `SessionDetail.pending_question` after refresh/restart.
- Answering validates pending state; a resolved card cannot be answered again.
- Skip and dismiss both produce their SSE event and a new or finalized run as appropriate.
- A new message always wins over a pending card and starts a normal turn.
- Question payloads contain no freeform capability fields; rendering policy is frontend-owned.

Integration coverage in `crates/quire-runtime/tests/runtime_integration.rs`:

- `ask_user_suspends_until_answers_resume_the_run`
- `ask_user_skip_marks_skipped_and_continues`
- `new_message_dismisses_pending_question_and_starts_a_new_turn`

Unit coverage in `crates/quire-runtime/src/rig_agent.rs`:

- `ask_user_emits_questionnaire_and_pauses_the_run`

Full gate:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
