# RUNTIME MCP REGISTRY

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-runtime`
- **Related Notes:** `../notes/runtime_mcp_registry.md`

## 1. Scope & Non-Goals

This document governs the global MCP server/tool registry, discovery, tool-level global enable
switches, secret masking, cache invalidation, and the intersection with agent tool allow-lists. It
does not define MCP wire protocol fields, Quire report tool semantics, or agent model selection.

## 2. Canonical Design & Contract

### Storage

`migrations/0006_mcp_registry.sql` adds:

- `mcp_servers(server_id PK, name, url, auth_token_ref, enabled, created_at, updated_at)`
- `mcp_tools(server_id FK ON DELETE CASCADE, tool_name, description, input_schema, enabled,
  updated_at, PRIMARY KEY(server_id, tool_name))`

`auth_token_ref` follows the same secret-ref rules as model providers: `env:VAR` references or
AES-256-GCM `enc:...` values encrypted with `RUNTIME_MASTER_KEY`. Public records expose
`auth_token_masked` only.

### Seed

`RuntimeService::seed` calls `McpRegistry::seed_default_quire_report` after KB seeding. It inserts:

- `server_id = "quire-report"`
- `url = Config.quire_base_url`
- `auth_token_ref = "env:QUIRE_RW_TOKEN"`
- `enabled = true`

The insert is `ON CONFLICT (server_id) DO NOTHING`, so a later administrator-disabled state is never
overwritten.

### Discovery and switches

`sync_server_tools(server_id)` resolves the server URL/token, calls MCP `tools/list`, and upserts
descriptions and input schemas. Existing tool rows keep their `enabled` value; newly discovered tools
default to `true`. Tool-level switches are changed through `set_tool_enabled`.

Every server upsert/patch/delete, sync, seed insertion and tool toggle invalidates the local moka
catalog and publishes `runtime:mcp:invalidate`. Other runtime instances subscribe and invalidate
their cache.

### Tool policy

The cached catalog contains enabled servers and every tool's global enable bit. The provider
selects a server only when:

- the server is enabled;
- the tool is globally enabled;
- the agent allow-list matches, supporting:
  - `"*"` for every tool on every server;
  - `"<server_id>:*"` for every tool on one server;
  - a plain tool name;
  - `"<server_id>:<tool_name>"` for an explicit server/tool pair.

Selected names are then used to build one `McpHttpClient` per server and listed through actual MCP
`tools/list`, so tools removed upstream are not callable even if a stale registry row remains.
Built-in runtime tools take precedence over remote tools with the same name.

### REST

All routes use `runtime_auth`:

- `GET /api/mcp/servers`
- `POST /api/mcp/servers`
- `PATCH /api/mcp/servers/{server_id}`
- `DELETE /api/mcp/servers/{server_id}`
- `POST /api/mcp/servers/{server_id}/sync`
- `GET /api/mcp/tools` (optional `server_id` query)
- `PATCH /api/mcp/servers/{server_id}/tools/{tool_name}` with `{enabled}`

### Admin MCP

`/admin/mcp` exposes the same service methods:

- `mcp_list_servers`
- `mcp_upsert_server`
- `mcp_sync_tools`
- `mcp_list_tools`
- `mcp_set_tool_enabled`

Every input uses `deny_unknown_fields`, every receipt carries `_next_hint`, and no receipt returns a
raw auth token reference.

## 3. Invariants & Verification

- Seeding is idempotent and never re-enables an administrator-disabled `quire-report`.
- Sync preserves existing tool enable switches; new tools are enabled.
- A globally disabled tool is filtered even when present in `agent_definition.allowed_tools`.
- Re-enabling a tool restores it on the next run.
- Auth token refs are encrypted/masked in HTTP and Admin MCP receipts.
- Server/tool mutations publish cache invalidation.

Integration coverage in `crates/quire-runtime/tests/runtime_integration.rs`:

- `seed_registers_quire_report_mcp_without_overwriting_admin_state`
- `mcp_tool_switch_filters_allow_listed_tools_and_invalidates_cache`
- `mcp_auth_token_is_masked_in_http_and_admin_receipts`
- `mcp_sync_discovers_tools_and_preserves_admin_switches`

Full gate:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
