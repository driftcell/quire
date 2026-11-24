# RUNTIME NATIVE REPORT TOOLS

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-24
- **Promoted To:** `../docs/RUNTIME_RIG_STREAMING.md`, `../docs/TOOL_DESCRIPTIONS.md`

## 1. Context & Problem

The agent runtime registers report tools as native Rig `Tool` values, but their implementations and
model-facing metadata still come from `QuireMcpServer`. This leaves two tool architectures in the
runtime path and makes it difficult to reason about which descriptions and execution contracts an
agent actually receives. The accepted product boundary is that every agent-runtime report tool is a
native Rig tool, while the administrative `/admin/mcp` control plane remains an rmcp server.

The immediate stale-excerpt incident was separately traced to knowledge-base seeding: startup only
imports agents and skills into an empty branch, so restarting did not upsert edited workspace prompts.
The native-tool migration must not be presented as a fix for that KB synchronization bug.

## 2. Rejected Options & Why

- **Keep the native wrapper around rmcp handlers and `tool_metadata`.** Rejected because it preserves
  the two-layer tool path and the ambiguity about the canonical model-facing definition.
- **Copy every report mutation directly into runtime-built tools.** Rejected because Smart CAS,
  lint scoping, comment publication, source deduplication and receipt behavior would diverge between
  runtime and other report interfaces.
- **Remove the administrative rmcp server.** Rejected explicitly: `/admin/mcp` remains the management
  control plane and is unrelated to agent tool dispatch.
- **Rewrite external search/database MCP tools as built-ins.** Rejected as out of scope. Their names
  and schemas are discovered dynamically and they are not report-domain tools.
- **Add metadata string-containment tests as the main verification.** Rejected because they do not
  demonstrate that a report tool executed correctly. Existing end-to-end tool-call workflows provide
  the meaningful regression gate.

## 3. Chosen Approach & Trade-offs

Extract report request/receipt contracts and domain execution into a shared `QuireReportService` in
`quire-server`. Declare every runtime report tool with `#[rig_tool(name = "...")]`; function and
argument doc comments own the model-facing definition, and generated Rig dispatch calls the shared
service with the existing in-process actor/session context. Explicit `required(...)` lists preserve
optional non-`Option` fields without changing the flat JSON arguments. Any retained transport adapter
delegates to the same service rather than owning report behavior. `/admin/mcp` and dynamic external
business MCP tools remain unchanged.

The migration preserves tool names, flat input shapes, receipts, Smart CAS behavior, lazy session
report binding, preview commits and comment notifications. The accepted cost is a one-time separation
of transport adapters from domain service code; the service temporarily shares the existing
structured report error contract so error hints do not regress during the move.

Verification uses existing native runtime report workflow calls and the full Rust workspace gate, not
new description/schema string assertions. Historical reports and source excerpts are not modified.
