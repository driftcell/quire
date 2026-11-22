# RIG 0.46 LIFECYCLE CONSOLIDATION

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-22
- **Promoted To:** `../docs/RUNTIME_RIG_STREAMING.md`, `../docs/RUNTIME_ASK_USER.md`, `../docs/RUNTIME_SESSION_EVENTS.md`, `../docs/RUNTIME_MCP_REGISTRY.md`, `../docs/RUNTIME_CHART_PREVIEW.md`, `../docs/RUNTIME_SANDBOX.md`

## 1. Context & Problem

The first native Rig 0.46 runtime retained several v1 ownership boundaries around the stock
`AgentRunner`. Outer cancellation could abort the producer before canonical history and sandbox
cleanup completed; mixed `ask_user` batches split skipped-call audit between hooks; report binding,
preview state and request context held different document IDs; and retry depended on comparing the
last canonical message with the prompt. Built-in tools and chart commits also retained parallel
context and MCP-connection paths.

The runtime needs one owner for each lifecycle decision while keeping the stock runner, canonical
rig-memory history and existing session wire contracts.

## 2. Rejected Options & Why

- **Keep abort-driven outer cancellation.** Rejected because returning or breaking on the token drops
  the producer guard before asynchronous history commit and sandbox cleanup can finish.
- **Reconstruct cancellation history as input history plus the initial prompt.** Rejected because it
  discards accepted intermediate assistant/tool rounds. Hook-driven runner cancellation returns
  Rig's closed canonical history instead.
- **Keep separate question, preview and tool-audit finish events.** Rejected because a skipped Rig
  tool dispatch still reaches `on_outcome`; two owners can emit duplicate terminal events and leave
  provisional previews unresolved.
- **Stop normally in the `ask_user` outcome.** Rejected after tracing Rig's batch driver: a stop there
  drops the current answer from normal batch commit and fail-fast prevents later siblings from
  starting. Normal outcomes proceed; the next completion-call hook stops before the provider call.
- **Copy the initially bound document ID into preview state.** Rejected because `create_report` can
  bind the first document lazily during the same run. Preview and request context must observe that
  shared update.
- **Patch dynamic document state into the preamble.** Rejected because it changes the frozen system
  prefix and can contradict stale request documents. Dynamic state belongs in per-call extra context.
- **Infer retry ancestry from message equality.** Rejected because tool results and interrupted
  suffixes make the last message differ from the user prompt. Retry instead locates the canonical
  `MessageId` and removes the failed run's suffix explicitly.
- **Retain a service-level assistant fallback after real Rig runs.** Rejected because it adds another
  read/write path, can create empty assistant entries and splits one run into inconsistent UI
  receipts.
- **Wrap every built-in tool only to carry runtime handles.** Rejected because Rig 0.46
  `ToolContext::with_scope` preserves live scopes across dispatch clones.
- **Open a dedicated chart-commit MCP connection per call.** Rejected because the registry already
  owns authenticated shared connections and per-call Quire identity belongs in MCP metadata.

## 3. Chosen Approach & Trade-offs

Use one run-control hook and one tool-lifecycle hook, with the latter solely responsible for tool
start/finish audit and preview finalization. Share run-scoped document state, freeze the preamble,
and generate session-state documents at each completion boundary. Commit only runner-produced
history, project one non-empty visible assistant receipt per run, and use canonical message IDs for
retry truncation. Attach runtime handles through tool scopes and route chart commits through the
shared MCP registry with per-call metadata.

The outer stream still retains abort-on-drop as a last-resort guard for abandoned tasks. Graceful
cancellation can therefore wait for a model/tool lifecycle boundary rather than forcibly unloading
an in-flight future; built-in cancellation checks and native model idle timeouts remain necessary.
Canonical memory and operational/UI projections remain eventually consistent, as before.
