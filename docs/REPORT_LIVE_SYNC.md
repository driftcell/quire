# REPORT LIVE SYNC

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-protocol`, `quire-server`
- **Related Notes:** `../notes/report_live_sync_performance.md`

## 1. Scope & Non-Goals

This document governs live document synchronization for the read-only session workspace: revision
SSE coordination, the snapshot-delta REST contract, full-snapshot recovery, frontend structural
sharing and the React boundaries that keep session/preview traffic from rerendering unchanged blocks.

Session conversation projection remains governed by `RUNTIME_SESSION_EVENTS.md`. A4 pagination,
virtualization and agent patch semantics are outside this document. The canonical patch engine stays
in `quire-protocol`; the frontend does not reimplement `PatchOp` or computed projections.

## 2. Canonical Design & Contract

### 2.1 Snapshot and applied-delta APIs

`GET /api/docs/{doc_id}` is the initial-load and recovery API. It returns the complete canonical
document and `compute_document_meta` output. `GET /api/docs/{doc_id}?rev=N` returns the same contract
reconstructed at committed revision N (revision 0 and future revisions are rejected). Preview pages
pass the revision they intend to inspect; a live document must never be presented as evidence for an
older revision.

`GET /api/docs/{doc_id}/sync?fromRev=N` loads the current target snapshot, reconstructs the requested
base when it differs, and returns `DocumentSyncDelta`. A `fromRev` ahead of the target is rejected.
The generated contract lives in `contracts/report-protocol.schema.json` and
`web/src/contracts/report-protocol.ts`.

| Field                                                    | Contract                                                                                          |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `from_revision` / `to_revision`                          | Exact base and target revisions; a client never applies the delta to another base.                |
| `block_ids`                                              | Complete target block order, including unchanged blocks and all reorder operations.               |
| `upserted_blocks`                                        | Only added or modified blocks in their already-applied form. Empty lists may be omitted.          |
| `removed_block_ids`                                      | IDs absent from the target. Exact target order remains authoritative. Empty lists may be omitted. |
| `cover`, `toc`, `theme`, `references`, `lint`, `sources` | Present only when the corresponding complete value changed.                                       |
| `computed`                                               | Canonical Rust `compute_document_meta` output for the target snapshot.                            |

Computed metadata is a complete projection in each sync response; it is not an independent incremental
projection protocol. This avoids a second TypeScript implementation of citations, table layouts,
TOC and chart metadata.

### 2.2 Revision stream coordination

The document SSE connection starts as soon as a document ID exists so early ephemeral previews are
not lost. Historical revision frames do not trigger synchronization until the initial document query
is ready. Once the snapshot is ready, a higher observed revision triggers one catch-up sync; historical
frames at or below the cached revision do not refetch anything.

Revision bursts are batched for 32ms. `LatestRevisionSync` permits one active request and retains the
highest requested revision. Frames arriving during that request join the same drain loop and cause at
most one trailing sync. A sync response may advance past the requested target when more writes commit
while it is being produced.

A stale base, malformed delta, pruned history, failed incremental request or `sync_required` uses the
full-document recovery path. Failed full recovery is surfaced through the existing React Query document
state. Full recovery can run at the same revision, and no partial delta is ever applied to the wrong
base.

### 2.3 Frontend identity and render boundaries

`applyDocumentSync` validates document identity, base/target revisions, block existence, duplicate IDs
and unreferenced upserts. It applies the exact target order and reuses every unchanged block. If only
metadata changed, it also retains the original block array. A JSON structural-sharing pass retains
unchanged computed branches, including citation maps, bibliography entries and per-block layouts.

`ReportBlockItem` receives the citation map plus only its own framing, table layout and chart
projections instead of the complete computed object. Continuous-mode block slices are memoized
separately from ephemeral preview mixing, so preview updates retain committed slice identities.
Session-scoped callbacks are stable and `SessionReportPane` is memoized, preventing chat-only state
updates from rerendering the report.

## 3. Invariants & Verification

- A revision-pinned snapshot matches the state that `document_at_revision` reconstructs; an
  incomplete forward or inverse history fails with `CORRUPT_REVISION` instead of returning a
  partial document.
- Preview receipts identify the rendered document and revision, and a requested revision that does
  not match the rendered one is an error.
- Initial snapshot loading does not turn historical SSE replay into full-document or sync requests.
- A revision burst has one active sync and a bounded trailing latest-revision request; no revision is
  silently lost.
- Unchanged block and computed identities survive synchronization; changed identities are limited to
  their actual JSON branches.
- Exact target order, removals, metadata replacements and canonical computed output match the full
  snapshot for the same target revision.
- Invalid base/document/block data fails closed and recovers from the canonical full snapshot.
- Chat-only updates do not invalidate report props; preview-only updates preserve committed block
  slices.
- Protocol tests cover applied changes and empty deltas. The server end-to-end test compares sync and
  full-snapshot contracts. Frontend tests cover structural sharing, stale/missing data rejection,
  single-flight trailing behavior and same-revision forced recovery. Playwright verification covers
  initial load, real MCP revision bursts, DOM convergence and long-report ephemeral previews.

Full gate:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
just schema
bun run --cwd web gen:types
bun test --cwd web
bun run --cwd web typecheck
bun run --cwd web lint
bun run --cwd web build
```
