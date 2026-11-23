# TOOL DESCRIPTION RESOURCES

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-protocol`, `quire-server`, `quire-runtime`
- **Related Notes:** `../notes/quire_typed_json_boundaries.md`

## 1. Scope & Non-Goals

This document governs model-facing tool descriptions for report MCP, runtime administration MCP and
Rig built-in tools. It does not cover parameter-level schema hints, agent prompts, skill resources or
compaction prompts.

## 2. Canonical Design & Contract

Every tool has one UTF-8 Markdown file at `descriptions/tools/<tool_name>.md`. The file contains only
the description sent to the model; it has no frontmatter or duplicate tool-name heading required by
the runtime. Parameter schemas and parameter-level `params(...)` descriptions remain beside the Rust
input types.

`quire_protocol::tool_description!("tool_name")` expands to `include_str!` with the invoking crate's
`CARGO_MANIFEST_DIR`. Report and admin RMCP attributes use this macro directly. Rig's derive macro
requires a literal description, so runtime registration wraps each generated tool in `DescribedTool<T>`.
The wrapper delegates execution, parameters, annotations and error mapping while `description()` reads
the same external resource through the macro. Both MCP and in-process Rig paths therefore share one
canonical text per tool.

## 3. Invariants & Verification

- Adding, removing or renaming a tool requires the matching file rename and a compile check; a missing
  file fails compilation through `include_str!`.
- No model-facing tool description is duplicated as a long Rust string literal.
- Runtime tests read descriptions through `DescribedTool`, matching the production registration path.
- The full Rust and web gates pass after description changes:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
