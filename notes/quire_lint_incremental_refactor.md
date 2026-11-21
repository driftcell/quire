# QUIRE LINT INCREMENTAL HIGHER-EDUCATION REFACTOR

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-21
- **Promoted To:** `../../docs/REPORT_LINT_RULES.md`

## 1. Context & Problem

The report linter combines contract checks, evidence checks, visual layout checks and prose-quality
checks. A higher-education writing audit found duplicate diagnostics, repeated full-document scans,
false positives for standard official-document phrasing, and correctness gaps in table, citation,
heading and chart checks. These checks run synchronously in the document write path, so changes must
reduce noise without weakening validation at alternate construction and write entry points.

## 2. Rejected Options & Why

- **Delete every rule already checked by a dedicated MCP handler.** Rejected: public protocol patch
  operations, initial blocks, direct document construction and standalone lint calls can bypass those
  handlers. The lint rules remain the independent validation layer.
- **Move UUIDv4 rejection into `BlockId::parse` immediately.** Rejected: existing persisted documents
  may contain UUID block IDs accepted by the current protocol; stricter deserialization could make
  them unreadable without a migration.
- **Keep `QR405` as a compatibility alias or a rule that never emits.** Rejected: the registry should
  not accumulate dead rules or a second diagnostic vocabulary. `QR411` owns manual visual numbering;
  `QR405` is retired and its code is not reused.
- **Lint only touched blocks in the write path.** Rejected: citations, heading sequences, visual
  references, cross-chart colors and section density depend on document-wide state.
- **Treat any digit as evidence or exempt all conclusion paragraphs from citation checks.** Rejected:
  years and section numbers are not quantitative support, and conclusions must not introduce new
  unsupported figures.
- **Use floating-point sums for table totals.** Rejected: binary floating point introduces unstable
  equality at decimal rounding boundaries; decimal totals require fixed-point arithmetic.
- **Enable broad prose/visual number-drift warnings immediately.** Rejected: derived values, unit
  conversion, rounding and entity/column matching require a labelled corpus before diagnostics can be
  sufficiently precise.

## 3. Chosen Approach & Trade-offs

Refactor in independently verified batches: characterization tests and diagnostic deduplication;
citation-scan reuse; official-document prose alignment; numeric/table/reference correctness;
relative heading and per-axis chart algorithms; then three focused higher-education rules. Preserve
standalone lint behavior and stable diagnostic ownership, retire only proven redundant or impossible
checks in isolated changes, and defer speculative drift detection and broader execution-profile work.
Each batch requires positive and negative regression fixtures plus the workspace verification gate.
