# NATIVE TOOL & PARAMETER DOC-COMMENT CONTRACT

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-server`, `quire-runtime`, `quire-session-protocol`
- **Related Notes:** `../notes/quire_typed_json_boundaries.md`, `../notes/runtime_native_report_tools.md`, `../notes/mcp_preview_tool_exposure.md`, `../notes/task_focus_and_todo_split.md`

## 1. Scope & Non-Goals

This document governs model-facing tool descriptions and parameter-level schema descriptions across
native runtime report tools, the optional report rmcp adapter (`QuireMcpServer`), runtime
administration MCP tools (`AdminMcpServer`), and other Rig built-in tools. It does not cover agent
system prompts, skill resources, or compaction prompts.

## 2. Canonical Design & Contract

Every tool co-locates its **Tool Description** and **Parameter Descriptions** in Rust next to the
implementation:

1. **Rig Built-in Tools (`#[rig::rig_tool]`)**:
   - The `///` doc comment on the `#[rig_tool]` function defines the **Tool Description** (purpose,
     routing boundaries against sibling tools, preconditions, and receipt/error recovery contract).
   - `///` doc comments directly on each function argument (plus `///` doc comments on fields of
     nested `#[derive(JsonSchema)]` structs such as `SubagentTask`, `QuestionItem`, and
     `QuestionOption`) define the **Parameter Descriptions** (semantics, format constraints,
     mutually exclusive flags, defaults, and side-effect warnings).
   - No intermediate wrapper struct or external Markdown file is used; `server.tool(TOOL)`
     registers the generated zero-sized tool unit struct directly on Rig's `ToolServer`.
   - Status-tracking built-ins keep concerns split with explicit scenario boundaries in their `///`
     descriptions: `describe_task` broadcasts live work focus only (`ModelStreamEvent::TaskFocus`,
     `DescribeTaskReceipt`), while `update_todos` replaces the structured checklist dynamically
     (`ModelStreamEvent::TodosUpdated`, `UpdateTodosReceipt`) and auto-registers alongside
     `describe_task` for both lead and subagent runs.
2. **Native Report Tools (`report_inproc.rs`)**:
   - Every one of the 19 tools uses `#[rig_tool(name = "...")]`. Function and argument `///`
     comments define the model-facing description and flat parameter schema; no custom report-tool
     definition macro or rmcp `tool_metadata` remains.
   - Existing public fields stay as direct function arguments. Explicit `required(...)` lists make
     defaulted non-`Option` fields optional without nesting them under a synthetic request object.
     A transparent `SourceInputMap` preserves ordered `IndexMap` deserialization while advertising
     the established object-map schema.
   - Generated Rig dispatch calls `QuireReportService` directly and returns its domain receipt. The
     service owns execution, not model-facing tool discovery.
3. **RMCP Tools (`#[rmcp::tool]` on `QuireMcpServer` & `AdminMcpServer`)**:
   - The `///` doc comment on each `#[tool]` handler method defines the **Tool Description**.
   - `///` doc comments on the fields of each `#[derive(Deserialize, JsonSchema)]` `*Params` /
     `*Input` struct define the **Parameter Descriptions**.
   - Report rmcp handlers are thin transport adapters over the same `QuireReportService`; the three
     preview handlers use `PreviewToolService` through the external harness bridge, with field-level
     contracts in `PreviewReportInput`, `PreviewEchartsParams` and `CommitChartParams`.
   - `/admin/mcp` remains an independent administrative rmcp server; its `preview_report` adapter also
     delegates to `PreviewToolService`.

## 3. Invariants & Verification

- Tool descriptions focus on _what the tool does_, _when to choose it over sibling tools_, and
  _preconditions / post-call receipt expectations_. Parameter-level rules live on the parameter or
  `JsonSchema` struct field `///` doc comments rather than being duplicated in the tool prose.
- All tool input structs enforce `#[serde(deny_unknown_fields)]` so misspelled arguments fail fast
  with self-healing field names.
- The full Rust and web gates pass after description or schema changes:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
