# SESSION EVENT PIPELINE CONSOLIDATION

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-21
- **Promoted To:** `../docs/RUNTIME_RIG_STREAMING.md`, `../docs/RUNTIME_SESSION_EVENTS.md`

## 1. Context & Problem

The agent runtime streams rig events to the browser through Session-SSE. The wire contract is
already centralised (`quire-session-protocol::SessionEvent` / `SessionEventKind`, exported to
`contracts/session-protocol.schema.json` and generated into `web/src/contracts/session-protocol.ts`),
and the rig boundary is already single-point (`rig_agent/mod.rs::handle_stream_event`). The
maintainability problem is downstream of those two boundaries:

1. **Duplicated translation.** `ModelStreamEvent -> SessionEventKind` is written twice in
   `crates/quire-runtime/src/service.rs`: once in `execute_turn` (main agent) and once in
   `handle_subagent_event` (subagents, recursive over `SubagentEvent`). The two copies differ only in
   envelope construction, and already disagree on convention: `SubagentStarted` is emitted with
   `agent_id = agent_name`, while `SubagentFinished` is emitted with `agent_id = "main"`.
2. **Ordering is a call-site convention.** `flush_all() + emit_coalesced()` appears 15 times inside
   `execute_turn`. Missing one reorder silently breaks the persisted
   `[Execution, UserMessage, Text]` order that the UI relies on.
3. **Projection exists twice.** `store.rs::aggregate_run_traces` (refresh/hydration) and
   `web/src/hooks/useSessionEventStream.ts` + `web/src/lib/liveTrace.ts` (live) re-implement the same
   rules: thinking settlement, subagent detection, segment boundaries. `is_subagent_event` is defined
   independently in Rust and TypeScript. The TypeScript chain is an `if/else` ladder with no
   exhaustiveness check, so a new event variant fails silently.
4. **Two parallel SSE surfaces.** `quire-runtime` serves Session-SSE on
   `/api/sessions/{id}/events` (PostgreSQL `seq` replay), while `quire-server` serves a different
   stream on the same path (Redis-Stream `doc_created` discovery). `create_report` produces both
   `doc_created` (server) and `doc_linked` (runtime). These streams have different consistency
   domains and should stay separate, but ownership is currently implicit.

A code-level review also found defects that a refactor must fix rather than preserve. Each item
below is derived from the current control flow; Phase 0 adds the regression test that pins it.

- **Phantom control-plane traces.** `PendingQueueCoordinator::emit_queue_update` appends
  `PendingQueueUpdated` with `run_id = "queue"`, `agent_id = "system"`. `aggregate_run_traces` calls
  `ensure_run_trace` for every event except compaction, so a synthetic, permanently `running` trace
  with `run_id = "queue"` is created. `active_trace_snapshot` then selects `traces.next_back()`, so
  when the first queue event of a session arrives during an active run, `SessionDetail.active_run_trace`
  can be the empty `queue` trace instead of the real run. Hydration inherits the wrong `run_id`.
- **Provider tasks outlive their consumer.** `RigModelStreamProvider::stream_turn` spawns
  `run_main_turn` on an unbounded channel with no abort-on-drop guard. `RuntimeService::run_turn`
  removes the run from the registry on error but never cancels the token. Any service-side failure
  (for example a database error inside `append_event`) therefore marks the run `failed` while the
  provider keeps executing model turns and tools, including document-mutating tools.
- **Subagent deltas are not coalesced.** Main deltas are coalesced at 50 ms / 64 chars, but
  `handle_subagent_event` persists every subagent `ThinkingDelta` / `TextDelta` immediately: one
  database transaction plus one Redis publish per token, multiplied by concurrent subagents.
  `DeltaCoalescer` is already keyed by `(run_id, agent_id)`, so the earlier decision to keep subagent
  deltas out of the coalescer no longer matches the data structure.
- **Subagent usage overwrites the main meter.** `handle_subagent_event` re-emits subagent `Usage` as
  `ContextUsage`, and the frontend `context_usage` branch applies it without checking scope, so the
  live context meter shows subagent usage until the main usage frame arrives.
- **Pub/Sub is fragile.** `EventSubscriber::next_event` and `next_json` return `None` on malformed
  payloads. A single malformed frame closes the SSE loop, and a malformed interrupt permanently exits
  `spawn_interrupt_listener`. That listener also returns immediately if the initial subscribe fails,
  with no retry.
- **Terminal-event loss is not self-healing.** `append_event` only warns when the Redis publish
  fails. Gap detection repairs the stream when a later event arrives, but if the lost frame is the
  final `turn_finished`, connected clients keep showing `running` until they reconnect.
- **Read path scales with total history.** `get_session_detail` loads every `session_events` row for
  the session and aggregates all of it, although `SessionDetail` only needs the active run trace.
  `session_events` has no run-oriented index and no retention, so detail cost grows without bound.
- **Frontend run transitions race.** `turn_finished` starts an async refetch and then clears
  `liveMain` / `liveSubagents` unconditionally. A queued follow-up run can emit `turn_started` and
  `subagent_started` before the refetch resolves, and the late clear wipes the new run's live state.
- **Multi-instance is assumed, not enforced.** Session gates and `RunRegistry` are in-memory. The
  "one active run per session" rule and pending-queue dequeue are not protected by any database
  invariant, so two runtime instances can start duplicate runs or double-dequeue.

A second review confirmed the same defects and added these associated failure modes:

- **Terminal transitions can race.** `interrupt()` cancels the token and then independently writes
  `interrupt_run` plus `TurnFailed`, while the run loop can still flush deltas or can already have
  emitted `TurnFinished` without yet calling `finish_run`. This permits activity after a terminal
  frame and, in the second window, both `TurnFinished` and `TurnFailed` for one run. Terminal status
  and terminal event therefore need one atomic, mutually exclusive writer.
- **Not every turn-start entry point is gated.** `answer_question` and `retry` do not take the
  session gate. `take_pending_batch_locked` performs message insertion, queue deletion and event
  append in separate transactions, so a crash can leave a queue item and its persisted message
  divergent. `session_gates` also grows without bound after session deletion.
- **Scoped coalescing must retain the whole envelope.** `CoalescedDelta` currently carries only
  `run_id` and `agent_id`, and `emit_coalesced` hardcodes `parent_run_id = None`. Reusing it for
  subagents without changing the key to a complete `EventScope` would turn coalesced subagent deltas
  into top-level traces.
- **All invalidation listeners share the same failure mode.** Besides interrupts, the KB, model and
  MCP listeners return permanently when the initial subscribe fails or the subscription ends.
- **Live and hydrated subagent traces are not structurally equivalent.** The persisted
  `RunTraceSubagent` is flattened into one thinking string, one tool list and one text string, while
  the live subagent trace appends ordered multi-round segments. `liveSubagentFromSummary` hardcodes
  `[thinking, execution, text]`, so multi-round subagents cannot satisfy prefix/suffix hydration
  equivalence until the subagent segment model is unified.
- **Read amplification has a frontend multiplier.** Many events invalidate the full session detail,
  which currently rereads all messages, attachments, questions, queue items and the complete event
  history. Event payloads should update the query cache directly when they already contain the
  changed state; refetch remains necessary for structurally incomplete frames.
- **Preview transport can block model consumption.** Streamed `apply_operations` arguments await
  preview HTTP POSTs inside the model-read loop. Ephemeral previews should use a latest-frame-wins
  handoff to a background sender instead of backpressuring the LLM stream.
- `TaskFocus` was re-reviewed and is **not** classified as a thinking-settlement defect by itself:
  built-in `describe_task` execution is bracketed by `ToolCallStarted` / `ToolCallFinished`, which
  already settle thinking for both scopes. Adding `TaskFocus` only to `settles_sub_thinking` would
  create a new main/subagent asymmetry. The actual blocker is the flattened subagent segment model.

## 2. Rejected Options & Why

- **Merge Session-SSE and document-SSE into one stream.** Rejected: document revisions use
  revision-based Smart CAS and must work for non-agent readers, while session events use a
  session-scoped `seq` and run lifecycle. Merging would couple `quire-server` to agent run internals.
- **Serialize `ModelStreamEvent` directly to the browser.** Rejected: it carries full tool
  input/output and runtime-internal variants. The wire event deliberately carries previews only; full
  payloads stay in `agent_tool_calls` behind REST.
- **Send a full `RunTraceSummary` (or trace patch) with every delta.** Rejected: per-token bandwidth
  and latency cost, plus a second derived state to keep consistent. Keep the event log as the source
  of truth and make the two projections provably equivalent instead.
- **Share the Rust aggregator with the frontend through WASM.** Rejected for now: build and bundle
  complexity is out of proportion to the benefit. Shared JSON fixtures give the same drift protection
  with no new toolchain.
- **Keep subagent deltas uncoalesced.** Supersedes the earlier note: the coalescer is already keyed
  per `(run_id, agent_id)`, so subagent scopes can be buffered independently without misattribution.
  Per-token transactions are the larger correctness/performance risk.
- **Move session events to Redis Streams for durability.** Rejected: PostgreSQL is already the
  durable log. Pub/Sub is only a wake-up signal; a periodic database reconcile repairs lost tails
  without introducing a second log to keep consistent.
- **Immediate event retention.** Rejected until completed-run traces are materialised. Deleting raw
  events today would destroy history and break replay; retention also needs an explicit
  minimum-available-seq marker so stale cursors produce `sync_required` instead of silent gaps.
- **Big-bang rewrite of `service.rs` / `store.rs`.** Rejected: the current invariants are covered by
  integration and browser tests. Behaviour must be pinned with golden fixtures first, then changed in
  small, independently verifiable steps.

## 3. Chosen Approach & Trade-offs

### 3.1 Target architecture

```text
rig StreamEvent
  -> rig_agent::handle_stream_event            (unchanged rig boundary)
  -> ScopedEvent { scope, ModelStreamEvent }   (scope decided once, at the producer)
  -> events::project(scope, event)             (one pure mapping table -> EventAction)
  -> events::EventSink                         (coalesce + flush + side effects + append)
  -> RuntimeStore / SessionEventLog            (transactional event log, batched appends)
  -> PostgreSQL session_events                 (durable source of truth)
  -> Redis Pub/Sub wake-up + periodic DB reconcile
  -> /api/sessions/{id}/events                 (opaque JSON forwarding, seq from columns)
  -> projections:
       canonical Rust aggregator in quire-session-protocol (snapshots/traces)
       TypeScript pure reducer (live), pinned by shared fixtures
```

### 3.2 Event classification in the protocol crate

Add non-serialised classification to `quire-session-protocol` (no wire change):

- `RunLifecycle`: `TurnStarted`, `TurnFinished`, `TurnFailed`, `QuestionnaireRequested`.
- `RunActivity`: deltas, `TaskFocus`, tool start/finish, subagent lifecycle, `UserMessageInjected`,
  `ContextUsage`.
- `SessionControl`: `PendingQueueUpdated`, `CompactionStarted/Finished`, `SyncRequired`.
- `SessionSignal`: `DocLinked`, `SessionTitleUpdated`, `UserMessageAdded`, `QuestionnaireAnswered`,
  `QuestionnaireDismissed`.

`SessionEvent::trace_run_id()` returns `Some` only for run-scoped classes. Aggregation filters on it,
which removes phantom `queue` / `compaction` traces by construction. `active_trace_snapshot` must
select the trace by the actual active run id from `agent_runs` (`running` / `awaiting_input`), never
by `next_back()`.

Classification also documents the guarantee each class needs:

- `RunLifecycle`: atomic with the run/session status transition.
- `RunActivity`: ordered, replayable, scope-attributed; loss is not acceptable.
- `SessionControl`: not part of any trace; replayable; never creates a run trace.
- `SessionSignal`: invalidation hint for state already committed elsewhere; loss is repaired by the
  next refetch, so it does not need transactional coupling.

### 3.3 Scoped envelope and one EventSink

- Introduce `EventScope { run_id, agent_id, parent_run_id }` with `main` / `subagent` constructors.
  Keep today's wire meaning: for subagent activity `parent_run_id` is the owning root run, which is
  what trace grouping relies on. Do not reinterpret it as an immediate-parent pointer without also
  changing trace grouping.
- `DeltaCoalescer` / `CoalescedDelta` carry the full scope instead of separate run/agent strings, so
  main and subagent deltas share one scoped coalescer and one flush path. In particular,
  `parent_run_id` is part of the bucket key and of every flushed delta; hardcoding it to `None`
  would promote coalesced subagent deltas into phantom top-level runs.
- `project(scope, event) -> EventAction` is the only mapping table:
  `Delta | Emit | PersistToolStarted | PersistToolFinished | RequestQuestion | Ignore`.
  Main-only events (`DocLinked`, `SessionTitleUpdated`, `UserMessageInjected`,
  `QuestionnaireRequested`) stay explicit arms instead of being silently dropped in a second match.
- `EventSink` owns the coalescer and a narrow `SessionEventLog` trait (`append_event`,
  `append_events_batch`, `save_tool_call`, `create_question`). `RuntimeStore` implements the trait;
  a fake implements it in tests for fault injection. `EventSink::emit` always flushes before a
  structural event, so the ordering rule exists exactly once.
- `execute_turn` becomes a small `select!` over cancellation / tick / stream plus `sink.apply`.
  `handle_subagent_event` is deleted. Subagent deltas are coalesced per scope.
- Land this in two PRs: first a `flatten(SubagentEvent) -> ScopedEvent` helper (no provider change),
  then move the scope into the stream item and delete `ModelStreamEvent::SubagentEvent`.

Accepted cost: cross-scope deltas may be persisted in a different order than they arrived, because
flush order is per-scope key order. Within a scope order is preserved, the UI and aggregator treat
scopes independently, and `seq` remains a total order. If a future feature needs cross-scope arrival
order, it must be modelled explicitly rather than inferred from coalescer timing.

### 3.4 Transactional event log

- Split `append_event` into `append_event_in(&mut Transaction, draft)` plus the existing convenience
  wrapper, and add `append_events_batch` for coalesced flushes (one transaction, sequential `seq`,
  publish after commit).
- Make terminal transitions atomic: `finish_run` + session status + `TurnFinished` / `TurnFailed` in
  one transaction. Tool rows + their `ToolCallStarted/Finished` events in one transaction. Question
  creation + `QuestionnaireRequested` in one transaction.
- `run_turn`'s error path cancels the token and uses the atomic fail path; `stream_turn` additionally
  gets an abort-on-drop guard so a dropped consumer always stops provider work.
- Bind the provider channel to a bounded capacity and make `EventScope::send` await capacity. Full
  tool input/output still travels through the stream, so an unbounded queue is a real memory risk
  when the database is slow. Dropping deltas is not an option; backpressure is the correct semantics.

Accepted cost: backpressure can slow the model/tool loop when persistence is slow. That is the
intended behaviour, but capacity must be configurable and queue depth observable.

### 3.5 Transport hardening

- Serve SSE from `(seq, event_type, event_json)` columns. Use the `seq` column for cursors and gap
  detection and forward the stored JSON without deserialising it into `SessionEvent`. Transport then
  tolerates event variants written by a newer runtime during a rolling deploy.
- Aggregation takes the opposite, explicit position: unknown `event_type` values are skipped with a
  warning instead of failing `get_session_detail` / `list_run_traces`.
- `EventSubscriber` / `next_json` skip malformed frames with a warning and only terminate on a real
  stream end. `spawn_interrupt_listener` and the KB/model/MCP invalidation listeners retry
  subscription with backoff instead of exiting.
- If `after_seq > last_event_seq`, emit `sync_required` (client cursor is ahead of the server).
- Use the heartbeat tick to reconcile: read `agent_sessions.last_event_seq`, and if it is ahead of
  the last sent `seq`, replay the missing rows through the existing gap path. This repairs a lost
  terminal publish without requiring a reconnect.
- Document the rolling-deploy rule: new fields are optional/defaulted, new variants are deployed to
  readers before writers, and no existing variant is renamed.

### 3.6 Read-path scaling

- `get_session_detail` resolves the active run and aggregates only that run's events. Record the
  run's `start_seq` when `TurnStarted` is appended (and optionally its `end_seq` at completion), then
  read `seq BETWEEN start_seq AND last_event_seq` through the existing `(session_id, seq)` primary
  key. This includes both main and subagent frames without guessing the latest trace and avoids two
  new secondary indexes. `agent_sessions.active_run_id` may be added in the same lifecycle
  transaction if resolving the active run still requires a status query.
- Keep full-history aggregation only for `/traces`. Later, materialise each completed run's
  `RunTraceSummary` at completion using the same canonical aggregator, so `/traces` reads
  materialised rows plus the active run. Only after that introduce retention with a tracked
  minimum-available `seq` that maps stale cursors to `sync_required`.
- A denormalised `trace_run_id` column remains an optional later simplification if SQL grouping by
  run becomes the dominant query; it is not required for the active-run fix.

### 3.7 Frontend reducer and shared fixtures

- Extract a pure `reduceSessionEvent(state, event) -> { state, effects }`. Effects describe query
  invalidation, trace refetch, live-state clear and reconnect; the hook only executes them.
- Use an exhaustive `switch` with `assertNever`, and make scope handling explicit: `context_usage`
  only updates the meter for main-scope events, control events never touch run traces, and
  `turn_finished` / `turn_failed` only clear state whose `run_id` matches the event.
- Keep hydration and live projection equivalent by construction: shared JSON fixtures under
  `contracts/fixtures/session-events/` contain an event sequence plus the expected trace. The Rust
  canonical aggregator and the TypeScript reducer both run against them. Add the key property test:
  for **every** split point `k`, `reduce(prefix ++ suffix)` equals `hydrate(aggregate(prefix))`
  followed by `reduce(suffix)`, rather than testing only one hand-picked split.
- Unify the subagent segment model before enabling that property for multi-round subagents. The
  preferred end state is `RunTraceSubagent.segments: Vec<RunTraceSegment>`, isomorphic to the main
  trace, which also removes the duplicated flattened subagent aggregation and the hardcoded
  `[thinking, execution, text]` hydration order. This is an explicit wire/schema migration with
  regenerated contracts, not a Phase 0 behaviour patch.
- Apply event payloads directly to the query cache when they are complete (`SessionTitleUpdated`,
  run status and queue snapshots); reserve full detail refetches for structurally incomplete frames
  such as `UserMessageAdded` and `SyncRequired`.
- Add browser regressions for "queue a message during the first run, then reload" and for a follow-up
  run starting while the previous run's `turn_finished` refetch is still in flight.

### 3.8 Phased roadmap

**Phase 0 — pin behaviour and stop the bleeding (no redesign).**

1. Add golden fixtures for representative sequences: main-only, subagent fan-out, inline user message,
   ask_user suspension. These pin current behaviour before any refactor.
2. Fix phantom traces with run-scoped filtering and select the active trace by active run id.
   Regression test: queue events never create traces and never become `active_run_trace`.
3. Fix provider lifetime: abort-on-drop in `stream_turn`, token cancellation on the `run_turn` error
   path. Test that a dropped/failed consumer stops further provider events and tool execution.
4. Harden Pub/Sub decoding and listener retry for **all four** subscriptions (interrupt, KB, model
   and MCP). Unit-test the frame decoder; malformed frames are skipped, disconnections trigger a
   backoff resubscribe, and cache listeners invalidate once after reconnecting to cover missed
   broadcasts.
5. Targeted frontend guards: main-scope-only `context_usage`, run-id-guarded clear on terminal
   events. Fix the phantom-trace bug first, because the run-id guard relies on a correct hydrated
   `run_id`.

Exit: existing gates green, new regression tests green, no wire or schema change.

**Phase 1 — consolidate the pipeline.** `EventScope`, scoped coalescer, `project`, `EventSink`,
`SessionEventLog` + fake, delete `handle_subagent_event`, coalesce subagent deltas, bounded provider
channel. Land as "flatten helper" then "scoped stream item".

Exit: `execute_turn` contains no per-variant envelope code and exactly one flush call site; ordering
integration tests pass unchanged; subagent-heavy runs produce measurably fewer `session_events`
writes (expected order-of-magnitude reduction); no wire change.

**Phase 2 — atomicity, database invariants and canonical projection.** Move classification and the
pure aggregator into `quire-session-protocol`; `append_event_in` / `append_events_batch`; atomic
terminal/tool/question transitions; fault-injection tests that fail the Nth append and assert no
partial state and a cancelled provider. Terminal transitions have a single winner: the status
transition and its terminal event commit in one transaction, and a loser cannot append a second
terminal frame. `interrupt()` signals cancellation rather than racing the run loop to write an
unrelated terminal sequence. `answer_question` and `retry` take the session gate; queue message
insertion, queue deletion and event append share one transaction; session deletion removes its
in-memory gate.

The deployment decision is **database invariants with an in-process fast path**, not an assumption
of permanent single-instance deployment. Add a partial unique index allowing at most one top-level
`running` / `awaiting_input` run per session, and use `agent_sessions ... FOR UPDATE` for active-run
transitions and queue claiming. Keep the in-memory `session_gate` for cheap local serialisation. This
also protects rolling deployments, where old and new processes briefly overlap.

Exit: run status and terminal event can no longer diverge or double-terminate; canonical aggregator
is shared by store and fixtures; concurrent turn-start and queue-drain tests cannot create two
active runs; `just schema` shows no contract change.

**Phase 3 — transport and read path.** Opaque JSON SSE forwarding, unknown-event tolerance in
aggregation, cursor-ahead `sync_required`, heartbeat reconcile, active-run-only hydration with the new
indexes. Optional follow-ups: materialised completed-run traces, then retention with
minimum-available-`seq`.

Exit: detail cost depends on the active run rather than session history; a lost `turn_finished`
publish is repaired without reconnect; an unknown event variant neither closes SSE nor fails detail.

**Phase 4 — frontend state machine.** Pure reducer, effects, exhaustive switch, shared fixtures,
hydration-equivalence property test, browser regressions. Can run in parallel with Phases 2–3 once
fixtures exist.

**Phase 5 — observability, lifecycle and docs.** Structured spans/fields (`session_id`, `run_id`,
`agent_id`, `seq`, `event_type`), counters for publish failures / gaps / sync_required / invalid
frames / aborted providers, retained background-task handles with graceful shutdown. Consolidate the
five subscriptions on one resilient subscriber abstraction that distinguishes malformed frames from
disconnection. Add an RAII run-turn guard so every early return cancels and unregisters the run, and
architecture guardrail tests: exhaustive sample-variant classification/mapping and module visibility
that forces event writes through `EventSink`. Move ephemeral preview POSTs to a latest-frame-wins
background sender so slow preview transport cannot block model consumption. Write
`docs/RUNTIME_SESSION_EVENTS.md` (module layout, event classes, invariants, replay/retention,
schema-evolution rules), update `docs/RUNTIME_RIG_STREAMING.md` module layout and link this note.

Every phase ends with:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
just schema            # only when a protocol type changes
pnpm --dir web gen:types
```

### 3.9 Target invariants

1. `seq` is strictly increasing per session and allocated together with the event row.
2. A structural event is never persisted before the deltas that preceded it.
3. Run/session status and the terminal event commit atomically.
4. No provider task outlives its consumer; cancellation stops tool side effects at the next boundary.
5. Control-plane events never create or mutate a run trace.
6. The active trace is selected by active run id, never by event order.
7. Event attribution is decided once at the producer and never recomputed downstream.
8. Coalescing is per scope; subagent scopes use the same flush rules as main.
9. Unknown event types are forwarded by transport and skipped by aggregation.
10. Live and hydrated projections agree on the same event prefix (fixture-verified).
11. No unbounded channel and no unbounded history read on the detail path.
12. Full tool payloads never appear on the wire; only previews do.

### 3.10 Costs, limitations and follow-up decisions

- More modules (`events/`, protocol `trace.rs`) and one new trait; `service.rs` and `store.rs` change
  substantially even though the wire contract does not.
- Scoped coalescing trades immediate subagent frames for 50 ms batching and per-scope flush order.
- Opaque forwarding stops validating event shape at the transport layer; validation stays at write
  time, and malformed rows would be relayed as-is.
- Materialised traces and retention add a completion-time write and a backfill job; they are deferred
  until the active-run read path proves insufficient.
- **Deployment decision (settled):** use database invariants for one active top-level run per
  session and transactional queue claiming, while retaining the in-memory gate as a fast path. This
  covers multi-instance deployments, rolling-deploy overlap and single-process paths that forget to
  acquire the in-memory gate. It intentionally avoids a separate distributed-lock manager.
- The subagent segment unification changes the public trace schema and is deliberately scheduled as
  an explicit contract migration, not hidden inside the no-wire-change phases.
- `EventScope` should become a strongly typed constructor boundary (main / subagent / session
  control), rather than another collection of positional `&str` run IDs, so future code cannot swap
  `run_id`, `parent_run_id` and `subagent_run_id` or handcraft a synthetic run envelope.

## 4. Implementation Status

- **Phase 0 landed:** run-scoped trace classification, active-run selection by run ID, provider
  abort-on-drop and failure cancellation, resilient subscriptions, frontend scope/terminal guards,
  and four golden projection fixtures.
- **Phase 1 core landed:** `events::{scope, project, sink, log}` now owns the only model-event
  mapping and structural flush path. Main and subagent deltas are coalesced by complete scope,
  `handle_subagent_event` and the duplicated tool-event mappers are deleted, and the provider queue
  is bounded at 256 frames with a backpressure test.
- `ModelStreamEvent::SubagentEvent` remains at the provider boundary and is flattened once by
  `project`. Moving scope into the `ModelEventStream` item type is optional cleanup, not a requirement
  for the single-mapping invariant; deferring it avoids changing the provider trait and every test
  provider in the same step.
- **Phase 2 landed:** `append_event_in` and batched appends; atomic tool/question-plus-event writes;
  conditional `finish_run_with_event` for every terminal path; interrupt flush-then-finalize with a
  bounded fallback; the `0008_active_run_uniqueness.sql` partial unique index and transactional
  `create_run`; session gates on `answer_question` / `retry` and gate cleanup on delete; atomic,
  revalidated queue claiming with a single winner; and the canonical aggregator moved to
  `quire-session-protocol/src/trace.rs` together with the golden fixture test.
- **Phase 3 core landed:** opaque `(seq, event_type, event_json)` SSE replay and gap repair,
  unknown-variant-tolerant projection, cursor-ahead `sync_required`, configurable heartbeat
  reconciliation against durable `last_event_seq`, and `agent_runs.start_seq` recorded atomically with
  `turn_started` for primary-key active-run range scans. Idle detail requests no longer read event
  history. Completed-run trace materialisation and event retention remain deferred.
- **Phase 4 landed:** `RunTraceSubagent.segments` is now isomorphic to the main trace, with regenerated
  wire schemas and updated aggregate/historical/live views. The frontend event chain is replaced by
  the exhaustive pure `sessionEventReducer` plus effect execution. Shared fixtures now include
  `prefix_traces`, and `sessionEventReducer.test.ts` verifies every split point, including a dedicated
  multi-round subagent fixture. The equivalence tests also caught and fixed live `truncated` updates
  and `taskFocus` object-shape drift.
- **Phase 4 support work landed:** RAII `RunTurnGuard`, exhaustive event/projection guardrails,
  `agent_runs.trace_summary` materialization at terminal transitions, lazy backfill, and
  `min_available_seq` retention with `POST /api/sessions/{session_id}/prune-events`. `/traces` now
  reads stored traces and only the active run is aggregated live.
- **Frontend refetch reduction landed:** `SessionTitleUpdated`, `DocLinked`, `PendingQueueUpdated`
  and `TurnStarted` now patch the query cache directly instead of invalidating the full session
  detail. Refetches remain only for structurally incomplete frames and terminal/synchronization
  boundaries. Reducer effect tests pin this contract.
- **All planned phases and support work landed:** preview latest-frame-wins handoff with a final-frame
  barrier, subagent `resume_run_id` with bounded in-memory history, the higher-education 9-step
  writing workflow, and the canonical `docs/RUNTIME_SESSION_EVENTS.md` contract.
- **Review fixes landed:** preview flush uses a watch watermark instead of `Notify`, avoiding lost
  wakeups; resumed subagent history includes the final assistant reply and frontend/backend resume
  state is deduplicated; pruning computes the active-run cursor and deletes under one session row
  lock, including the missing-`start_seq` edge case.
