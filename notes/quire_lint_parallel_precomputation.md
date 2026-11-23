# QUIRE LINT PRECOMPUTATION AND DETERMINISTIC PARALLELISM

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-23
- **Promoted To:** `../docs/REPORT_LINT_RULES.md`

## 1. Context & Problem

The lint rules repeatedly mask the same prose, validate the same table shapes, and deep-merge the
same effective ECharts options. A single shared `&mut Engine` also prevents independent rule passes
from running in parallel. Long reports amplify these repeated allocations, while small reports can
be slower if every check pays thread-scheduling overhead.

The implementation must preserve every diagnostic and its final ordering. It must also remain a
pure synchronous crate: no Tokio runtime assumptions, I/O, or global mutable state.

## 2. Rejected Options & Why

- **Add `par_iter()` independently inside every rule:** rejected because it preserves duplicate
  precomputation and creates many fork/join barriers for small blocks of work.
- **Share a mutable collector behind a mutex:** rejected because lock ordering and contention make
  the rules harder to reason about and needlessly couple otherwise independent passes.
- **Always use Rayon:** rejected because small documents may not amortize scheduling overhead.
  Serial execution remains the small-document path and the semantic reference in tests.
- **Cache by block ID in global state:** rejected because IDs are not stable identity across document
  revisions, global caches create invalidation and concurrency risks, and a per-call view is enough.

## 3. Chosen Approach & Trade-offs

Build a per-call `LintContext` containing the citation scan and block-indexed precomputed views.
Each rule pass is a function from `&LintContext` to `Vec<Finding>`; no collector is shared between
passes. Large documents use Rayon for context construction and rule passes, while small documents
use serial iterators. The existing report finish step remains the sole filtering, suppression,
counting, and total-ordering boundary.

The trade-off is additional memory for one masked string per paragraph and one table shape per
table/effective option per chart. Stripped markup is used immediately to derive wall measurements
rather than retained. These values are bounded by the document and replace repeated transient
allocations. Parallel and serial reports must compare exactly in tests, including diagnostics and
suppression counts.
