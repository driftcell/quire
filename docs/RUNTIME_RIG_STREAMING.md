# RUNTIME RIG STREAMING

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-runtime`
- **Related Notes:** `../notes/runtime_rig_046_lifecycle_consolidation.md`, `../notes/runtime_rig_046_native_migration.md`, `../notes/runtime_rig_streaming_engine.md`, `../notes/runtime_pending_failure_recovery.md`

## 1. Scope & Non-Goals

This document governs the native Rig 0.46.0 model/tool runtime in `quire-runtime`: Agent construction,
structured conversation history, streaming hooks, tool execution, subagents, pending-queue steering,
usage, cancellation and structured compaction. Quire report MCP semantics, Smart CAS and provider
credential storage remain owned by their existing crates.

The production runtime uses stock `rig-agent::AgentRunner`; it does not use a custom `AgentRun`
driver, serialized per-step checkpoints or a Rig fork.

## 2. Canonical Design & Contract

### 2.1 Dependencies and module layout

The workspace aligns Rig 0.46.0 through the `rig` facade with `derive`, `memory`, `postgres`,
`rmcp` and `reqwest` support. `quire-runtime` imports core, agent, memory, PostgreSQL and MCP types
through that facade rather than maintaining parallel direct dependencies on the component crates.

`crates/quire-runtime/src/rig_agent/` contains:

- `mod.rs`: model/tool assembly and the `AgentRunner` driver;
- `context.rs`: serializable `ToolContext` values and host-only result metadata;
- `hooks.rs`: responsibility-focused hook implementations and their unit tests;
- `builtin_tools.rs`: native Rig `Tool` implementations and allow-list matching;
- `preview_stream.rs`: partial `apply_operations` parsing, preview transport and ID injection;
- `subagents.rs`: child scheduling and canonical child-history resume.

`crates/quire-runtime/src/memory.rs` owns canonical history access and UI-message projection.

### 2.2 Native tools and context

Built-ins are registered directly as native typed `Tool` values in a `ToolServer`. One
`Arc<RuntimeToolScope>` is attached to the runner's `ToolContext::with_scope`; Rig preserves that
live scope in each dispatch clone. There is no wrapper tool or per-call `DynamicTool` / `ToolAdapter`
conversion for built-ins. MCP tools remain dynamic because their names and schemas are discovered at
runtime.

`AgentRunner::tool_context` carries:

- `QuireTurnMeta { session_id, run_id, agent_id, kb_branch, supports_vision }`;
- `McpMeta` with `_meta.quire.{session_id,run_id,agent_id}`.

Runtime handles are not serialized into context. The current report ID lives in one shared
`Arc<RwLock<Option<String>>>` used by tools, previews, subagents and per-call request documents;
`create_report` updates it after the session binding succeeds. `ask_user` publishes
`AskUserRequestedMeta` through `ToolContext::insert_result`; the lifecycle hook reads that host-only
metadata from the outcome.

`preview_report` and `preview_echarts` return ordered `ToolOutput::content` blocks containing
structured diagnostics, an inspection caption and, for vision models, the screenshot. There is no
`pending_preview_images` side channel or synthetic follow-up user image message.

### 2.3 Canonical rig-memory history

The official PostgreSQL memory store is the only canonical conversation history. Fresh runtime
migrations do not create `agent_messages`. User-visible message IDs, client idempotency keys,
timestamps, run IDs and visibility are saved as `quire.message_receipt` Asides anchored to Rig
`MessageId`s. Asides persist with the conversation and are never sent to the model.

A normal user prompt is appended before starting the runner. On load, a recorded prompt must be the
latest canonical entry; the driver removes that entry from the Runner's input history and supplies it
once through `.prompt(prompt)`. A mismatch fails closed instead of appending a duplicate. Retry first
finds the visible user receipt's canonical `MessageId`, removes every later entry and asides anchored
to the removed suffix, saves that splice, and then starts from the retained prompt. Subagents use
`<session_id>:sub:<subagent_run_id>` memory logs and resume through `resume_run_id`, not a
process-local history cache.

The driver normalizes both `PromptResponse::messages` and cancelled `chat_history` into full history,
then commits only the suffix after the exact prefix it loaded. Concurrent prefix changes fail with
expected/current/full-history lengths and the first mismatch index instead of silently rewriting
another turn. Cancellation before Rig commits the initial prompt is a no-op because `append_prompt`
already persisted it. One non-empty visible Assistant receipt aggregates all text turns of a main run
and is anchored to the last text-bearing Assistant entry; empty output gets no receipt or synthetic
empty message. Providers that do not commit canonical history (Stub/Mock) use the service's idempotent
non-empty assistant fallback. The same fallback also preserves a failed run's non-empty response on a
provider stream error; an existing provider receipt makes this recovery write a no-op.

### 2.4 AgentRunner and hook order

`RigModelCore::run_turn` resolves the model route, constructs a `DynModel`, assembles native and MCP
tools, and builds an `Agent`. Each run uses:

- `.history(canonical_prefix)` and the current prompt;
- `.max_turns(usize::MAX)` to preserve effectively unbounded product behavior;
- `.max_invalid_tool_call_retries(3)` and `.max_consecutive_malformed_tool_calls(3)`;
- `.tool_concurrency(8)` so independent calls in one model batch run concurrently. Rig commits
  answers in call order, but tool bodies (including report writes) may interleave, so a report
  mutation must not share a batch with reads or lints that depend on its committed revision;
- native model `.with_idle_timeout(Some(120s))`.

Two hooks are registered in this explicit order:

1. `TurnControlHook`: cancellation, safe-boundary queue supersession, dynamic extra-context
   documents, text/reasoning deltas and the last accepted model call's usage;
2. `ToolLifecycleHook`: partial previews, final `preview_ids` rewrites, mixed `ask_user` batches,
   every tool start/finish, failures, invalid-call feedback and lazy report binding.

The preamble is frozen for the session. `TurnControlHook` reads the shared report ID at every
completion boundary and sends `quire-session-state` plus an optional `quire-knowledge-update` in
`RequestPatch::extra_context`; it never patches the preamble. Normal tools finish preview rewriting
before the same lifecycle hook records the effective arguments and emits start. It is the only hook
that emits tool finish. Provisional text/tool deltas travel through the bounded 256-frame runtime
channel and the single `EventSink` projection path; hooks do not write session databases directly.

### 2.5 Queue steering and terminal states

A queued user message is not inserted into an active Runner's request-only history. At a completion
boundary after tool results, `TurnControlHook` stops the Runner with an internal supersession signal.
The driver commits canonical history and emits `ModelStreamEvent::Superseded`; the service finalizes
the run as `superseded` with `turn_superseded`, then the queue coordinator starts a standard new turn.

Queue claiming writes the idempotent canonical prompt before deleting revalidated queue rows. A crash
between those steps replays the same client key rather than duplicating the message. The drain blocks
`running` and `awaiting_input` plus any latest running run; failed and interrupted sessions may release
their queue once no active run remains. The old `user_message_injected` event and
`RunTraceSegment::UserMessage` no longer exist.

### 2.6 ask_user outline

`ask_user` is a one-shot main-agent tool. All sibling calls in the same model turn receive exactly
one skipped start/finish pair and no execution, independent of position. Normal question and sibling
outcomes proceed so Rig commits the complete tool-result batch, including `awaiting_input`. The
control hook then stops at the following completion-call boundary before any provider request. The
question is persisted and the run enters `awaiting_input`; answers are pure structured JSON inputs to
a new turn. See `RUNTIME_ASK_USER.md`.

### 2.7 Usage, cancellation and cleanup

`TurnControlHook` keeps the last accepted completion's counters. `context_used` uses provider context tokens
when available and `context_limit` comes from the resolved model row. Subagent usage remains scoped to
the child and never overwrites the main session meter.

Hooks stop at run-start, completion, delta, model-turn and dispatch boundaries. The driver may observe
the run `CancellationToken` while waiting for the next stream item, but it only records cancellation
and keeps driving the Runner to Rig's canonical `PromptError::Cancelled` history; it never replaces
that history with input-plus-prompt. Neither the service loop nor the producer channel breaks on the
token. The channel closes only after `run_turn` commits history and awaits sandbox cleanup, and the
service then performs the cancelled terminal transition. `AbortOnDrop` remains only a last-resort
guard when the consumer itself is abandoned. Dropping a subagent `JoinSet` cancels children. Native
idle timeout applies before the first frame and between frames; long tool execution is not
misclassified as model stream silence.

### 2.8 Structured compaction

Compaction reads canonical messages from rig-memory and calls
`prompt_typed::<CompactionHandoff>(COMPACTION_INSTRUCTION).retries(1)` with native structured output.
The result projects to the stable Markdown handoff, then `rig::memory::apply_summary` splices it into
the conversation while preserving pairing and undo metadata. Automatic compaction can preserve the
new user prompt by anchoring `keep_from` to its canonical `MessageId`. The old text-stream heading
validator and `compacted_up_to_message_id` history are removed.

## 3. Invariants & Verification

- Production uses stock `AgentRunner` and native hooks, never the direct `DynModel::stream` loop.
- Canonical history preserves structured ToolCall/ToolResult pairs and multimodal output.
- A prompt appears exactly once in the provider request and one committed suffix is appended per run.
- Retry rewinds by canonical `MessageId`; early cancellation is a no-op, not a prefix error.
- A main run has at most one visible Assistant receipt, aggregating its non-empty text turns.
- The preamble is byte-frozen; dynamic report/KB state appears only in tail extra-context documents.
- Built-ins are native typed tools; per-call metadata is serializable and runtime handles stay live.
- Mixed `ask_user` batches execute no sibling tool and emit exactly one skipped finish per sibling.
- Pending messages create a `superseded` old run and a standard new run, never inline history patches.
- Invalid-call retries are bounded while total normal model turns remain effectively unbounded.
- Last-call usage, native idle timeout, cancellation and sandbox cleanup retain their distinct roles.
- Main/subagent memory logs are independent and child resume needs no process-local message cache.
- Compaction is typed, schema-constrained and applied to canonical rig-memory history.

Verification includes both mixed-batch HookStack orders, lazy document preview binding, frozen
preamble/dynamic context, aggregate/empty receipts, producer cleanup, repeated receipt appends and
ordered subagent slots. Integration coverage includes canonical retry with a cancelled tool suffix,
early cancellation commit, queue supersession, `failed_turn_preserves_text_and_releases_pending_queue`,
question answer/skip/dismissal with canonical dismissal context, compaction, cancellation, subagent
scopes and real model/tool streaming.

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
