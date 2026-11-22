# RIG 0.46 NATIVE RUNTIME MIGRATION

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-21
- **Promoted To:** `../docs/RUNTIME_RIG_STREAMING.md`, `../docs/RUNTIME_ASK_USER.md`, `../docs/RUNTIME_SESSION_EVENTS.md`

## 1. Context & Problem

`quire-runtime` drives `DynModel<Completion>` directly and manually reconstructs the model/tool
loop, tool telemetry, previews, queue steering, cancellation and questionnaire suspension. This
duplicates `rig-agent` lifecycle responsibilities and the text-only `agent_messages` history drops
native ToolCall/ToolResult pairing and multimodal content.

The migration targets Rig 0.46.0 without backward-compatibility shims. The repository is still in
development, so protocol, frontend, migrations and tests may change together when that produces a
cleaner contract.

## 2. Rejected Options & Why

- **Keep the manual `DynModel::stream` loop.** Rejected because it preserves the duplicated agent
  runtime and cannot use Rig's production `AgentHook` lifecycle automatically.
- **Fully self-drive `AgentRun` with stock Rig.** Rejected because stock Rig 0.46.0 does not expose a
  production hook-aware custom driver; `HookContext` construction and the model/tool dispatch engine
  are crate-private. Using `HookTestHarness` in production or copying the private engine would create
  a larger maintenance problem.
- **Patch or fork Rig to expose internal run state.** Rejected after comparison with the supported
  `AgentRunner` path. The accepted design uses the stock runner and gives up serialized `AgentRun`
  checkpoints and same-run `agent.resume` for `ask_user`.
- **Inject queued messages into the same Quire run.** Rejected because stock `AgentRunner` does not
  expose a canonical mid-run append operation. Request-only history patches do not update canonical
  memory and cannot reliably place a user message after the current tool-result prompt.
- **Resume `ask_user` through a serialized `AgentRun`.** Rejected with the stock-runner decision.
  Answers instead form one structured JSON input to a new turn.
- **Require `ask_user` to be the only model-emitted call.** Rejected because it needs an extra model
  correction round or fails otherwise valid mixed plans. Instead, every sibling call is skipped before
  execution, independent of position, so no sibling produces side effects.
- **Persist completed turns in Quire-owned text messages.** Rejected because text-only history loses
  native tool pairing, structured output and media. `rig-postgres`/`rig-memory` is the canonical
  conversation store.
- **Keep `session_events` as a second durable fact source.** Rejected because it duplicates canonical
  history and requires cross-store reconciliation. Active traces use a Redis hot projection; completed
  runs store a derived `RunTraceSummary` in `agent_runs.trace_summary`.
- **Retain `pending_preview_images` and synthetic user image messages.** Rejected in favor of native
  multimodal `ToolOutput::content` in the originating tool result.
- **Introduce a fixed 64-call turn cap.** Rejected because the current product direction is an
  effectively unbounded loop. The runner uses `usize::MAX`; invalid-call retry budgets remain bounded.
- **One monolithic Quire hook.** Rejected in favor of explicitly ordered, responsibility-focused hooks
  for previews, tool audit, run control and usage/lifecycle.

## 3. Chosen Approach & Trade-offs

- Use the Rig facade and stock `AgentRunner`/`AgentHook`; drive streaming with native model idle
  timeouts, structured tools, bounded invalid-call recovery and `usize::MAX` model-call budget.
- Use official `rig-postgres` 0.46.0 with its `memory` feature as the only canonical structured
  conversation history. Session messages and traces are projections, not alternate model facts.
- Implement built-in tools as native Tool types: shared runtime handles live on the tool value,
  serializable per-run data uses `ToolContext`, and host-only result metadata uses `insert_result`.
- Return preview diagnostics and screenshots as ordered multimodal `ToolOutput::content` blocks.
- Split hooks by responsibility and register them in an explicit order. Hooks emit runtime events;
  they do not become a second authorization or database-transaction boundary.
- `ask_user` remains a one-shot main-agent tool. In a mixed batch all sibling calls receive structured
  skipped results without executing. The question is durable; its answer is a pure JSON payload that
  starts a new turn. No prose continuation prompt or same-run resume remains.
- A pending user message ends the current turn at the next safe boundary with an explicit `superseded`
  terminal state/event, then starts a new turn. Hard interruption remains distinct.
- Active trace state lives in Redis with sequencing and hot snapshots for reconnect. On terminal
  transition, materialize the complete derived trace to `agent_runs.trace_summary` and remove hot
  state. Session/SSE frames are display data only.
- Pool MCP connections by server/auth identity. Carry session/run/agent identity per call through
  namespaced MCP `_meta`; `quire-server` validates and consumes that metadata. This intentionally
  expands the migration into `quire-server`.
- Compaction uses `prompt_typed::<CompactionHandoff>` with native structured output and one retry.

Accepted costs: official rig-memory commits are not transactionally coupled to Quire display
projections; stock Runner cancellation requires explicit handling of returned canonical history;
hot Redis state and cold trace materialization need reconciliation; and the provider matrix must
support the selected native structured-output compaction path.
