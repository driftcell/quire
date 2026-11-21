# RUNTIME SESSION EVENTS

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-runtime`, `quire-session-protocol`
- **Related Notes:** `../notes/session_event_pipeline_consolidation.md`

## 1. Scope & Non-Goals

This document is the single source of truth for the agent session event pipeline: rig event
adaptation, scoped event projection, delta coalescing, durable session events, run lifecycle,
Session-SSE replay, trace projection and the frontend live reducer. It also covers completed-run
trace materialization, event retention and the ephemeral preview handoff and subagent resume
contracts owned by `quire-runtime`.

It does not govern report blocks, report Smart CAS, the Quire MCP tool surface, model/provider
credential storage, KB branch management, or the higher-education writing workflow in `skills/` and
`agents/`. `docs/RUNTIME_RIG_STREAMING.md` remains the detailed rig model-loop reference; this page
is the event and persistence contract.

## 2. Canonical Design & Contract

### 2.1 Pipeline and ownership

```text
rig StreamEvent
  -> rig_agent::handle_stream_event
  -> ModelStreamEvent
  -> events::project(EventScope, event) -> EventAction
  -> events::EventSink
  -> SessionEventLog / RuntimeStore
  -> session_events (PostgreSQL) + Redis Pub/Sub wake-up
  -> /api/sessions/{session_id}/events (opaque frames + replay)
  -> quire-session-protocol::trace::aggregate_run_traces
  -> web/src/lib/sessionEventReducer.ts (live) + liveTrace.ts (hydration)
```

`crates/quire-runtime/src/events/` owns the only model-event mapping and structural flush path:

- `scope.rs`: `EventScope { run_id, agent_id, parent_run_id }`. Main events use `main` with no
  parent. Subagent events use the child run and the root `parent_run_id`. The complete scope is the
  `DeltaCoalescer` key, so a subagent delta cannot become a top-level run.
- `project.rs`: one `ModelStreamEvent -> EventAction` table, including `SubagentEvent` flattening,
  lifecycle envelopes and explicit main-only `Ignore` actions. Main text alone contributes to the
  persisted assistant message.
- `sink.rs`: `EventSink` coalesces main and subagent deltas independently (50 ms / 64 chars),
  flushes every scope before structural events, persists tool/question side effects before their
  wire events, and returns main-agent usage and created questions.
- `log.rs`: the narrow `SessionEventLog` boundary. Production uses batched appends and atomic
  tool/question-plus-event writes; tests use a recording fake.

Provider events use a bounded 256-frame channel. Slow persistence applies backpressure instead of
creating an unbounded queue. Dropping a provider stream aborts its producer task; `RunTurnGuard`
cancels and unregisters a run on every exit path, including panic.

### 2.2 Event envelope and classification

`SessionEvent` contains `seq`, `run_id`, `agent_id`, optional `parent_run_id`, `timestamp` and a
flattened `SessionEventKind`. `SessionEvent::trace_run_id()` returns an owning run only for
run-scoped lifecycle/activity events. Queue, compaction, discovery, questionnaire-resolution and
`sync_required` frames never create a run trace, so synthetic IDs such as `queue` or `compaction`
cannot become phantom runs. Active traces are selected by the active run ID from `agent_runs`, never
by aggregation recency.

Wire events carry previews only. Full tool input/output stays in `agent_tool_calls` and is fetched
by REST. Main-agent usage updates `agent_sessions.context_usage`; subagent usage remains a child
frame and never overwrites the session meter.

### 2.3 Durability, ordering and lifecycle

`append_event_in` allocates `seq` under the `agent_sessions` row lock. `append_event_batch` writes a
coalesced batch in one transaction. Tool rows and their events, and question rows and their request
events, commit atomically. Every structural event is persisted only after preceding deltas have been
flushed.

`finish_run_with_event` conditionally changes a running run to a terminal state and appends the
terminal event in the same transaction. Exactly one terminal event is possible; a late finisher is a
no-op. `interrupt()` signals cancellation, lets the owning turn flush residual deltas and finalize,
and uses a bounded fallback with the same conditional transition. `create_run` and migration
`0008_active_run_uniqueness.sql` enforce at most one top-level `running` / `awaiting_input` run per
session across processes and rolling deploys. `answer_question` and `retry` take the in-process
session gate, and deleting a session removes its gate.

`claim_pending_batch` writes the message, deletes queue items and emits queue events in one
transaction, after revalidating the queue IDs under the session row lock. Concurrent claims have one
winner.

### 2.4 Session-SSE transport and recovery

`/api/sessions/{session_id}/events` forwards opaque `(seq, event_type, event_json)` frames during
replay and gap repair. A runtime therefore relays event variants introduced by a newer writer during
a rolling deploy. Projection skips unknown variants with a warning instead of failing the request.

Cursor rules:

- `afterSeq > last_event_seq` returns `sync_required` immediately.
- If the next expected event is older than `agent_sessions.min_available_seq`, the stream returns
  `sync_required` instead of silently skipping a deleted replay window.
- `RUNTIME_SSE_REPLAY_LIMIT` (default 500) bounds replay before `sync_required`.

`RUNTIME_SSE_HEARTBEAT_MS` (default 15000) controls the heartbeat. Each heartbeat also reconciles
against durable `last_event_seq`: a lost Pub/Sub publish, including the final `turn_finished`, is
replayed through the same gap path instead of leaving the client `running` until reconnect.

### 2.5 Trace projection and retention

The canonical pure aggregator is `quire-session-protocol/src/trace.rs`. `RunTraceSubagent.segments`
is isomorphic to the main trace: thinking, execution, user-message and text segments preserve
multi-round order instead of being flattened into fixed slots.

`agent_runs.start_seq` is recorded atomically with `turn_started` (migration
`0009_run_start_seq.sql`). Active-run hydration reads `seq BETWEEN start_seq AND last_event_seq`
through the `(session_id, seq)` primary key. Idle detail requests skip event history entirely.

Completed runs are materialized into `agent_runs.trace_summary` by `finish_run_with_event` and
`resolve_suspended_run` (migration `0010_trace_materialization.sql`). `/traces` reads those stored
traces and aggregates only the active run. Legacy completed runs are lazily backfilled, and
run-scoped events without an `agent_runs` row retain an aggregate fallback.

`POST /api/sessions/{session_id}/prune-events` first materializes completed traces, then deletes old
raw events while preserving the active run's complete history, and advances `min_available_seq`.
The active-run check, cutoff calculation, deletion and cursor update all share one `agent_sessions`
`FOR UPDATE` transaction. An active run whose `start_seq` has not yet been recorded is treated as an
active run, not as history that can be discarded. Retention never discards the materialized
completed trace.

### 2.6 Frontend projection and cache behavior

`web/src/lib/sessionEventReducer.ts` is the only frontend event projection: an exhaustive pure
reducer returns state plus effects. `useSessionEventStream` only executes those effects. The reducer
is scope-aware, applies only main-scope usage, and clears a finished run only if no newer run owns
the live pane. `liveTrace.ts` uses the same ordered segments for live and historical subagents.

Complete payloads update the query cache directly: `SessionTitleUpdated`, `DocLinked`,
`PendingQueueUpdated` and `TurnStarted` do not invalidate the full session detail. Refetches remain
for `UserMessageAdded`, `UserMessageInjected`, `CompactionFinished`, questionnaire resolution,
`TurnFinished`, `TurnFailed` and `SyncRequired`, where complete messages or traces are needed.

### 2.7 Preview handoff and subagent resume

`ToolPreviewStreamer` uses a latest-frame-wins `watch` channel with a lazily spawned background
sender. Intermediate preview frames never block the model stream. `push_block_now` enqueues the
final frame and waits on a watch watermark for the last enqueued frame ID to be sent (not on a
`Notify` permit), so completion cannot be lost to a wakeup race. The existing 200 ms
`PREVIEW_ANCHOR_SETTLE` barrier then runs before `apply_operations`, so a stale preview cannot arrive
after `ephemeral_clear`.

`SubagentTask.resume_run_id` resumes a subagent from an earlier round in the same session.
`RigModelCore` keeps up to 128 `(session_id, subagent_run_id)` history entries, each containing the
agent name and complete `Vec<Message>` including the final assistant reply. A hit with a matching
agent name reuses the run ID and appends the new user task to that history; a miss or agent mismatch
starts a fresh subagent with a warning. `SubagentOutcome.subagent_run_id` is returned to the main
agent for subsequent resume calls. Reusing a run ID does not create a duplicate frontend card:
`sessionEventReducer` updates the existing subagent in place, and the backend resets the previous
summary and settles the old thinking clock before the new round.

### 2.8 Generated contracts

`contracts/session-protocol.schema.json` is generated by `just schema`, and
`web/src/contracts/session-protocol.ts` is generated from it by `bun run gen:types`. The
`RunTraceSubagent.segments` migration is included in both. Shared golden fixtures live in
`contracts/fixtures/session-events/` and include `events`, `expected_traces` and `prefix_traces`.

## 3. Invariants & Verification

- `seq` is strictly increasing per session and is allocated with the event row.
- A structural event is never persisted before the deltas that preceded it.
- Main and subagent deltas are coalesced independently with full scope, including root
  `parent_run_id`.
- Run/session status and terminal event commit atomically; a late finisher cannot emit a second
  terminal event.
- Control-plane events never create a run trace, and active traces are selected by active run ID.
- Unknown event types are forwarded by transport and skipped by projection.
- The provider queue is bounded at 256 frames; a full queue applies backpressure.
- Completed-run traces are materialized in the same lifecycle transaction and match the canonical
  aggregator. Pruning preserves those traces and enforces `min_available_seq`.
- Live and hydrated projections agree for every fixture split point, including multi-round
  subagents.
- Preview `push_block_now` waits for the final frame before the commit barrier, and intermediate
  frames do not replay as an unbounded queue.
- A resumed subagent receives its previous conversation history and appends the new task; a missing
  or mismatched resume starts fresh.
- `[待拟标题：...]` is rejected by `QR401`, not reported as a generic heading by `QR418`.

Integration coverage in `crates/quire-runtime/tests/runtime_integration.rs` includes run lifecycle,
queue claiming, active-run uniqueness, terminal single-winner behavior, cancellation ordering,
unknown-event forwarding, cursor-ahead handling, heartbeat reconciliation, `start_seq` hydration,
trace materialization, retention and pruning with a missing active-run `start_seq`.
`crates/quire-session-protocol` runs the golden projection fixtures, including subagent resume.
`web/src/lib/sessionEventReducer.test.ts` checks every prefix/suffix split, lifecycle guards and
refetch-reducing effects. `rig_agent::preview_stream::handoff_tests` verifies the final preview
barrier and immediate wakeup after the last frame, while `rig_agent::subagents::tests` verifies
resume history, missing history, agent mismatch and retention of the final assistant reply.

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

Runtime integration tests require a disposable PostgreSQL database and Redis; set
`TEST_DATABASE_URL` to a disposable database before running them.
