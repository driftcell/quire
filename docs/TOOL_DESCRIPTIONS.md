# NATIVE TOOL & PARAMETER DOC-COMMENT CONTRACT

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-server`, `quire-runtime`, `quire-session-protocol`
- **Related Notes:** `../notes/quire_typed_json_boundaries.md`

## 1. Scope & Non-Goals

This document governs model-facing tool descriptions and parameter-level schema descriptions across
report MCP tools (`QuireMcpServer`), runtime administration MCP tools (`AdminMcpServer`), and Rig
built-in tools (`#[rig_tool]`). It does not cover agent system prompts, skill resources, or
compaction prompts.

## 2. Canonical Design & Contract

Every tool co-locates its **Tool Description** and **Parameter Descriptions** directly in Rust `///`
doc comments next to the implementation:

1. **Rig Built-in Tools (`#[rig::rig_tool]`)**:
   - The `///` doc comment on the `#[rig_tool]` function defines the **Tool Description** (purpose,
     routing boundaries against sibling tools, preconditions, and receipt/error recovery contract).
   - `///` doc comments directly on each function argument (plus `///` doc comments on fields of
     nested `#[derive(JsonSchema)]` structs such as `SubagentTask`, `QuestionItem`, and
     `QuestionOption`) define the **Parameter Descriptions** (semantics, format constraints,
     mutually exclusive flags, defaults, and side-effect warnings).
   - No intermediate wrapper struct or external Markdown file is used; `server.tool(TOOL)`
     registers the generated zero-sized tool unit struct directly on Rig's `ToolServer`.
2. **RMCP Tools (`#[rmcp::tool]` on `QuireMcpServer` & `AdminMcpServer`)**:
   - The `///` doc comment on each `#[tool]` handler method defines the **Tool Description**.
   - `///` doc comments on the fields of each `#[derive(Deserialize, JsonSchema)]` `*Params` /
     `*Input` struct define the **Parameter Descriptions**.
3. **In-Process Report Tools (`report_inproc.rs`)**:
   - `define_stateless_report_tool!` and `define_contextual_report_tool!` read `(description, schema)`
     from `QuireMcpServer::tool_metadata(name)`, which extracts the `///` doc comments and `schemars`
     parameter schemas from `QuireMcpServer::tool_router()` once. Remote MCP clients and in-process
     Rig agents therefore share one canonical tool and parameter definition per report tool.

## 3. Invariants & Verification

- Tool descriptions focus on *what the tool does*, *when to choose it over sibling tools*, and
  *preconditions / post-call receipt expectations*. Parameter-level rules live on the parameter or
  `JsonSchema` struct field `///` doc comments rather than being duplicated in the tool prose.
- All tool input structs enforce `#[serde(deny_unknown_fields)]` so misspelled arguments fail fast
  with self-healing field names.
- The full Rust and web gates pass after description or schema changes:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
