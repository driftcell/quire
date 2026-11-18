# DOCUMENT TITLE

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-protocol`, `quire-store`
- **Related Notes:** `../../notes/lowercase_snake_case.md`

## 1. Scope & Non-Goals

State exactly what this document governs and what it deliberately does not cover. Name adjacent
subsystems only to mark the boundary.

## 2. Canonical Design & Contract

Describe the implemented design as a stable contract: crate ownership, types, tables, protocol
fields, state transitions, API/MCP behavior and generated schema locations. Use concrete names and
examples that match the code. Do not describe rejected alternatives here.

## 3. Invariants & Verification

List invariants that must remain true and the tests or commands that prove them. Include the full
gate when relevant:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
