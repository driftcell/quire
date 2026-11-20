# QUIRE LINT REGISTRY SPLIT

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-20
- **Promoted To:** `../../docs/REPORT_LINT_RULES.md`

## 1. Context & Problem

The original linter lived in `quire-protocol/src/linter.rs`. It mixed pure lint output contracts with
a large rule engine, hardcoded uppercase codes, no centralized registry and no machine-readable
ignore policy. Adding rules changed the protocol crate, callers could not filter diagnostics, and
agents had no way to tell whether a warning was safe to suppress.

The repository requires zero technical debt, no dual-track compatibility and a pure synchronous
library boundary. `quire-server` and `quire-store` need to embed linting, while frontend contracts
must expose stable rule identity and suppression metadata.

## 2. Rejected Options & Why

- **Keep the engine in `quire-protocol`.** Rejected: protocol changes would be forced by every rule
  tweak, the crate would keep a mixed responsibility, and the store/server would still pull the
  entire rule engine through the protocol surface.
- **Ship a `quire-lint` CLI.** Rejected: the actual consumers are embedded libraries. A CLI adds a
  binary, argument parsing and a second configuration path without serving the report workflow.
- **Keep legacy uppercase codes or add a forwarding shim.** Rejected: two code vocabularies would
  drift, tests would keep asserting obsolete names and agents would need a compatibility table.
- **Duplicate code constants in protocol for normalization warnings.** Rejected: normalization also
  emits diagnostics, so its metadata would become a second registry. The diagnostic-producing
  normalizer moved into `quire-lint`; protocol keeps only pure JSON transform helpers.
- **Store ignore policy only in frontend local storage.** Rejected: lint policy is report data and
  must survive reload, rollback and multi-client rendering. It belongs in `ReportDocument.lint`.

## 3. Chosen Approach & Trade-offs

`quire-protocol` now owns only wire contracts and document configuration. `quire-lint` owns the
central `RuleId`/`RuleMeta` registry, the rule modules and the three-layer select/ignore engine.
Diagnostics carry `code`, `rule`, `suppressible`, `hint` and optional anchors. Non-suppressible core
and safety rules are enforced in the engine rather than by convention.

The cost is one additional workspace crate and a store migration for `ReportDocument.lint`. In
exchange, rules evolve independently of protocol serialization, callers get deterministic filtering,
and agents receive self-describing diagnostics without hardcoded rule lists in prompts.
