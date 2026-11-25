# AGENTS.md

Operational contract for AI agents working in this repository. Keep this file short; durable
knowledge belongs in `docs/`, decision history belongs in `notes/`.

## Repository boundaries

| Crate | Boundary |
| --- | --- |
| `quire-protocol` | Pure synchronous domain protocol: Block AST, patch engine, ranks, citations, Markdown projection, lint wire contracts, report/session wire types. Zero I/O. |
| `quire-lint` | Pure synchronous lint rule registry and engine: `QRxxx` codes, select/ignore filters, document/block suppression. Zero I/O, no CLI. |
| `quire-store` | PostgreSQL persistence, Smart CAS revisions, snapshots, Redis Streams/PubSub. No HTTP or MCP concerns. |
| `quire-server` | Shared `QuireReportService`, report tool contracts, Axum REST/SSE and optional `rmcp` transport adapters. Must not depend on `rig` or agent-runtime internals. |
| `quire-session-protocol` | Pure synchronous Session REST/SSE contracts and JSON Schema export. Zero I/O. |
| `quire-runtime` | Unified session backend and agent runtime. Native report `rig::tool::Tool`s call `QuireReportService` directly; mounts `/api/docs/*`, optional report `/mcp`, and the independent `/admin/mcp` control plane. |
| `quire-migrate` | Standalone SQLx migration runner for fresh/bootstrap databases. Owns no business logic and has no application-crate dependencies. |

Dependency direction is one-way: protocol crates never depend on storage/server/runtime crates, and
`quire-server` remains independent of `quire-runtime` and `rig`.

## Engineering rules

- Every crate keeps `#![forbid(unsafe_code)]`.
- Workspace Clippy denies `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `print_stdout`,
  `print_stderr` and `indexing_slicing`. Do not bypass these lints without an explicit, local reason.
- Protocol crates stay synchronous and perform no network, filesystem, database or Redis I/O.
- Secrets use `secrecy::SecretString`; never log, serialize or return raw tokens/API keys.
- Before declaring work complete, run:

  ```bash
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  ```

- If a protocol type changes, regenerate and commit the contracts:

  ```bash
  just schema   # contracts/report-protocol.schema.json + contracts/session-protocol.schema.json
  ```

## Documentation workflow

1. **Read `docs/` first.** At task start read only `AGENTS.md` plus the `docs/` entries relevant to
   the subdomain. Consult `notes/` only when tracing why an alternative was rejected; do not load all
   notes by default.
2. **Discuss before settling.** Trade-offs, option comparisons and rejected alternatives go to
   `notes/`. Write a note while the decision is still being made.
3. **Document only after implementation.** Update or create `docs/` only when the code has landed and
   the verification gate above passes. `docs/` is the single source of truth.
4. **Templates are mandatory.** Every new document must follow `docs/TEMPLATE.md` or
   `notes/TEMPLATE.md`.
5. **Bidirectional links.** `docs/` pages list `Related Notes`; `notes/` pages list `Promoted To`.
   This prevents later agents from re-proposing rejected designs.

Naming rules: `docs/` uses `UPPER_SNAKE_CASE.md`; `notes/` uses `lowercase_snake_case.md`.
`README.md` and `TEMPLATE.md` are the only exceptions in both trees.
