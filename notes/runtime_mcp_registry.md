# RUNTIME MCP REGISTRY

## Metadata

- **Status:** Accepted
- **Date:** 2026-11-18
- **Promoted To:** `../docs/RUNTIME_MCP_REGISTRY.md`

## 1. Context & Problem

The runtime previously discovered one Quire `/mcp` server from `Config`. Production needs a global
registry of MCP servers, tool discovery, per-tool global kill switches, and a single policy shared
by REST, `/admin/mcp` and the rig tool loop. Server auth references must never be returned raw.

## 2. Rejected Options & Why

- **Configure MCP endpoints only through environment variables.** Rejected because tools and
  unsafe subsets change at runtime and must be toggled without restarting the process.
- **Per-agent copies of server/tool settings.** Rejected because a security-critical tool kill
  switch must be global. Agent allow-lists remain the per-agent layer, intersected with global
  switches.
- **Trust newly discovered tools immediately and overwrite stored switches.** Rejected because a
  server update must not silently re-enable a tool an administrator disabled.
- **Store raw auth tokens in `mcp_servers`.** Rejected; the registry reuses `env:VAR` / `enc:...`
  references and returns masks only.
- **Re-query MCP tables on every model turn.** Rejected; enabled server/tool state is cached in moka
  and every admin mutation publishes a Redis invalidation.

## 3. Chosen Approach & Trade-offs

`McpRegistry` owns `mcp_servers`, `mcp_tools`, discovery, switch updates and the cached catalog.
`RuntimeService::seed` idempotently inserts the default `quire-report` server without overwriting
admin changes. `sync_server_tools` upserts descriptions/schemas while leaving existing `enabled`
values untouched; new tools default enabled.

Tool assembly is the intersection of enabled server, enabled tool and agent allow-list, with `*`,
`<server_id>:*`, plain tool names and explicit `<server_id>:<tool>` patterns. The accepted cost is
that stale tools removed upstream remain listed until a future cleanup action, though they cannot be
called because actual `/mcp` discovery still filters the registry selection.

Follow-up (2026-11-20): allow-list matching was unified on `tool_pattern_matches`, adding prefix,
suffix and multi-segment wildcards (`subjectcloud_*`, `*_detail`, `server_a:shuangyou_*`) across the
registry, Rig MCP client and built-in tool loop. Missing-tool warnings now expand wildcards against
the assembled tool map instead of looking up the pattern literally.
