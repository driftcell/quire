# RUNTIME RIG STREAMING

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-runtime`
- **Related Notes:** `../notes/runtime_rig_streaming_engine.md`, `../notes/pending_user_message_trace_timeline.md`

## 1. Scope & Non-Goals

This document governs the real rig-core model streaming engine in `quire-runtime`: turn context,
model/tool resolution, multi-turn tool execution, subagent fan-out, event mapping, usage reporting,
ephemeral preview streaming and cancellation. It does not govern Quire report MCP tools themselves,
report block Smart CAS, or provider credential storage, which are owned by `quire-server`,
`quire-store` and `quire-protocol`.

### Module layout

`crates/quire-runtime/src/rig_agent/` is split by responsibility without changing the public
`rig_agent` exports:

- `mod.rs`: `RigModelStreamProvider`, `RigModelCore`, `AgentExecution`, `EventScope`, the main turn
  loop, stream-event mapping and pending-queue injection.
- `preview_stream.rs`: streamed `apply_operations` argument parsing, preview throttling/transport,
  anchor inference and preview-ID injection.
- `subagents.rs`: `SubagentTask`, `SubagentOutcome`, `spawn_subagents` and the subagent lifecycle.
- `builtin_tools.rs`: built-in tool schemas/implementations, allow-list matching and credential/URL
  redaction helpers.

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
`TextDelta`/`ThinkingDelta`; the service coalesces only main-agent deltas. When the provider turn
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
or `AwaitingInput` and the latest aggregated run is still active, the detail response carries
`active_run_trace` plus its flattened `active_subagent_traces`. The runtime builds both from
`session_events` using the same `seq <= last_event_seq` boundary returned in the summary, so the
snapshot and the subsequent `afterSeq = last_event_seq` SSE connection partition the event stream
without gaps or duplicates.

`RunTraceSummary.started_at` is the originating `TurnStarted` timestamp. `RunTraceSegment::Thinking`
retains `started_at` while a thinking phase is still streaming, and `RunTraceSubagent` carries its own
`started_at`, `thinking_started_at` and streamed `text`. The frontend converts the snapshot to
`LiveAgentTrace` / `LiveSubagentRun` before subscribing to SSE, which keeps the top run timer and the
active thinking timer on their original server clocks instead of restarting from zero after refresh.

Built-in tools (`load_skill`, `read_skill_resource`, `describe_task`, `spawn_subagents`) read skills
from the session branch. `describe_task` lets an agent publish `TaskFocus { title, detail,
section_id }` to the UI while working; the schema documents the short Chinese title, optional detail
and section `block_id`. `spawn_subagents` reads each child
`agent_definitions` row, resolves its model route/allowed tools, and runs real nested model loops
concurrently with `tokio::task::JoinSet`. Parent lifecycle is emitted as `SubagentStarted` /
`SubagentFinished`; child deltas, task focus, tool calls and usage are wrapped in `SubagentEvent` so
the service preserves the child `agent_id` and parent run envelope.

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

The cancellation token is checked by the stream wrapper, every model turn and every tool execution.
Dropping or cancelling the stream stops subagent `JoinSet` work as well. Each `stream.next()` read has
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
Anchors follow the real `PatchOp` wire fields and accept both the `op` compatibility key and the
serialized `type` discriminator: `insert_after`/`insert_before` read `target_id`, `append_to_section`
reads `section_id`, `patch_block` reads `block_id` and parses the nested `patch`, and `replace_range`
reads `start_id`. Multi-block insert arrays chain subsequent previews after the previous emitted
block ID, so previews stay in the target section instead of falling back to the document tail.
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
- Main text/thinking deltas are coalesced; subagent deltas retain the child `agent_id` and parent
  run.
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
- Streamed preview anchors follow the real `PatchOp` `type` discriminator and `op` compatibility key,
  and chain multi-block inserts. A preview with an unresolved target anchor is hidden, never pinned to
  the document tail; the complete-argument pass republishes final anchors before commit.
- Runtime HTTP clients configure `connect_timeout(15s)` and a request timeout; no unbounded default
  reqwest client is used for model catalogs, preview publishing, comment proxying or MCP transport.
- An active-run snapshot and its SSE cursor share one `last_event_seq` boundary; refreshing during a
  run hydrates prior thinking, tools and subagents, then continues from the next event.
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

Browser coverage in `web/e2e/pending-user-message-timeline.js` verifies the same DOM order before
and after `page.reload()`, exactly one inline user bubble, and two settled activity groups.
`web/e2e/thinking-duration-accuracy.js` reloads a running session with prior thinking and tools and
asserts the hydrated timers continue from the server clocks before the terminal SSE events arrive.
Frontend unit coverage in `web/src/components/chat/traceSegments.test.ts` applies the same boundary
to live and historical segment builders. `web/src/components/report/ephemeralPreviewMix.test.ts`
asserts that unresolved `after_block`/`append_to_section` previews stay hidden instead of mounting at
the document tail, while real `append_to_doc` previews still render at the tail.

Unit coverage in `crates/quire-runtime/src/rig_agent/{mod.rs,preview_stream.rs,builtin_tools.rs,subagents.rs}`:

- `describe_task` schema advertises the status title, detail and section parameters
- partial `apply_operations` argument block extraction
- real `PatchOp` `type` discriminator anchors, chained multi-block inserts and `patch_block` type isolation
- preview ID merge
- real usage mapping with a 64k context limit

Full gate:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
