# RUST 2024 ENGINEERING CONTRACT

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-protocol`, `quire-lint`, `quire-runtime`, `quire-server`, `quire-store`, `quire-session-protocol`
- **Related Notes:** `../notes/rust_2024_async_boundaries.md`, `../notes/quire_typed_json_boundaries.md`

## 1. Scope & Non-Goals

This document defines async-trait boundaries, Edition 2024 capture rules, large-enum layout,
multi-parameter write commands and lock choices used by the Rust workspace. It does not alter report
wire formats, tool schemas or model-provider behavior.

## 2. Canonical Design & Contract

### Async traits

`InvalidationSubscriber` and `SessionEventLog` use native RPITIT with explicit `Send` futures for
static dispatch. `ModelStreamProvider`, `SandboxProvider` and `Sandbox` remain object safe because
runtime tests and the service substitute dynamic implementations. Their methods return explicit boxed
futures; `SandboxFuture<'a, T>` is the shared alias. No crate uses `async-trait`.

`DescribedTool::call` uses Edition 2024 precise capturing (`use<'a, 'b, T>`) so the opaque future
captures only the actual self/context lifetimes and tool type. Dynamic provider substitution remains
part of the supported test and runtime contract.

### Data layout and write commands

`PatchOp::PatchBlock` boxes `BlockPatch`; large cover/theme variants are boxed as well, and the raw
forgiving patch representation is boxed too. `BlockDiffEntry` boxes block payloads. Serde and
schemars keep the same JSON representation. Unit tests require `PatchOp`, `RawPatchOp` and
`BlockDiffEntry` to occupy at most 96 bytes on the supported target.

KB agent writes use `UpsertKbAgentCommand`; pending-message insertion uses `EnqueuePendingMessage`.
Rig tool functions keep separate arguments because each argument is a separate model-visible JSON
Schema field, with a local `expect` reason rather than an unexplained allow.

### Regex and locks

Built-in regular expressions use `LazyLock<Result<Regex, regex::Error>>`. Failure is logged rather
than silently converting the regex into `None`; unit tests require all built-in patterns to compile.
The Aho-Corasick cliche automaton uses the same result discipline and a tested linear fallback.

`SandboxLease.current` is protected by a short non-poisoning synchronous mutex. The separate Tokio
mutex serializes creation/destruction I/O; no synchronous lock is held across an `.await`.

### Module size and public compatibility

Production source files stay at or below 650 lines. Large modules use directory modules with cohesive
child files and, where useful, multiple inherent `impl` blocks for the same type. Original module roots
retain public re-exports, tool registration and MCP route composition so downstream paths and wire
contracts remain unchanged. Test-only files are outside this production-size limit.

## 3. Invariants & Verification

- No `async-trait`, old `ref mut` binding form or `#[allow]` remains in the workspace.
- Protocol JSON and generated TypeScript contracts remain unchanged after boxing.
- All dynamic model/sandbox providers compile without macros, including custom integration fakes.
- Patch/diff size assertions and the complete workspace gate pass:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
