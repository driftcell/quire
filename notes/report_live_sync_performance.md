# REPORT LIVE SYNC PERFORMANCE

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-22
- **Promoted To:** `../docs/REPORT_LIVE_SYNC.md`

## 1. Context & Problem

Long reports currently refetch the complete `GET /api/docs/{doc_id}` response for every revision SSE
frame. Initial subscription can start at revision zero, so historical replay also invalidates the
document query many times. Session and ephemeral-preview updates separately invalidate React memo
boundaries, causing unchanged blocks and charts to render while the agent is writing.

The fix must preserve the canonical Rust patch and computed-projection semantics, keep revision gap
recovery correct, retain unchanged block identities for React, and avoid disrupting an active runtime
during development and verification.

## 2. Rejected Options & Why

- **Debounce full-document refetches only.** This reduces request count but leaves O(document size)
  transfer, JSON parsing, computed replacement and full-report rendering on every write burst.
- **Reimplement `PatchOp` in TypeScript and apply SSE `ops` locally.** Effective operation output has
  server-assigned IDs and detailed table, section, metadata and option-merge behavior. A second patch
  engine would create a permanent correctness drift risk. It would also still need a TypeScript port
  of citation, table-layout, TOC and chart projections.
- **Embed complete computed metadata in every revision SSE frame and Redis revision record.** This
  would multiply hot-stream storage and replay payloads, including for clients that do not need the
  presentation projection.
- **Virtualize the report before fixing data flow.** Virtualization can reduce DOM cost later, but it
  does not remove redundant server work, network traffic or eager ECharts updates, and it conflicts
  with exact A4 measurement when applied indiscriminately.

## 3. Chosen Approach & Trade-offs

Use a read-only `GET /api/docs/{doc_id}/sync?fromRev=N` endpoint backed by canonical document
snapshots. Its protocol response contains exact target block order, added/modified blocks, removed
IDs, only changed document configuration, and the latest Rust-computed presentation metadata. The
frontend applies this snapshot delta with structural sharing; unchanged blocks retain their object
identity. Full-document GET remains the initial load and the fallback for stale, failed or gapped
incremental synchronization.

The document stream starts only after the initial revision is known. Revision bursts use one
single-flight sync with a trailing highest-revision request. Stable callbacks and render boundaries
keep chat state out of unchanged report blocks, and continuous preview mixing reuses committed slice
objects when the document itself has not changed.

The trade-off is that computed metadata remains a complete projection in each sync response and the
server still computes it from the target snapshot. This avoids semantic drift and substantially
reduces block transfer and client rendering, but it is not an incremental computed-projection engine.
