# RUNTIME SESSION EVENTS

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-runtime`, `quire-session-protocol`
- **Related Notes:** `../notes/runtime_rig_046_lifecycle_consolidation.md`, `../notes/runtime_rig_046_native_migration.md`, `../notes/session_event_pipeline_consolidation.md`, `../notes/runtime_pending_failure_recovery.md`, `../notes/task_focus_and_todo_split.md`, `../notes/ask_user_run_decoupling.md`

## 1. Scope & Non-Goals

This document is the source of truth for canonical session history, live model-event projection,
Redis hot traces, Session-SSE replay, cold run traces and the frontend reducer. Rig model-loop details
live in `RUNTIME_RIG_STREAMING.md`; questionnaire payloads live in `RUNTIME_ASK_USER.md`. Document
revision SSE coordination and snapshot-delta synchronization live in `REPORT_LIVE_SYNC.md`.

Model conversation history has exactly one canonical store: official `rig-postgres` / `rig-memory`.
Session events and run traces are display projections, never an alternate source of model history.

## 2. Canonical Design & Contract

### 2.1 Storage separation

Fresh runtime migrations intentionally do not create `agent_messages`, `session_events`,
`agent_sessions.last_event_seq` or `agent_sessions.min_available_seq`.

- **Canonical conversation:** `rig_memory_logs`, `rig_memory_log_chunks` and `rig_memory_files`, opened
  through the official `PostgresSessionStore` on the existing PostgreSQL pool.
- **UI message receipts:** saved `quire.message_receipt` Asides anchored to canonical `MessageId`s
  (supporting multiple ordered receipts per `MessageId`). They carry idempotency and presentation
  metadata and are never sent to a model. A main run has at most one visible Assistant receipt whose
  content aggregates all non-empty text turns of that run; empty output creates neither a receipt nor
  a synthetic empty Assistant entry. `MessageRole::Question` receipts anchor `ask_user` cards (`question_id`,
  `run_id`) immediately onto the originating run's canonical tail after any Assistant receipt. A
  provider stream error uses the same idempotent fallback to preserve a failed run's non-empty
  response when suffix persistence did not complete; an existing provider receipt is never duplicated.
- **Hot display events:** an ordered Redis list plus a persistent session sequence key and Pub/Sub
  channel. The list has a 24-hour sliding TTL refreshed by appends.
- **Cold run trace:** `agent_runs.trace_summary`, materialized from the hot events on terminal
  transition. `agent_runs.start_seq` records the hot sequence of `turn_started`.
- **Operational records:** `agent_tool_calls`, `agent_questions`, pending queue rows, attachments and
  run/session rows retain their existing domain responsibilities.

Retry locates a visible user receipt's canonical `MessageId`, removes the failed/cancelled suffix and
all asides anchored to removed entries, then reuses the retained prompt. It never appends a second
copy based on message equality. Real Rig runs commit their own receipts; the service's non-empty
assistant fallback is restricted to providers that do not commit canonical history (Stub/Mock).

Deleting a session removes its canonical main/subagent logs and Redis hot keys. Content-addressed
memory blobs may be shared and are not deleted blindly with one conversation.

### 2.2 Runtime event pipeline

```text
AgentRunner / AgentHook
  -> ModelStreamEvent over bounded 256-frame channel
  -> events::project(EventScope, event)
  -> events::EventSink
       - delta coalescing and structural flush ordering
       - tool/question/queue side effects
  -> PostgreSQL operational commit, when applicable
  -> Redis atomic hot append + Pub/Sub
  -> Session-SSE transport / frontend reducer
```

`EventScope { run_id, agent_id, parent_run_id }` remains the coalescer key. Main and child deltas are
independent, and subagent wrappers flatten exactly once. Tool and question rows commit before their
wire events. A side-effect failure therefore cannot be followed by a successful tool/question frame.

Hot appends use one Redis Lua invocation to allocate the sequence, append the list item and publish
it. The script replaces only the serialized top-level `"seq":0` placeholder; it does not perform a
Lua `cjson` round trip. Empty arrays consequently remain arrays, and payload bytes/schema order are
not rewritten.

### 2.3 Active snapshots and cold materialization

`SessionSummary.last_event_seq` is read from Redis, not PostgreSQL. For a `running` session,
`get_session_detail` reads hot events in `start_seq..=last_event_seq` and delegates to the canonical
pure `aggregate_run_traces` projector. Idle and `awaiting_input` sessions do not maintain an active
run trace (question turns close cleanly as `completed` via `finish_run_for_question`).

On a winning terminal run transition:

1. PostgreSQL updates the run and session status;
2. the terminal event is appended to Redis;
3. hot events for the run are aggregated;
4. the complete `RunTraceSummary` is saved to `agent_runs.trace_summary`.

`/traces` reads cold summaries and aggregates only rows without one. Hot-only orphan traces can still
be projected for test/recovery scenarios. Terminal transitions remain conditional, so a late finisher
cannot emit a second terminal frame.

### 2.4 SSE replay and pruning

`/api/sessions/{session_id}/events` uses raw hot JSON payloads for transport. A newer event variant is
therefore forwarded even when the local trace projector cannot decode it; projection skips unknown
variants with a warning.

- `afterSeq > last_event_seq` returns `sync_required`.
- `min_available_seq` is the first retained hot sequence. If the hot list is empty after sequences
  have advanced, it is `last_event_seq + 1`; older cursors must resync.
- Heartbeats reconcile the Redis high-water mark and replay missing Pub/Sub frames through the same
  gap path.
- `prune-events` first materializes completed traces, then atomically trims old hot list entries while
  protecting an active run's full range. It does not delete canonical rig-memory history.

Redis is a hot projection, not a second durable fact store. Canonical memory commits and display
projection commits are eventually consistent and can be reconciled from run/message metadata.

### 2.5 Session messages and trace wire contract

`SessionDetail.messages` is projected by walking canonical conversation entries and their visible
message-receipt Asides. Internal prompts such as `ask_user_response` JSON remain in model history but
have no visible user bubble. Compaction markers are visible receipts attached to Rig summary entries.

`SessionEventKind` includes `turn_superseded`; `user_message_injected` and
`RunTraceSegment::UserMessage` are removed. Tool finish status `skipped` is terminal and rendered as
“已跳过”. The frontend reducer remains the single live projection:

- `turn_started` establishes a new run owner;
- `turn_superseded` settles thinking and requests detail/trace refresh with run-owner guards;
- standard `user_message_added` invalidates the detail projection;
- `turn_finished` preserves `liveRunStatus = "awaiting_input"` while `livePendingQuestion` is non-null before clearing `liveMain` via `refetch-and-clear`;
- `questionnaire_requested`, `questionnaire_answered`, and `questionnaire_dismissed` are all non-terminal, non-run-scoped session events (`trace_run_id()` is `None`);
- skipped tool results update both live and hydrated traces;
- `task_focus` updates live work focus (`title`, `detail`, `section_id`) while preserving existing `todos` unless non-empty legacy `todos` are present; `todos_updated` updates `todos` atomically without overwriting an existing focus `title`, `detail`, or `section_id`;
- scope-aware usage and terminal single-winner behavior remain unchanged.

`contracts/session-protocol.schema.json` is generated by `just schema` and TypeScript contracts by
`bun run --cwd web gen:types`.

## 3. Invariants & Verification

- Canonical structured history exists only in rig-memory; no parallel text-message fact table exists.
- Asides never reach the model and visible messages project in canonical entry order.
- A run never splits into multiple visible Assistant receipts or creates an empty Assistant bubble.
- Retry removes canonical suffix messages and their receipt asides before reusing the original prompt.
- Hot append allocates sequence, stores and publishes atomically without changing empty array shapes.
- Operational side effects precede their display events; structural events follow earlier delta flush.
- Active hydration uses the active run ID and hot `start_seq`, never the newest trace by recency.
- A terminal winner appends one terminal event and materializes one complete cold trace.
- Failed/interrupted sessions release pending work only after no active run remains; the recovered turn
  preserves the queued user message and any non-empty failed assistant response without duplication.
- Raw SSE transport tolerates unknown variants while trace projection skips them.
- Pruning removes only hot display data and never canonical history or cold materialized traces.
- Supersession creates separate old/new runs; there is no inline user-message trace segment.
- `TaskFocus` and `TodosUpdated` stay strictly orthogonal across backend trace projection and the frontend reducer so concurrent focus/checklist updates remain commutative.
- Main/subagent delta coalescing, usage attribution and run-owner cleanup guards remain scope-safe.

Integration coverage includes hot replay/gap repair, unknown-event forwarding, active-run snapshots,
terminal materialization, pruning and retained cursors, queue supersession, concurrent queue claims,
`failed_turn_preserves_text_and_releases_pending_queue`, question lifecycle and canonical memory
append/compaction. Frontend tests cover every fixture split,
`turn_superseded`, skipped tools, duplicate replay protection and subagent hydration.

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

Runtime integration tests require disposable PostgreSQL and Redis. `just test` recreates the test
database with non-interactive `sqlx database drop -y --force` before running the workspace suite.
