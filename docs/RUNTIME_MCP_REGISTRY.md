# RUNTIME MCP REGISTRY

## Metadata

- **Status:** Settled
- **Owner Crates:** `quire-runtime`, `quire-server`
- **Related Notes:** `../notes/runtime_mcp_registry.md`, `../notes/runtime_rig_046_native_migration.md`

## 1. Scope & Non-Goals

This document governs the global MCP server/tool registry, discovery, tool-level global enable
switches, secret masking, cache invalidation, and the intersection with agent tool allow-lists. It
does not define MCP wire protocol fields, Quire report tool semantics, or agent model selection.

## 2. Canonical Design & Contract

### Storage

The consolidated `migrations/0001_init.sql` adds:

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
catalog and shared-connection caches and publishes `runtime:mcp:invalidate`. Other runtime instances
invalidate both caches, so changed credentials/endpoints never keep using an old connection.

### Tool policy

The cached catalog contains enabled servers and every tool's global enable bit. The provider
selects a server only when:

- the server is enabled;
- the tool is globally enabled;
- the agent allow-list matches through the single `tool_pattern_matches` implementation shared by
  the registry, the Rig MCP client and the built-in tool loop:
  - `"*"` for every tool on every server;
  - a plain tool name or `"<server_id>:<tool_name>"` for an exact match;
  - `"<server_id>:*"` for every tool on one server;
  - prefix, suffix and multi-segment wildcards such as `subjectcloud_*`, `*_detail` and
    `panorama_report_*_query`. A pattern containing `:` is matched against
    `server_id:tool_name`; otherwise it is matched against the bare tool name.

Selected names are served from a shared `RmcpClient` cached by `server_id`. The first use performs
MCP `initialize` and `tools/list`; subsequent main-agent and subagent runs reuse the live
`McpConnection` and its discovered `McpTool` values. A closed connection is reconnected under the
registry connection lock. Built-in runtime tools take precedence over remote tools with the same
name. Missing-tool warnings expand wildcard patterns against the actual assembled native/dynamic
tool-name set.

### Shared connections and per-call identity

Model/tool runs use `RmcpClient::connect_shared`, which configures only the endpoint and static auth;
it does not put a particular session or agent on connection-level HTTP headers. Every AgentRunner
`ToolContext` carries `McpMeta` with:

```json
{
  "quire": {
    "session_id": "sess_...",
    "run_id": "run_...",
    "agent_id": "main"
  }
}
```

`rig-rmcp` forwards this object as the MCP call's `_meta`. `quire-server` injects the rmcp
`RequestContext<RoleServer>`, reads HTTP `Parts` from its extensions and the per-call
`RequestMetaObject` from `RequestContext.meta`. Header and metadata identities must agree when both
are present; session, actor and run IDs use the same format validation as header identities.
`create_report` requires a resolved session identity. This allows one authenticated connection pool
entry to serve many sessions without trusting model-authored identity fields: metadata is inserted
by the host runtime, not read from model arguments.

Administrative discovery and explicit operational helpers may still create dedicated clients when
they intentionally use a fixed header identity; the normal model tool loop uses the shared pool.

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
- Allow-list matching is identical across the registry, MCP client and built-in tool loop; scoped
  wildcards never cross server boundaries.
- Valid wildcard patterns do not produce missing-tool warnings.
- Re-enabling a tool restores it on the next run.
- Auth token refs are encrypted/masked in HTTP and Admin MCP receipts.
- Server/tool mutations publish cache invalidation and retire pooled connections.
- Main and subagent runs sharing a server do not repeat `initialize` / `tools/list` per run.
- Shared connections carry no fixed session identity; each call forwards host-authored `_meta.quire`.
- Header/metadata mismatches are rejected and report creation cannot proceed without a session.

Unit coverage in `crates/quire-runtime/src/mcp_registry.rs` and `rig_agent/builtin_tools.rs`:

- `tool_patterns_support_prefix_suffix_and_multi_segment_wildcards`
- `scoped_wildcards_never_cross_server_boundaries`
- `wildcard_allow_list_supports_prefix_suffix_and_multi_segment_patterns`
- `warn_missing_allowed_tools_expands_wildcards_instead_of_literal_lookup`

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
