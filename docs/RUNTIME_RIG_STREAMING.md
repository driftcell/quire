# RUNTIME RIG STREAMING

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-runtime`
- **Related Notes:** `../notes/runtime_rig_streaming_engine.md`

## 1. Scope & Non-Goals

This document governs the real rig-core model streaming engine in `quire-runtime`: turn context,
model/tool resolution, multi-turn tool execution, subagent fan-out, event mapping, usage reporting,
ephemeral preview streaming and cancellation. It does not govern Quire report MCP tools themselves,
report block Smart CAS, or provider credential storage, which are owned by `quire-server`,
`quire-store` and `quire-protocol`.

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
`DynModel<Completion>::stream`. `StreamEvent::Text` and `StreamEvent::Reasoning` map to
`TextDelta`/`ThinkingDelta`; the service coalesces only main-agent deltas. When the provider turn
finishes, the canonical `CompletionResponse.choice` supplies the assistant message and tool calls.
Each call is answered with a `Message::tool_results` message, and the next model turn runs until the
model returns no tool calls. There is intentionally no fixed model-turn or subagent-depth cap;
cancellation is the stop mechanism.

Built-in tools (`load_skill`, `read_skill_resource`, `describe_task`, `spawn_subagents`) read skills
from the session branch. `describe_task` emits `TaskFocus`. `spawn_subagents` reads each child
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
Dropping or cancelling the stream stops subagent `JoinSet` work as well.

### Ephemeral previews

While streamed `apply_operations` / `apply_draft` arguments arrive, `PreviewState` parses the partial
JSON with rig's partial-object parser, extracts block-like objects (heading, paragraph, quote, table,
echarts, custom_html), and pushes `PartialBlockPreview` frames through `ToolPreviewStreamer`.
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
- `context_usage.context_limit` is the resolved provider model limit, not a fixed constant.
- Task focus, subagent lifecycle, tool lifecycle, usage and turn completion retain SSE order.

Integration coverage in `crates/quire-runtime/tests/runtime_integration.rs`:

- `rig_turn_context_and_task_events_reach_the_persisted_session_contract`

Unit coverage in `crates/quire-runtime/src/rig_agent.rs`:

- partial `apply_operations` argument block extraction
- preview ID merge
- real usage mapping with a 64k context limit

Full gate:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
