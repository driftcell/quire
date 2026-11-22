# RUNTIME RIG STREAMING

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-runtime`
- **Related Notes:** `../notes/runtime_rig_046_native_migration.md`, `../notes/runtime_rig_streaming_engine.md`

## 1. Scope & Non-Goals

This document governs the native Rig 0.46.0 model/tool runtime in `quire-runtime`: Agent construction,
structured conversation history, streaming hooks, tool execution, subagents, pending-queue steering,
usage, cancellation and structured compaction. Quire report MCP semantics, Smart CAS and provider
credential storage remain owned by their existing crates.

The production runtime uses stock `rig-agent::AgentRunner`; it does not use a custom `AgentRun`
driver, serialized per-step checkpoints or a Rig fork.

## 2. Canonical Design & Contract

### 2.1 Dependencies and module layout

The workspace uses aligned Rig 0.46.0 crates:

- `rig` with `agent`, `derive`, `memory`, `rmcp` and `reqwest` support;
- `rig-core` for provider/model and message types;
- `rig-postgres` with `memory` for the official `PostgresSessionStore`;
- `rig-rmcp` for MCP connections, tools and per-call metadata.

`crates/quire-runtime/src/rig_agent/` contains:

- `mod.rs`: model/tool assembly and the `AgentRunner` driver;
- `context.rs`: serializable `ToolContext` values and host-only result metadata;
- `hooks.rs`: responsibility-focused hook implementations and their unit tests;
- `builtin_tools.rs`: native Rig `Tool` implementations and allow-list matching;
- `preview_stream.rs`: partial `apply_operations` parsing, preview transport and ID injection;
- `subagents.rs`: child scheduling and canonical child-history resume.

`crates/quire-runtime/src/memory.rs` owns canonical history access and UI-message projection.

### 2.2 Native tools and context

Built-ins are registered as native typed `Tool` values in a `ToolServer`. `RuntimeTool<T>` holds an
`Arc<RuntimeToolScope>` for shared handles and delegates the concrete argument/output types,
annotations, footprints and `map_error` to the wrapped tool. There is no per-call `DynamicTool` /
`ToolAdapter` conversion for built-ins. MCP tools remain dynamic because their names and schemas are
discovered at runtime.

`AgentRunner::tool_context` carries:

- `QuireTurnMeta { session_id, run_id, agent_id, kb_branch, doc_id, supports_vision }`;
- `McpMeta` with `_meta.quire.{session_id,run_id,agent_id}`.

Runtime handles are not serialized into context. `ask_user` publishes `AskUserRequestedMeta` through
`ToolContext::insert_result`; hooks read that host-only metadata from the outcome.

`preview_report` and `preview_echarts` return ordered `ToolOutput::content` blocks containing
structured diagnostics, an inspection caption and, for vision models, the screenshot. There is no
`pending_preview_images` side channel or synthetic follow-up user image message.

### 2.3 Canonical rig-memory history

The official PostgreSQL memory store is the only canonical conversation history. Fresh runtime
migrations do not create `agent_messages`. User-visible message IDs, client idempotency keys,
timestamps, run IDs and visibility are saved as `quire.message_receipt` Asides anchored to Rig
`MessageId`s. Asides persist with the conversation and are never sent to the model.

A normal user prompt is appended before starting the runner. On load, if canonical history ends with
that exact prompt, the driver removes it from the Runner's input history and supplies it once through
`.prompt(prompt)`. If it does not match, as when retrying an older message, the prompt is treated as a
new internal entry. Subagents use `<session_id>:sub:<subagent_run_id>` memory logs and resume through
`resume_run_id`, not a process-local history cache.

The driver normalizes both `PromptResponse::messages` and cancelled `chat_history` into full history,
then commits only the suffix after the exact prefix it loaded. Concurrent prefix changes fail instead
of silently rewriting another turn. Stub/mock providers use the same canonical store through an
idempotent assistant fallback in the service.

### 2.4 AgentRunner and hook order

`RigModelCore::run_turn` resolves the model route, constructs a `DynModel`, assembles native and MCP
tools, and builds an `Agent`. Each run uses:

- `.history(canonical_prefix)` and the current prompt;
- `.max_turns(usize::MAX)` to preserve effectively unbounded product behavior;
- `.max_invalid_tool_call_retries(3)` and `.max_consecutive_malformed_tool_calls(3)`;
- `.tool_concurrency(1)` through the runner default;
- native model `.with_idle_timeout(Some(120s))`.

Hooks are registered in this explicit order:

1. `QuestionHook`: detects an `ask_user` batch, skips every sibling before preview/execution, and
   turns successful question outcomes into durable suspension;
2. `RunControlHook`: cancellation, safe-boundary queue supersession and per-turn preamble/context
   patches;
3. `PreviewHook`: partial argument previews, final flush/anchor correction and `preview_ids` rewrites;
4. `ToolAuditHook`: actual tool input/output, timing, failures, skipped results and report binding;
5. `StreamEventHook`: text/reasoning deltas and cancellation during streaming;
6. `InvalidToolCallHook`: malformed-JSON feedback separately from unknown/disallowed tool feedback;
7. `UsageHook`: the last accepted model call's usage, not aggregate multi-call usage.

Normal tools have previews applied before audit observes the effective arguments. An `ask_user`
failure continues to tool audit and receives a failed finish instead of leaving a running row.
Provisional text/tool deltas travel through the existing bounded 256-frame runtime channel and the
single `EventSink` projection path; hooks do not write session databases directly.

### 2.5 Queue steering and terminal states

A queued user message is not inserted into an active Runner's request-only history. At a completion
boundary after tool results, `RunControlHook` stops the Runner with an internal supersession signal.
The driver commits canonical history and emits `ModelStreamEvent::Superseded`; the service finalizes
the run as `superseded` with `turn_superseded`, then the queue coordinator starts a standard new turn.

Queue claiming writes the idempotent canonical prompt before deleting revalidated queue rows. A crash
between those steps replays the same client key rather than duplicating the message. The old
`user_message_injected` event and `RunTraceSegment::UserMessage` no longer exist.

### 2.6 ask_user outline

`ask_user` is a one-shot main-agent tool. All sibling calls in the same model turn receive skipped
start/finish records and no execution, independent of position. The question is persisted and the
run enters `awaiting_input`; answers are pure structured JSON inputs to a new turn. See
`RUNTIME_ASK_USER.md`.

### 2.7 Usage, cancellation and cleanup

`UsageHook` keeps the last accepted completion's counters. `context_used` uses provider context tokens
when available and `context_limit` comes from the resolved model row. Subagent usage remains scoped to
the child and never overwrites the main session meter.

The outer stream loop listens to the run `CancellationToken`; hooks also stop at delta and dispatch
boundaries. Dropping `StreamingResult` cancels in-flight model/tool futures and dropping a subagent
`JoinSet` cancels children. Sandbox cleanup belongs to the outer `run_turn`, not a per-hook settled
callback, so every normal, error and cancellation exit shares one cleanup owner. Native idle timeout
applies before the first frame and between frames; long tool execution is not misclassified as model
stream silence.

### 2.8 Structured compaction

Compaction reads canonical messages from rig-memory and calls
`prompt_typed::<CompactionHandoff>(COMPACTION_INSTRUCTION).retries(1)` with native structured output.
The result projects to the stable Markdown handoff, then `rig_memory::apply_summary` splices it into
the conversation while preserving pairing and undo metadata. Automatic compaction can preserve the
new user prompt by anchoring `keep_from` to its canonical `MessageId`. The old text-stream heading
validator and `compacted_up_to_message_id` history are removed.

## 3. Invariants & Verification

- Production uses stock `AgentRunner` and native hooks, never the direct `DynModel::stream` loop.
- Canonical history preserves structured ToolCall/ToolResult pairs and multimodal output.
- A prompt appears exactly once in the provider request and one committed suffix is appended per run.
- Built-ins are native typed tools; per-call metadata is serializable and runtime handles stay live.
- Mixed `ask_user` batches execute no sibling tool and persist every sibling as skipped.
- Pending messages create a `superseded` old run and a standard new run, never inline history patches.
- Invalid-call retries are bounded while total normal model turns remain effectively unbounded.
- Last-call usage, native idle timeout, cancellation and sandbox cleanup retain their distinct roles.
- Main/subagent memory logs are independent and child resume needs no process-local message cache.
- Compaction is typed, schema-constrained and applied to canonical rig-memory history.

Verification includes runtime unit tests, `HookTestHarness` sibling-skip coverage, repeated memory
receipt append tests, and integration tests for queue supersession, question answer/skip/dismissal,
canonical compaction, cancellation, subagent scopes and real model/tool streaming.

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
