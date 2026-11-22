# RIG MODEL STREAMING LOOP

## Metadata

- **Status:** Superseded
- **Date:** 2026-11-18
- **Superseded By:** `runtime_rig_046_native_migration.md`
- **Promoted To:** `../docs/RUNTIME_RIG_STREAMING.md`

## 1. Context & Problem

The runtime had a real `ModelRegistry` and `DynModel` builder but still executed sessions through
`StubStreamProvider`. The turn loop needed to become a real rig-core streaming agent: context-aware
prompts, tool allow-lists, multi-turn tool execution, live task/subagent events, real token usage,
preview streaming, and cancellation. The event contract also had to preserve the existing session SSE
ordering and full tool-call persistence.

## 2. Rejected Options & Why

- **Add `rig-agent` and use `Agent::prompt(...).stream()`.** Rejected for this step because the
  host still needs custom preview fragment extraction, full tool input/output persistence and
  branch-scoped built-in tools. Driving `DynModel<Completion>` directly keeps every stream event,
  tool result and cancellation point under runtime control without adding a second agent runtime
  dependency.
- **Keep `StubStreamProvider` as the default and require explicit wiring.** Rejected because it
  leaves production sessions answering with placeholder text. `main.rs` now installs
  `RigModelStreamProvider`; offline tests opt into stub/mock providers explicitly.
- **Coalesce subagent deltas into the main coalescer.** Rejected because coalesced frames carry one
  `agent_id`; subagent frames would be misattributed. Main deltas are coalesced, subagent frames are
  emitted with their own `agent_id` and parent run.
- **Persist only provider tool-call IDs.** Rejected because `agent_tool_calls.tool_call_id` is a
  global primary key while providers reuse IDs such as `call_1` across sessions. The runtime prefixes
  the owning run ID for a stable, collision-free identity.
- **Impose a fixed model-turn or subagent-depth cap.** Rejected by product direction. The loop runs
  until the model stops calling tools; cancellation is the explicit stop mechanism.

## 3. Chosen Approach & Trade-offs

`RigModelStreamProvider` owns an `Arc<RigModelCore>` with `KbService`, `ModelRegistry` and `Config`.
Each turn resolves the agent definition on the session branch, resolves the model route and context
limit, builds the allow-listed built-in/Quire tools, and drives `DynModel::stream` to completion. Tool
calls are executed and appended as assistant/tool-result messages for the next model turn.

Events are pushed into an unbounded channel consumed by the `ModelStreamProvider` stream. Because
subagent tasks push into the same channel, their `SubagentStarted` / `SubagentFinished`, text,
thinking, task-focus, tool and usage events reach the service live even while the parent tool future
is still running.

The accepted costs are: unbounded turn/subagent loops (by direction), per-block preview streamers
(one HTTP stream per block), and direct tool execution serialized within an agent run while
`spawn_subagents` itself fans out with `JoinSet`.
