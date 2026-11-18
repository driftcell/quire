# Quire Decision Notes

`notes/` records decision context, rejected alternatives and the trade-offs behind the settled
contracts in `../docs/`. It is history, not the source of truth for current behavior.

## Read order

Read `../AGENTS.md` and the relevant `../docs/` pages first. Open a note only when you need to know
why an earlier design was rejected or what cost was accepted.

## Structure and naming

- Group notes by subdomain when useful (`architecture/`, `quire/`, `runtime/`, etc.).
- Use `lowercase_snake_case.md` for every note, for example
  `echarts_ssr_vs_playwright.md`.
- `README.md` and `TEMPLATE.md` are the only permitted exceptions.
- Every new note must copy and follow [`./TEMPLATE.md`](./TEMPLATE.md).
- When a note results in a settled contract, set `Promoted To` to the corresponding `docs/` path.
  The promoted document must link back through `Related Notes`.

## What belongs here

- The problem and constraints that triggered a decision.
- Alternative designs, including the **rejected options and the reason each was rejected**.
- The chosen approach, known costs, limitations and follow-up conditions.
- Superseded decisions, with a link to the replacement note or document.

## What is forbidden here

- Local machine paths, credentials, tokens, database URLs or internal IPs.
- Large raw terminal logs or full source-file dumps.
- Untraceable opinions without context or consequences.
