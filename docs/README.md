# Quire Documentation

`docs/` is the single source of truth for settled architecture, contracts and verified behavior.
Decision history and rejected alternatives live in `../notes/`, not here.

## Read order

1. Read `../AGENTS.md`.
2. Read only the documents for the subdomain you are changing.
3. Consult `../notes/` only when you need to understand why a previous option was rejected.

## Structure and naming

- Group by subdomain when useful (`architecture/`, `quire/`, `runtime/`, etc.).
- Use `UPPER_SNAKE_CASE.md` for every document, for example `ECHARTS_6_CONTRACT.md`.
- `README.md` and `TEMPLATE.md` are the only permitted exceptions.
- Every new document must copy and follow [`./TEMPLATE.md`](./TEMPLATE.md).
- Link relevant decision notes from the document's `Related Notes` metadata field.

## What belongs here

- Settled architecture boundaries and crate responsibilities.
- Database tables, protocol contracts and generated schema locations.
- State machines, API/MCP behavior and error contracts.
- Invariants that are enforced by code and covered by verification.
- Operational constraints that all agents must preserve.

## What is forbidden here

- Local machine paths, `.env` contents, test tokens, API keys or database connection strings.
- Internal IP addresses or personal shell/editor preferences.
- Unimplemented ideas, open-ended option comparisons or "we are still deciding" prose.
- Raw terminal logs or large source dumps.

If a decision is not implemented and verified yet, write it in `../notes/` first. Promote it here only
after the code lands and the full gate passes.
