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
| `quire-session-protocol` | Pure session/SSE contract types and `contracts/session-protocol.schema.json` |
| `quire-server` | Axum REST/SSE endpoints and the 19-tool `rmcp` server mounted at `/mcp` |
| `quire-runtime` | Independent session backend, rig.rs agent runtime, KB/skill management, recovery and Session SSE |

## Quickstart

```bash
cp .env.example .env
just up          # PostgreSQL 16 + Redis 7 via docker compose
just migrate     # SQLx migrations
just server      # quire-server on BIND_ADDR (default 0.0.0.0:8080)
just runtime     # quire-runtime on 127.0.0.1:8081 (auto-runs its own migrations)
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
just test            # workspace tests; recreates report_core_test from DATABASE_URL
just clippy          # cargo clippy --workspace --all-targets -- -D warnings
just e2e             # explicit end-to-end run
```

`just test` derives a disposable `report_core_test` database from `DATABASE_URL`. Running the
runtime integration tests directly with `cargo test` requires `TEST_DATABASE_URL`; they refuse to
run against `report_core` because they mutate runtime state.

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
| `RUNTIME_BIND_ADDR` | `127.0.0.1:8081` | Independent agent-runtime HTTP bind address |
| `RUNTIME_API_TOKEN` | — | Bearer token protecting runtime `/api/*` routes |
| `RUNTIME_MASTER_KEY` | — | Symmetric key for `enc:` provider credential refs |
| `QUIRE_BASE_URL` | `http://127.0.0.1:8080` | Quire server used by the runtime for MCP/document calls |
| `RUNTIME_MODEL` | `stub/echo` | Default model route recorded for runtime agents |
| `RUNTIME_MODEL_PROVIDER` | `mock` | `mock`, `openai_compatible` or `anthropic` |
| `MODEL_BASE_URL` | — | Optional provider base URL override |
| `MODELS_DEV_URL` | `https://models.dev/api.json` | Catalog source for model imports |
| `RUNTIME_SSE_REPLAY_LIMIT` | `500` | Maximum Session-SSE replay frames before `sync_required` |
| `RUNTIME_PREVIEW_THROTTLE_MS` | `65` | Tool-argument preview throttle (clamped to 50–80 ms) |
| `PREVIEW_BASE_URL` | `http://127.0.0.1:5173` | Frontend origin loaded by Playwright for `preview_report` |
| `PREVIEW_PUBLIC_URL` | `http://localhost:5173` | Preview origin returned to the user |
| `PLAYWRIGHT_MCP_URL` | — | Playwright MCP base URL; the runtime posts JSON-RPC to `<url>/mcp` |
| `PLAYWRIGHT_MCP_PORT` | `8931` | Host port published by the compose `playwright` service |
| `SANDBOX_PROVIDER` | `disabled` | Agent sandbox provider: `disabled`, `local` (test stub) or `fc` |
| `SANDBOX_FC_API_KEY` | — | FC cloud-sandbox data-plane E2B API key, required for `SANDBOX_PROVIDER=fc` |
| `SANDBOX_FC_REGION` | `cn-hangzhou` | FC region used to derive the API URL and sandbox domain |
| `SANDBOX_FC_TEMPLATE` | `code-interpreter-v1` | Default FC sandbox template |
| `SANDBOX_TIMEOUT_SECONDS` | `300` | Remote sandbox TTL; runtime cleanup still runs at the end of each agent run |
| `RUNTIME_MCP_ALLOWED_HOSTS` | loopback defaults | Comma-separated Host allowlist for `/admin/mcp` |

## Agent sandbox

The runtime exposes `sandbox_execute`, `sandbox_read_file` and `sandbox_write_file` as allow-listed
rig tools. Sandboxes are created lazily, reused within one agent run and destroyed at run completion;
the TTL is a cleanup backstop. `local` is an explicit stub for tests, while `disabled` never executes
on the host. See `docs/RUNTIME_SANDBOX.md` for the provider and lifecycle contract.

For Alibaba Cloud FC Agent Sandbox:

```bash
SANDBOX_PROVIDER=fc
SANDBOX_FC_API_KEY=<cloud-sandbox-e2b-api-key>
SANDBOX_FC_REGION=cn-hangzhou
SANDBOX_FC_TEMPLATE=code-interpreter-v1
SANDBOX_TIMEOUT_SECONDS=300
```

`SANDBOX_FC_API_KEY` is the cloud-sandbox data-plane key, not an Alibaba Cloud AccessKey ID/secret
pair. `SANDBOX_FC_API_URL` and `SANDBOX_FC_DOMAIN` can override the region-derived endpoints.

## Visual preview

`preview_report` drives the `playwright` compose service over Streamable HTTP
(`POST /mcp`). The service runs headless Chromium in a container, so the preview
route must be reachable from that container and the runtime must reach the
published MCP port.

```bash
docker compose up -d playwright
# .env
# PLAYWRIGHT_MCP_URL=http://127.0.0.1:8931
# PREVIEW_BASE_URL=http://host.docker.internal:5173
# PREVIEW_PUBLIC_URL=http://localhost:5173
```

Vite binds `0.0.0.0` in dev so `host.docker.internal:5173` resolves to the host.
On Linux, `host.docker.internal` is provided by the compose `extra_hosts` entry.

The agent-facing `preview_report` tool returns structured diagnostics and the
preview URL only; the base64 PNG is omitted so text-only models do not receive
an unreadable image. MCP visual clients can request the image with
`include_image: true`.

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

## MCP tools (19)

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
| `configure_page_chrome` | A4 header/footer slots, placeholders and first-page visibility |
| `update_style_and_css` | presets, tokens, validated scoped CSS |
| `lint_document` | zero-browser static validation |
| `list_comments` | side-channel review threads with anchors + `anchor_detached` |
| `add_comment` | agent review comment; never touches revisions or CAS |
| `reply_comment` | writer reply, optionally advancing `open` to `addressed` |

## HTTP API

| Method | Path | Description |
| --- | --- | --- |
| GET | `/health` | PostgreSQL + Redis health |
| GET | `/api/docs?query=&limit=&offset=&page=&page_size=` | report list |
| GET | `/api/docs/{doc_id}` | canonical `ReportDocument` + `computed` |
| GET | `/api/docs/{doc_id}/events?sinceRev=N` | replay + live SSE revisions/previews/presence |
| GET | `/api/docs/{doc_id}/revisions?limit=N` | audit history |
| GET | `/api/docs/{doc_id}/diff?fromRev=A&toRev=B` | structured block-level diff (`toRev` defaults to current) |
| GET | `/api/docs/{doc_id}/comments?status=&block_id=` | review threads (default open/addressed; `status=all` includes archived) |
| POST | `/api/docs/{doc_id}/comments` | create a human review thread |
| POST | `/api/docs/{doc_id}/comments/{thread_id}/replies` | append a human reply |
| PATCH | `/api/docs/{doc_id}/comments/{thread_id}` | human-only archive/reopen |
| POST | `/api/docs/{doc_id}/rollback` | admin rollback `{target_revision, actor?}` |
| POST | `/api/docs/{doc_id}/ephemeral` | transient preview/presence/clear pub/sub |
| GET | `/api/sessions/{session_id}/events` | session discovery SSE |
| POST | `/mcp` | Streamable HTTP MCP transport |

SSE event names: `revision`, `ephemeral_preview`, `agent_presence`, `ephemeral_clear`,
`comment_updated`, `doc_created`, `sync_required`, plus `: heartbeat` comments.

### Ephemeral typewriter previews

`POST /api/docs/{doc_id}/ephemeral` accepts the tagged union in
`contracts/report-protocol.schema.json` (`EphemeralPayload`). The server overwrites `doc_id` and
`received_at`, defaults `ttl_ms` to 15000, and rejects payloads above 64 KiB with HTTP 400.

```json
{
  "kind": "preview",
  "preview_id": "tool_call_7",
  "actor": "sub-agent-a",
  "anchor": {"type": "after_block", "block_id": "ch1_placeholder"},
  "status": "streaming",
  "seq": 12,
  "block": {
    "block_id": "ch1_preview",
    "block_type": "paragraph",
    "text": "Revenue grew 18% year over year"
  }
}
```

Presence uses `kind: "presence"` with `actor`, `status` and an optional `section_id`. When the
streamed block is committed, pass the preview IDs to `apply_operations` (or `apply_draft`) as
`preview_ids`; the emitted `revision` delta then carries `cleared_preview_ids` so clients remove the
ephemeral block and apply the committed revision in one frame without flicker.

If a commit resolves to a no-op, no revision is produced. The server then emits
`event: ephemeral_clear` with `{"kind":"clear","preview_ids":[...]}` so clients can remove the
streamed placeholders immediately instead of waiting for `ttl_ms` to expire.

## Agent runtime

`quire-runtime` is an independent backend; it talks to `quire-server` over MCP/HTTP and never links
the server or `rig` into the report engine. It owns:

- PostgreSQL tables for `agent_sessions`, messages, attachments, runs, `session_events`,
  `agent_tool_calls`, knowledge documents, agent definitions, skills and skill resources.
- `POST/GET/PATCH/DELETE /api/sessions`, message/attachment/retry/compact/interrupt endpoints and
  `GET /api/sessions/{id}/events?afterSeq=N` (PostgreSQL replay, then Redis live tail).
- `/api/kb/docs`, `/api/kb/agents` and `/api/kb/skills` CRUD with monotonic KB versions, idempotent
  filesystem seeding and moka cache invalidation over Redis.
- Git-style KB branches: `/api/kb/branches` create/list/diff/promote, `/api/kb/rollback`, immutable
  `kb_snapshots`, item-level three-way merge with structured conflicts, and session-level
  `kb_branch` selection for isolated test runs.
- `/admin/mcp` Streamable HTTP administration tools for branches, branch-scoped items, providers,
  models and session inspection. It uses the same `KbService` / `ModelRegistry` / `RuntimeService`
  methods as REST and is protected by `RUNTIME_API_TOKEN`; report-authoring sub-agents only see the
  Quire server's `/mcp` surface.
- Crash recovery: startup marks orphaned `running` runs `interrupted` and appends a recoverable
  `turn_failed` event.
- Frozen prompt-cache prefixes: resume injects `<knowledge_update>` before the new user message
  instead of rewriting the pinned system prompt.
- rig-core `DynamicTool` adapters over Quire MCP tools, built-in skill/task/subagent tools and a
  `SandboxProvider`/`Sandbox` abstraction with disabled/local providers and an optional Alibaba
  Cloud FC implementation selected through `SANDBOX_*` environment variables.
- `RigModelStreamProvider` is the default production model engine: it resolves branch-scoped agent
  definitions and model routes, drives rig-core streaming multi-turn tool loops, runs subagents
  concurrently with `JoinSet`, reports real token usage/context limits, persists full tool
  input/output, and streams partial block previews to Quire's ephemeral preview endpoint.
- `ask_user` cards suspend a run as `awaiting_input` without holding a connection. Pending cards are
  durable in `SessionDetail.pending_question`; answers/skips resume through
  `POST /api/sessions/{id}/questions/{question_id}/answer`, while a normal new message dismisses the
  card and starts a fresh turn.
- Global MCP registry (`mcp_servers` / `mcp_tools`) with idempotent `quire-report` seeding, tool
  discovery/sync, per-tool global kill switches, masked auth refs and moka + Redis invalidation.
  Agent tool selection is the triple intersection of enabled server, enabled tool and allow-list.

Session contracts and SSE envelopes are exported to `contracts/session-protocol.schema.json`.

Model routes are managed by `llm_providers` / `llm_models` and exposed under `/api/models`. Provider
credentials are stored only as `env:VAR` references or AES-256-GCM `enc:` values derived from
`RUNTIME_MASTER_KEY`; GET responses expose masks only. `/api/models/import-models-dev` accepts either
an offline `models.dev` `api.json` payload or fetches `MODELS_DEV_URL`, and `ModelRouter` builds
rig-core clients for `openai_compatible` or `anthropic` protocols with the registered base URL,
context limit and tool/reasoning capabilities.

### Review comments
Comments are side-channel data: they never enter `report_blocks`, never increment
`report_documents.current_rev`, and never participate in Smart CAS. `block_id` is a soft `TEXT`
reference plus a truncated `anchor_snapshot` and nearest-heading `section_id`; deleting the block
leaves the thread intact with `anchor_detached: true`. Humans create/reply/archive through REST;
agents use `add_comment`/`reply_comment` and may only advance `open` to `addressed` with
`addressed_rev`. SSE `comment_updated` frames carry `thread_id`, `status`, `anchor_detached` and
`updated_at`.

### Version diff and computed chart tables

`GET /api/docs/{doc_id}/diff?fromRev=A&toRev=B` rebuilds both revisions from snapshots and
forward/inverse ops, then returns `BlockDiffEntry` values (`added`/`removed`/`modified`) plus
document metadata flags. `computed.chart_tables` exposes renderer-agnostic
`ChartDataTable { title, headers, rows }` entries for dataset, cartesian, relation, radar and
hierarchy charts; Markdown projection uses the same extraction function. `SourceEntry.excerpt`
stores a verbatim evidence quote and is returned in sources and bibliography entries.

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
`report_snapshots`, `report_comment_threads`, `report_comment_messages`. Snapshots are Zstd-compressed every 50 revisions. Source dedup uses a normalized
key (URL first, normalized title otherwise).

## Non-goals

- Frontend rendering (HTML/React/Tiptap/Paged.js).
- Headless browser screenshots/visual preview.
- Human interactive editing, cursors or comment workflows.

The external frontend is a pure read-only consumer of canonical JSON and SSE deltas.
