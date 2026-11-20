# REPORT LINT RULES AND FILTERS

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-protocol`, `quire-lint`, `quire-store`, `quire-server`
- **Related Notes:** `../notes/quire_lint_registry_split.md`

## 1. Scope & Non-Goals

This document governs the static report linter: crate ownership, rule registration, diagnostic
contract, layered ignore/select behavior, persistence and MCP exposure.

It does not cover Markdown projection, ECharts rendering, frontend lint visualization or runtime
agent prompting. The linter is a pure synchronous library; it performs no database, HTTP, filesystem
or CLI work.

## 2. Canonical Design & Contract

### Crate ownership

| Crate | Responsibility |
| --- | --- |
| `quire-protocol` | Wire contracts: `Diagnostic`, `LintSummary`, `LintReport`, `LintConfig`, `Severity`; `ReportDocument.lint`; `Block.lint_ignore`; `BlockPatch.lint_ignore`; `PatchOp::SetLint`. |
| `quire-lint` | `RuleId`, `RuleMeta`, `RULES`, `LintOptions`, `lint_document`, `lint_document_with_options`, `lint_echarts_block` and the normalization pass. |
| `quire-store` | Persists `report_documents.lint` JSONB and block payload `lint_ignore`; runs lint for inline warnings and blocking errors. |
| `quire-server` | Exposes `lint_document(doc_id, select?, ignore?)` and forwards diagnostics to MCP receipts. |

`quire-lint` depends only on `quire-protocol` and `serde_json`. There is no compatibility module,
re-export shim or legacy uppercase diagnostic code in `quire-protocol`.

### Diagnostic contract

Every diagnostic carries:

```json
{
  "code": "QR202",
  "rule": "table-empty-column",
  "severity": "warning",
  "message": "...",
  "hint": "...",
  "suppressible": true,
  "block_id": "financials",
  "path": "blocks[3].rows.spare"
}
```

`code` is the stable `QRxxx` identifier. `rule` is the kebab-case semantic name. `suppressible`
is `false` for core contract and safety rules and `true` for advisory rules. `LintSummary.suppressed`
counts suppressible diagnostics hidden by any ignore layer; it is omitted when zero.

### Rule registry

`quire-lint/src/registry.rs` defines `RuleId`, `RuleMeta`, `RULES` and `rule_matches_token`. Codes and
names are globally unique and code-sorted. The registry is the single source of truth; docs do not
duplicate individual rule metadata.

Category ranges:

| Range | Family | Policy |
| --- | --- | --- |
| `QR0xx` | Document/block/table core contract | Non-suppressible |
| `QR1xx` | Citations and source quality | `QR101` non-suppressible; quality advisories suppressible |
| `QR2xx` | Table layout risk | Suppressible warnings |
| `QR3xx` | ECharts safety/data | `QR301`-`QR304` non-suppressible; other chart checks suppressible |
| `QR4xx` | Structure and text quality | Suppressible, including placeholder errors |
| `QR5xx` | CSS/HTML/theme/cover/TOC | `QR501` non-suppressible; other style advisories suppressible |

### Matching and filter layers

`rule_matches_token` accepts:

- exact code: `QR202`
- exact semantic name: `table-empty-column`
- category prefix: `QR2`

Selection and suppression are applied in this order:

1. `LintOptions.select`: when non-empty, only matching rules are considered.
2. Call-level `LintOptions.ignore`.
3. Document-level `ReportDocument.lint.ignore`.
4. Block-level `Block.lint_ignore`, only for diagnostics anchored to that block.

A matching ignore token suppresses a diagnostic only when its `suppressible` flag is `true`.
Core contract and safety rules survive every ignore layer and are never counted in `suppressed`.

### Persistence and configuration

- Migration `crates/quire-store/migrations/0002_document_lint_config.sql` adds
  `report_documents.lint JSONB NOT NULL DEFAULT '{"ignore":[]}'`.
- `PatchOp::SetLint { lint }` replaces the document policy through the normal revision, inverse-op
  and rollback pipeline.
- `BlockPatch.lint_ignore` replaces the block list; an empty list clears it. Block data stays inside
  the existing `report_blocks.payload` JSONB.
- `ReportDraft.lint` is copied into created or merged documents.

### MCP behavior

`LintParams` accepts optional `select` and `ignore` arrays. `lint_document` forwards them to
`lint_document_with_options`; document and block filters still apply. A receipt with only suppressed
diagnostics remains `ok: true` but includes `summary.suppressed` so callers can audit what was hidden.

The protocol guide does not enumerate rule codes. It states the writing constraints and the generic
verify loop: fix every `error`, review `warning`, and ignore only diagnostics whose payload says
`suppressible: true` when the rule genuinely does not apply.

## 3. Invariants & Verification

- `quire-lint` has no CLI and no I/O dependencies.
- Every `RuleMeta.code` and `.name` is unique; the registry stays code-sorted.
- `QR001`-`QR008`, `QR101`, `QR301`-`QR304` and `QR501` are never suppressible.
- Prefix, name and code matching produce the same rule set as the registry.
- `summary.suppressed` counts each suppressible diagnostic hidden once.
- Document and block ignore policies survive reload, revision updates and rollback.
- The full gate passes:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web && bun test && bun run typecheck && bun run lint && bun run build
```

- `web/e2e/lint-document-workflow.js` verifies multi-rule detection, diagnostic metadata,
  block-level suppression, non-suppressible guardrails, `select`/`ignore`, `ok: true` after fixes
  and browser rendering with zero console errors.
