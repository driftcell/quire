# Quire

**Quire** is a headless multi-agent report state engine and MCP server. AI agents author, cite,
merge, version, lint and review structured reports; a separate read-only frontend consumes the
canonical JSON snapshot and live SSE deltas.

This workspace deliberately contains **no** HTML/Tiptap compiler and **no** Playwright/Chromium
dependency. Rendering and visual preview belong to the external frontend.

## Crates

| Crate | Purpose |
| --- | --- |
| `quire-protocol` | Pure, zero-I/O Block AST, fractional ranks, patch engine, table layout compiler, citation resolver, Markdown projector, linter, draft parser |
| `quire-store` | PostgreSQL row-per-block persistence, Smart CAS, inverse-op revisions, Zstd snapshots, Redis Streams publisher |
| `quire-server` | Axum REST/SSE endpoints and the 15-tool `rmcp` server mounted at `/mcp` |

## Quickstart

```bash
cp .env.example .env
just up          # PostgreSQL 16 + Redis 7 via docker compose
just migrate     # SQLx migrations
just server      # quire-server on BIND_ADDR (default 0.0.0.0:8080)
```

The server also auto-runs embedded migrations on startup (`AUTO_MIGRATE=true` by default).
All `/mcp` and mutating admin calls require `Authorization: Bearer $QUIRE_RW_TOKEN`. Read-only
frontend endpoints and SSE streams accept either the RO or RW token. `X-Session-Id` drives session
discovery and `X-Actor` (or legacy `X-Agent-Id`) is recorded on every revision.

```bash
# MCP and mutating admin routes
curl -H "Authorization: Bearer $QUIRE_RW_TOKEN" \
     -H "X-Session-Id: sess_lead" -H "X-Actor: lead-agent" \
     http://127.0.0.1:8080/mcp

# Read-only frontend routes
curl -H "Authorization: Bearer $QUIRE_RO_TOKEN" \
     http://127.0.0.1:8080/api/docs
```

`just` loads `.env` automatically. Cargo itself does not load `.env`, but the SQLx 0.9 query macros
read the workspace `.env` for compile-time checked queries; normal `cargo test`/`cargo build`
therefore work after `cp .env.example .env` and `just up` (or use `just prepare` and `SQLX_OFFLINE=true`
for DB-free builds).

### Verification gates

```bash
just test-protocol   # pure protocol unit tests
just test-store      # unit + live PostgreSQL/Redis integration tests
just test            # workspace tests including the E2E multi-agent simulation
just clippy          # cargo clippy --workspace --all-targets -- -D warnings
just e2e             # explicit end-to-end run
```

The E2E test boots the real `quire-server` binary, starts two parallel MCP clients, observes SSE,
merges an offline draft, self-heals a dangling citation, rolls back and validates `computed`
frontend metadata.

## Environment

| Variable | Default | Meaning |
| --- | --- | --- |
| `DATABASE_URL` | — | PostgreSQL connection string |
| `REDIS_URL` | — | Redis connection string |
| `QUIRE_RW_TOKEN` | — | Bearer token for MCP and mutating admin routes |
| `QUIRE_RO_TOKEN` | — | Bearer token for read-only frontend routes (RW token also accepted) |
| `BIND_ADDR` | `0.0.0.0:8080` | HTTP bind address |
| `QUIRE_WORKSPACE_DIR` | current directory | Root for offline `draft.json` files |
| `QUIRE_MAX_DRAFT_BYTES` | `5242880` | Draft read limit |
| `MCP_ALLOWED_HOSTS` | loopback defaults | Comma-separated Host allowlist for `/mcp` |
| `CORS_ALLOW_ORIGINS` | permissive | Comma-separated browser origins |
| `SSE_HEARTBEAT_SECS` | `15` | SSE heartbeat interval |
| `AUTO_MIGRATE` | `true` | Run embedded SQLx migrations at boot |
| `MCP_JSON_RESPONSE` | `true` | Prefer JSON responses for simple MCP request/response calls |

## Agent workflow

```
INIT -> SCHEMA -> AUTHOR -> VERIFY
```

1. Send `X-Session-Id` on MCP calls. `GET /api/sessions/{session_id}/events` receives
   `event: doc_created` as soon as `create_report` commits.
2. Read `get_document_digest`, `read_as_markdown` or `read_blocks` before editing.
3. Mutate with `apply_operations(doc_id, expected_revision, summary, ops)` or `apply_draft`.
   Parallel writers using disjoint block IDs are automatically rebased by Smart CAS; overlapping
   writes receive a structured conflict with the latest blocks.
4. Call `lint_document` until errors are zero, then `read_as_markdown` for review.

Every tool receipt includes `_next_hint`. Every error carries a structured `hint` with valid block
IDs, source IDs, enums or exact CSS line/column locations. Apply receipts are deliberately compact
(roughly 100 tokens): `{new_revision, assigned_ids, inline_warnings, _next_hint}`. Inspect the
document or digest only when the receipt does not answer the question.

`apply_operations` accepts an optional `sources` map that is upserted in the same transaction as the
blocks, so a citation can be registered in the same call that references it. Empty or colliding
block IDs are auto-assigned and reported in `assigned_ids`; clients do not need to invent stable IDs
for transient drafts.

## MCP tools (15)

| Tool | Purpose |
| --- | --- |
| `get_protocol_guide` | progressive quickstart/tables/citations/parallel-workflow guidance |
| `create_report` | create revision 1; idempotency key + `X-Session-Id` discovery |
| `list_reports` | paginated/filterable report index |
| `apply_draft` | offline draft ingest with block-ID prefixing and source deduplication |
| `get_document_digest` | low-token outline + current revision |
| `read_as_markdown` | full/section/block Markdown projection |
| `read_blocks` | full protocol JSON for targeted blocks |
| `apply_operations` | atomic Smart CAS batch patch |
| `rollback_revision` | replay inverse ops into a new monotonic revision |
| `upsert_sources` | URL/normalized-title deduplicated citation registry |
| `set_cover` | partial cover update |
| `configure_toc` | enable/title/depth/variant |
| `configure_references` | bibliography style/page break + orphan sources |
| `update_style_and_css` | presets, tokens, validated scoped CSS |
| `lint_document` | zero-browser static validation |

## HTTP API

| Method | Path | Description |
| --- | --- | --- |
| GET | `/health` | PostgreSQL + Redis health |
| GET | `/api/docs?query=&limit=&offset=&page=&page_size=` | report list |
| GET | `/api/docs/{doc_id}` | canonical `ReportDocument` + `computed` |
| GET | `/api/docs/{doc_id}/events?sinceRev=N` | replay + live SSE revisions/previews/presence |
| GET | `/api/docs/{doc_id}/revisions?limit=N` | audit history |
| POST | `/api/docs/{doc_id}/rollback` | admin rollback `{target_revision, actor?}` |
| POST | `/api/docs/{doc_id}/ephemeral` | transient preview/presence pub/sub |
| GET | `/api/sessions/{session_id}/events` | session discovery SSE |
| POST | `/mcp` | Streamable HTTP MCP transport |

SSE event names: `revision`, `ephemeral_preview`, `agent_presence`, `doc_created`, `sync_required`,
plus `: heartbeat` comments.

### Canonical document response

`GET /api/docs/{doc_id}` returns the stored `ReportDocument` flattened with:

```json
{
  "current_rev": 7,
  "computed": {
    "table_layouts": {
      "financials": {
        "header_rows": [{"key":"revenue","label":"Revenue","colspan":2,"rowspan":1}],
        "leaf_columns": [{"key":"q1","width_pct":38.0,"width_css":"38%"}],
        "header_depth": 2,
        "total_width_pct": 100.0
      }
    },
    "citation_map": {"sec10": {"number": 1, "label": "[1]", "title": "Annual filing"}},
    "bibliography": [],
    "toc_tree": [{"block_id":"intro","level":1,"text":"Introduction","number":"1","children":[]}]
  }
}
```

Table spans and widths are computed server-side from recursive `ColumnNode` trees. Widths use a
P90 visual-width heuristic (`CJK = 2.0`, ASCII = 1.0), `fit`/`grow` weights and
`style.column_widths` overrides, normalized to exactly 100%.

## Protocol highlights

- `ReportDocument`: `doc_id`, optional `session_id`, `revision`, cover/TOC/theme/references,
  ordered `sources` and `blocks`.
- Blocks: heading, paragraph, quote, table, ECharts, custom HTML; all share `id`, `class_name`,
  `style`, `source_ids`, `source_note`.
- Inline Markdown: `**bold**`, `*italic*`, `` `code` ``, links, and `[^source_id]` citations.
- Tables accept a compact shorthand (`{"headers":["Region","Revenue"],"rows":[["EMEA","$900"]]}`)
  with inferred column keys and data types, or the full recursive `ColumnNode` tree.
- Table/ECharts/CustomHtml blocks share outer `title`, `numbering` and `note` fields. The server
  computes `表 N` / `图 N` labels and source attribution in `computed.block_framing`; titles must
  never contain manual numbering.
- ECharts inner `title` is stripped with an `ECHARTS_INNER_TITLE_STRIPPED` warning unless it is a
  center-positioned KPI; interactive `toolbox` chrome is stripped from static exports.
  `lint_document` flags `MANUAL_PREFIX_IN_TITLE`, `DUPLICATE_INNER_TITLE`, JS callback strings,
  invalid series types/indices, missing matrix coordinates and empty data sources.
- ECharts 6.0 `option` mode supports `dataset`, multi-series Cartesian charts, `matrix`, `radar`,
  `sankey`/`graph`/`chord`, `beeswarm`, `treemap`/`sunburst`/`tree`, and extension series such as
  `wordCloud`, `liquidFill` and `bar3D`. `computed.chart_requirements` lists required map names and
  extension registrations per chart block.
- Fractional base-62 ranks use PostgreSQL `COLLATE "C"` ordering; inserting/moving touches one block row.
- Revisions store both `forward_ops` and exact `inverse_ops`. Rollback replays inverse ops in reverse
  order into a new revision; history is never rewritten.
- Mutation receipts expose `base_revision`, `rebased`, `rebased_over`, `committed`, and writer-scoped
  `blocking_errors`, so parallel agents can prove a Smart CAS rebase and distinguish committed
  writes from lint-clean writes. `apply_draft` accepts optional `expected_revision`.
- Redis `XADD` happens after commit while holding a per-document publication lock, so Stream entries
  arrive in exact revision order.
- The canonical JSON Schema is generated from the Rust types:

  ```bash
  cargo run -p quire-protocol --example export_schema
  ```

  The example writes `contracts/report-protocol.schema.json` directly and includes the
  `RevisionDelta`, `SessionDelta` and `SyncRequiredDelta` SSE payloads.

## Persistence

Tables: `report_documents`, `report_blocks`, `report_sources`, `report_revisions`,
`report_snapshots`. Snapshots are Zstd-compressed every 50 revisions. Source dedup uses a normalized
key (URL first, normalized title otherwise).

## Non-goals

- Frontend rendering (HTML/React/Tiptap/Paged.js).
- Headless browser screenshots/visual preview.
- Human interactive editing, cursors or comment workflows.

The external frontend is a pure read-only consumer of canonical JSON and SSE deltas.
