# RUNTIME RIG STREAMING

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-runtime`
- **Related Notes:** `../notes/runtime_rig_streaming_engine.md`, `../notes/pending_user_message_trace_timeline.md`, `../notes/session_event_pipeline_consolidation.md`

## 1. Scope & Non-Goals

This document governs the real rig-core model streaming engine in `quire-runtime`: turn context,
model/tool resolution, multi-turn tool execution, subagent fan-out, event mapping, usage reporting,
ephemeral preview streaming and cancellation. It does not govern Quire report MCP tools themselves,
report block Smart CAS, or provider credential storage, which are owned by `quire-server`,
`quire-store` and `quire-protocol`.

### Module layout

`crates/quire-runtime/src/rig_agent/` is split by responsibility without changing the public
`rig_agent` exports:

- `mod.rs`: `RigModelStreamProvider`, `RigModelCore`, `AgentExecution`, the main turn loop, rig
  stream-event adaptation and pending-queue injection.
- `preview_stream.rs`: streamed `apply_operations` argument parsing, preview throttling/transport,
  anchor inference and preview-ID injection.
- `subagents.rs`: `SubagentTask`, `SubagentOutcome`, `spawn_subagents` and the subagent lifecycle.
- `builtin_tools.rs`: built-in tool schemas/implementations, allow-list matching and credential/URL
  redaction helpers.

`crates/quire-runtime/src/events/` is the single session-event pipeline below the provider boundary:

- `scope.rs`: `EventScope { run_id, agent_id, parent_run_id }`. The full scope is also the delta
  coalescer key, so subagent output cannot lose its root parent and become a top-level run.
- `project.rs`: the only `ModelStreamEvent -> EventAction` mapping. It flattens `SubagentEvent`
  wrappers, preserves lifecycle envelopes, and makes main-only events explicit `Ignore` actions.
- `sink.rs`: `EventSink` coalesces main and subagent deltas independently, flushes every scope before
  structural events, persists tool/question side effects before their wire events, and returns
  main-agent usage and created questions to the run loop.
- `log.rs`: the narrow `SessionEventLog` persistence boundary. `RuntimeStore` implements it in
  production with batched appends and atomic tool/question-plus-event writes; unit tests use a
  recording fake for ordering and failure injection.

`service.rs::execute_turn` only orchestrates the provider stream, flush ticks, cancellation and run
lifecycle. It no longer contains per-variant envelope mapping or a separate subagent event mapper.

## 2. Canonical Design & Contract

### Turn request

`ModelTurnRequest` carries:

- `session_id`, `run_id`, `agent_id`, `system_prompt`, `user_message`
- `history: Vec<ContextMessage>` from `build_resume_context`, including the transient
  `<knowledge_update>` system reminder and the current user message
- `doc_id`: the bound Quire report, injected as `<runtime_session>` preamble guidance so the model
  uses the existing report instead of calling `create_report`
- `kb_branch`: session branch used for `agent_definitions` and `skill_definitions` lookups
- `model_route` and `allowed_tools`: the primary agent definition resolved by `RuntimeService`
- `cancellation: CancellationToken`

`RigModelStreamProvider` resolves the primary agent definition from `KbService` on the session
branch and falls back to the request route/allow-list. Unknown routes fail with a clear
`model route ... is not configured` error; missing agent definitions name the branch and tell the
operator to call `kb_upsert_agent`.

### Streaming and tool loop

The provider calls `ModelRegistry::build_rig_model`, builds allow-listed `DynamicTool`s, and uses
`DynModel<Completion>::stream`. The allow-list applies the same wildcard semantics to both built-in
and MCP tools: `"*"` enables every tool, while any other entry matches one tool name. `StreamEvent::Text` and `StreamEvent::Reasoning` map to
`TextDelta`/`ThinkingDelta`; `EventSink` coalesces deltas independently per complete `EventScope`,
including subagents. When the provider turn
finishes, the canonical `CompletionResponse.choice` supplies the assistant message and tool calls.
Each call is answered with a `Message::tool_results` message, and the next model turn runs until the
model returns no tool calls. Between the tool results (and any preview images) and the next model
request, the main-agent loop atomically takes one pending user-message batch under the session gate
and appends it as a `Message::user`. This lets queued corrections reach the very next model round
without waiting for the run to end. The message is inserted with the current `run_id`; its
`user_message_injected { message_id, content, created_at }` frame is then sent through the same
ordered model-event stream as tool events, so persistence cannot invert it ahead of the tool result.
Normal turn-start queue draining instead leaves `run_id` empty and emits `user_message_added`.
Subagents and `ask_user` suspensions never consume the queue.

At the end of a run, `finish_run` restores the session to `Idle` and the service immediately calls
`queue_notify.notify_one()`, so the pending-queue loop starts the next turn without waiting for the
2-second fallback tick. The same coordinator owns dequeue, message persistence and
`pending_queue_updated` events, preventing concurrent consumers from taking one item twice.

`RunTraceSegment::UserMessage` is the inline counterpart of `user_message_injected`. Persisted session
events are aggregated by SSE sequence, so refreshed traces retain the exact order: previous
thinking/execution, inline user message, then new thinking/execution/text. The live SSE handler appends
the same segment to `liveMain` and closes the preceding activity. Frontend rendering starts a fresh
activity after the user segment, reuses the standard right-aligned user bubble with full message
metadata when available, and suppresses only those message IDs from the outer transcript.

### Active-run hydration

`SessionDetail` is the refresh-safe counterpart of the live SSE pane. When the session is `Running`
or `AwaitingInput`, the runtime resolves the active run from `agent_runs` and selects its trace by
`run_id`; it never falls back to the newest aggregated trace. Only run-scoped events participate in
aggregation (`SessionEvent::trace_run_id`), so queue, compaction and discovery frames cannot create
phantom runs. The canonical pure aggregator lives in `quire-session-protocol/src/trace.rs`; the
runtime store reads `session_events` and delegates to it using the same `seq <= last_event_seq`
boundary returned in the summary. The detail response carries `active_run_trace` plus the active
subagent traces extracted from execution segments, so the snapshot and the subsequent
`afterSeq = last_event_seq` SSE connection partition the event stream without gaps or duplicates.

### Session-SSE transport and active-run reads

`/api/sessions/{session_id}/events` forwards opaque `(seq, event_type, event_json)` frames during
replay and gap repair instead of deserialising each variant. A runtime can therefore relay event
variants introduced by a newer writer during a rolling deploy. Trace projection takes the opposite
position: an unknown variant is skipped with a warning rather than failing the whole detail request.
An `afterSeq` cursor ahead of `last_event_seq` returns `sync_required` immediately.

The heartbeat (`RUNTIME_SSE_HEARTBEAT_MS`, default 15000 ms) also reconciles with the durable
`agent_sessions.last_event_seq`. If Pub/Sub lost a frame, including the final `turn_finished`, the
next heartbeat replays it through the same gap path instead of leaving the client `running` until
reconnect.

`agent_runs.start_seq` is written atomically with `turn_started` (migration `0009_run_start_seq.sql`)
and backfilled from existing events. Active-run hydration reads `seq BETWEEN start_seq AND
last_event_seq` through the `(session_id, seq)` primary key, so `get_session_detail` no longer
aggregates the whole session history for the active run; idle sessions skip event history entirely.

Completed runs are materialized into `agent_runs.trace_summary` by
`finish_run_with_event` and `resolve_suspended_run` (migration `0010_trace_materialization.sql`),
using the canonical aggregator over `start_seq..end_seq`. `/traces` reads those stored traces and
aggregates only the active run, with lazy backfill for legacy completed runs and a fallback for
run-scoped events that have no `agent_runs` row. `POST /api/sessions/{session_id}/prune-events`
materializes completed traces before deleting old raw events, preserves the active run's full
history, and advances `agent_sessions.min_available_seq`. An `afterSeq` cursor whose next expected
event is older than that retained window returns `sync_required`.

`RunTraceSummary.started_at` is the originating `TurnStarted` timestamp. `RunTraceSegment::Thinking`
retains `started_at` while a thinking phase is still streaming, and `RunTraceSubagent` carries its own
`started_at` plus ordered `RunTraceSegment` values isomorphic to the main run. Multi-round subagent
thinking, tools and text therefore keep the same order in snapshots and in the live reducer instead
of being flattened into fixed thinking/execution/text slots. The frontend converts the snapshot to
`LiveAgentTrace` / `LiveSubagentRun` before subscribing to SSE, which keeps the top run timer and the
active thinking timer on their original server clocks instead of restarting from zero after refresh.

Built-in tools (`load_skill`, `read_skill_resource`, `describe_task`, `spawn_subagents`) read skills
from the session branch. `describe_task` lets an agent publish `TaskFocus { title, detail,
section_id }` to the UI while working; the schema documents the short Chinese title, optional detail
and section `block_id`. `spawn_subagents` reads each child
`agent_definitions` row, resolves its model route/allowed tools, and runs real nested model loops
concurrently with `tokio::task::JoinSet`. Parent lifecycle is emitted as `SubagentStarted` /
`SubagentFinished`; child deltas, task focus, tool calls and usage are wrapped in `SubagentEvent`.
`events::project` flattens that wrapper into a complete `EventScope`, preserving the child `agent_id`
and root parent run envelope.

Quire `/mcp` tools are discovered with `McpHttpClient` using the real `X-Session-Id` and `X-Actor`,
then filtered by the global MCP registry and the agent allow-list. The triple intersection is
enabled server ∩ enabled tool ∩ allow-list match (`*`, `<server_id>:*`, plain name or
`<server_id>:<tool>`). Tool execution captures full JSON input/output and emits
`ToolCallStarted` / `ToolCallFinished`; the service writes `agent_tool_calls` rows at start and
updates them at finish. Provider call IDs are prefixed with the owning run ID because the database
key is global.

### Usage and cancellation

The provider emits one `Usage(ContextUsage)` after a successful run. `input_tokens`, `output_tokens`
and `cached_tokens` are the upstream `rig_core::completion::Usage` counters; `context_used` uses the
provider total when present; `context_limit` comes from the resolved `llm_models` row. The service
stores this in `agent_sessions.context_usage` and emits `ContextUsage`. Only offline providers that
report no usage use the legacy text-length estimate.

Provider events travel through a bounded 256-frame channel. Slow event persistence therefore applies
backpressure to model and tool production instead of accumulating an unbounded queue of deltas and
full tool payloads. The cancellation token is checked by the stream wrapper, every model turn and
every tool execution. The provider stream owns an abort-on-drop guard for its producer task, so
dropping the stream aborts
model and tool work instead of leaving a detached turn behind. A failed turn cancels the run token
before recording its terminal failure, preventing persistence or mapping errors from leaving tools
running after the run is marked as failed. Run status and terminal event commit atomically through
`finish_run_with_event`: normal completion, failure and user interruption have exactly one terminal
winner, and a late finisher is a no-op. `interrupt()` signals cancellation and lets the owning turn
flush residual deltas before finalizing; its bounded fallback uses the same conditional transition,
so a run cannot emit both `TurnFinished` and `TurnFailed`. Dropping or cancelling the stream stops
subagent `JoinSet` work as well. Each `stream.next()` read has
a 120-second idle timeout so a half-open provider connection cannot hang a turn forever. Subagents are
not bounded by a total wall-clock timeout: a long-running subagent that keeps making progress is
allowed to finish, and termination comes from normal completion, parent cancellation, or a network /
stream idle timeout. Runtime HTTP clients use a 15-second connect timeout plus bounded request
timeouts; rmcp MCP transports use a dedicated reqwest 0.13 client with the same connect bound and a
300-second request bound.

### Ephemeral previews

While streamed `apply_operations` arguments arrive, `PreviewState` parses the partial
JSON with rig's partial-object parser, extracts block-like objects (heading, paragraph, quote, table,
echarts, custom_html), and pushes `PartialBlockPreview` frames through `ToolPreviewStreamer`.
Anchors follow the serialized `PatchOp` wire format: the `type` discriminator selects
`insert_after`/`insert_before`/`append_to_section`/`patch_block`/`replace_range`;
`insert_after`/`insert_before` read `target_id`, `append_to_section` reads `section_id`,
`patch_block` reads `block_id` and parses the nested `patch`, and `replace_range` reads `start_id`.
Multi-block insert arrays chain subsequent previews after the previous emitted block ID, so previews
stay in the target section instead of falling back to the document tail.
Models may emit `blocks` before `target_id`/`section_id`/`type`; a preview whose anchor target is not
committed or not yet another preview is therefore hidden rather than pinned to the document tail.
When the tool-call stream ends, `PreviewState` re-extracts the complete arguments, force-publishes the
final frames with resolved anchors even inside the throttle window, and waits 200 ms for the frontend
to repaint before the tool executes and the commit clears the previews.
Frames are throttled to 50-80 ms per block and sent to `POST /api/docs/{doc_id}/ephemeral`. The
preview IDs are attached to the tool arguments as `preview_ids` immediately before execution, so the
formal Quire commit clears the placeholders atomically.

## 3. Invariants & Verification

- Production `main.rs` installs `RigModelStreamProvider`; stub/mock providers are only for offline
  tests.
- A session on a draft branch resolves agents and skills from that branch, not active.
- Main and subagent deltas are coalesced independently per complete `EventScope`; subagent deltas
  retain the child `agent_id` and root `parent_run_id`.
- `EventSink` is the only structural-flush owner. Tool and question side effects are persisted before
  their wire events, and a failed side effect does not emit a partial wire event.
- The provider queue is bounded at 256 frames; a full queue blocks the producer until the consumer
  drains a frame instead of growing without limit.
- Run status and terminal event are one transaction; a late finisher is a no-op and cannot emit a
  second terminal frame.
- `create_run` holds the session row lock and migration `0008_active_run_uniqueness.sql` adds a
  partial unique index, so at most one top-level `running` / `awaiting_input` run exists per session
  even across processes and rolling deploys. `answer_question` and `retry` take the same in-process
  session gate, and deleting a session removes its gate.
- Queue claiming is one transaction: the message, queue deletion and events commit together, and the
  queue IDs are revalidated under the session row lock so concurrent claims have exactly one winner.
- SSE replay and gap repair forward opaque frames; unknown variants are relayed by transport and
  skipped by projection. A cursor ahead of the server produces `sync_required`.
- The heartbeat reconciles against durable `last_event_seq`, so a lost terminal publish is repaired
  without requiring a reconnect.
- `turn_started` and `agent_runs.start_seq` commit atomically; active-run hydration is a primary-key
  range scan and idle detail requests do not read event history.
- Completed-run traces are materialized in the same lifecycle transaction. `/traces` reads
  `agent_runs.trace_summary` and only the active run is aggregated live. Pruning preserves those
  traces and enforces `min_available_seq` on replay cursors.
- Tool calls persist complete input and output, with globally unique IDs.
- Pending queue dequeue is serialized by the per-session gate; queue items are persisted as user
  messages exactly once.
- A queued user message is injected after tool results and before the next main-agent LLM request.
- Inline messages carry the active run ID and one `user_message_injected` event; normal turn-start
  messages use `user_message_added` and do not masquerade as inline activity.
- Live and refreshed traces share the `UserMessage` boundary: the previous activity is completed and
  later work starts a new activity. The outer transcript never duplicates an inline message ID.
- A model stream step that produces no event for 120 seconds fails the turn instead of hanging.
  Subagents have no total wall-clock cutoff; network/stream timeouts and parent cancellation bound them.
- Streamed preview anchors follow the serialized `PatchOp` `type` discriminator and chain multi-block
  inserts. A preview with an unresolved target anchor is hidden, never pinned to the document tail;
  the complete-argument pass republishes final anchors before commit.
- Runtime HTTP clients configure `connect_timeout(15s)` and a request timeout; no unbounded default
  reqwest client is used for model catalogs, preview publishing, comment proxying or MCP transport.
- An active-run snapshot and its SSE cursor share one `last_event_seq` boundary; refreshing during a
  run hydrates prior thinking, tools and subagents, then continues from the next event.
- Session control/signal frames never create a run trace, and the active trace is selected by the
  active run ID rather than by aggregation recency.
- Dropping a provider stream aborts its producer task, and every failed turn cancels its run token.
- Malformed Redis Pub/Sub frames are skipped rather than treated as end-of-stream. Interrupt and cache
  subscriptions reconnect with backoff; cache listeners also invalidate once after reconnecting to
  cover broadcasts missed during the gap.
- The frontend applies only main-scope `context_usage` frames to the session meter, and a delayed
  `turn_finished` refetch clears live state only if no newer run has taken ownership of the pane.
- `web/src/lib/sessionEventReducer.ts` is the only frontend event projection: an exhaustive pure
  reducer returns state plus effects, while `useSessionEventStream` only executes those effects.
- `RunTraceSubagent.segments` is isomorphic to the main trace, so multi-round subagents keep the same
  order after hydration as they had live.
- Every shared fixture also carries `prefix_traces`. `web/src/lib/sessionEventReducer.test.ts` checks
  every split point: `hydrate(aggregate(prefix)) + reduce(suffix) == reduce(all)`.
- `ask_user` suspension leaves the pending queue untouched.
- Idle completion notifies the queue loop immediately; the 2-second interval is only a fallback.
- `context_usage.context_limit` is the resolved provider model limit, not a fixed constant.
- Task focus, subagent lifecycle, tool lifecycle, usage and turn completion retain SSE order.

Integration coverage in `crates/quire-runtime/tests/runtime_integration.rs`:

- `rig_turn_context_and_task_events_reach_the_persisted_session_contract`
- `tool_boundary_injects_pending_message_into_same_run` also asserts persisted trace order
  `[Execution(tool), UserMessage(inline), Text(next round)]`.
- `idle_completion_wakes_pending_queue_without_interval_wait`
- `ask_user_suspension_keeps_pending_queue_untouched`
- `session_detail_returns_an_atomic_active_run_snapshot` verifies the active trace, `started_at`,
  completed/running tools, streaming thinking and active subagent clocks against `last_event_seq`.
- `queued_messages_do_not_create_phantom_traces_or_hijack_the_active_run` verifies that queue control
  frames neither create a `queue` trace nor replace the active run during hydration.
- `failed_turn_cancels_the_provider_token` verifies that a failed run is terminally marked and its
  provider cancellation token is triggered.
- `active_run_creation_is_unique_per_session` verifies the active-run guard and that a completed run
  releases the slot.
- `terminal_transition_has_a_single_winner` verifies that a late interrupt cannot add `TurnFailed`
  after `TurnFinished`.
- `interrupt_flushes_residual_deltas_before_a_single_terminal_event` verifies cancellation ordering
  and that a repeated interrupt appends no second terminal event.
- `concurrent_queue_claims_have_one_winner` verifies atomic, revalidated queue claiming.
- `unknown_events_are_forwarded_and_skipped_by_projection` verifies forward-compatible transport and
  tolerant projection.
- `cursor_ahead_of_the_server_returns_sync_required` verifies cursor validation.
- `heartbeat_reconcile_repairs_a_missing_publish` commits an event without publishing it and verifies
  that heartbeat reconciliation delivers it.
- `active_run_start_seq_scopes_hydration_to_the_current_run` verifies the atomic `start_seq` cursor
  and that older runs are excluded from active hydration.
- `completed_run_trace_is_materialized_on_terminal_transition` and
  `resolving_a_suspended_run_materializes_its_trace` verify trace materialization.
- `pruning_preserves_materialized_history_and_enforces_the_retained_cursor` verifies that pruning
  keeps completed traces and rejects cursors older than the retained window.

Browser coverage in `web/e2e/pending-user-message-timeline.js` verifies the same DOM order before
and after `page.reload()`, exactly one inline user bubble, and two settled activity groups.
`web/e2e/thinking-duration-accuracy.js` reloads a running session with prior thinking and tools and
asserts the hydrated timers continue from the server clocks before the terminal SSE events arrive.
Frontend unit coverage in `web/src/components/chat/traceSegments.test.ts` applies the same boundary
to live and historical segment builders. `web/src/lib/sessionEvents.test.ts` covers subagent scope
classification, main-scope usage filtering and run-owner guards for delayed terminal cleanup.
`web/src/components/report/ephemeralPreviewMix.test.ts`
asserts that unresolved `after_block`/`append_to_section` previews stay hidden instead of mounting at
the document tail, while real `append_to_doc` previews still render at the tail.

Unit coverage in `crates/quire-runtime/src/rig_agent/{mod.rs,preview_stream.rs,builtin_tools.rs,subagents.rs}`:

- `describe_task` schema advertises the status title, detail and section parameters
- partial `apply_operations` argument block extraction
- real `PatchOp` `type` discriminator anchors, chained multi-block inserts and `patch_block` type isolation
- preview ID merge
- real usage mapping with a 64k context limit
- dropping a provider stream aborts its producer task, while normal completion and cancellation end
  the stream
- a full bounded producer queue applies backpressure and draining one frame releases the producer
- `events::project` scope flattening, lifecycle envelopes, main-only filtering and assistant-text
  attribution
- `events::sink` structural flush ordering, scoped subagent coalescing, tool persistence ordering,
  side-effect failure handling and main/subagent usage outcomes
- malformed Pub/Sub frame decoding
- run-scoped trace classification and active-trace selection by run ID

Golden projection fixtures in `contracts/fixtures/session-events/*.json` pin main, subagent, inline
user-message and session-control event sequences to their exact `RunTraceSummary` output. The
`quire-session-protocol::trace::tests::golden_trace_fixtures_pin_the_projection_contract` test reruns
them on every change. `subagent_multi_round.json` additionally pins two consecutive
thinking/tool rounds and proves that the subagent segment model does not flatten their order. Each
fixture also includes `prefix_traces`, which `web/src/lib/sessionEventReducer.test.ts` uses for the
prefix/suffix hydration equivalence property.

Full gate:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
