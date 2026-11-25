use super::*;

#[tool_router(router = mcp_registry_router, vis = "pub(crate)")]
impl AdminMcpServer {
    /// List all registered external MCP servers with their endpoint URL, `enabled` switch, and masked auth token reference.
    ///
    /// Raw bearer tokens are never returned; `env:VAR` references are shown as-is and encrypted tokens are masked.
    #[tool(name = "mcp_list_servers")]
    pub async fn mcp_list_servers(
        &self,
        Parameters(_request): Parameters<EmptyParams>,
    ) -> Result<Json<McpServersReceipt>, ErrorData> {
        let servers = self
            .service
            .mcp()
            .list_servers()
            .await
            .map_err(admin_error)?;
        let next_hint =
            "Call mcp_sync_tools after registering a server, then inspect mcp_list_tools."
                .to_string();
        Ok(Json(McpServersReceipt { servers, next_hint }))
    }

    /// Register or update an external HTTP MCP server (`server_id`, `name`, `url`, optional `auth_token_ref`, `enabled`).
    ///
    /// Registering a server does NOT automatically discover its tools; you must call `mcp_sync_tools(server_id)` afterwards to connect and populate the tool catalog.
    /// When updating an existing server, omitting `auth_token_ref` preserves its stored credential reference.
    #[tool(name = "mcp_upsert_server")]
    pub async fn mcp_upsert_server(
        &self,
        Parameters(request): Parameters<McpServerInput>,
    ) -> Result<Json<McpServerReceipt>, ErrorData> {
        let server = self
            .service
            .mcp()
            .upsert_server(request)
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Server `{}` saved. Run mcp_sync_tools to discover its tool list.",
            server.server_id
        );
        Ok(Json(McpServerReceipt { server, next_hint }))
    }

    /// Connect to a registered MCP server, call `tools/list`, and upsert discovered tool descriptors into `catalog.mcp_tools`.
    ///
    /// Newly discovered tools default to `enabled: true`, while existing tools preserve their current `enabled` switch and refresh `description` and `input_schema`.
    #[tool(name = "mcp_sync_tools")]
    pub async fn mcp_sync_tools(
        &self,
        Parameters(request): Parameters<McpSyncParams>,
    ) -> Result<Json<McpSyncReceipt>, ErrorData> {
        let report = self
            .service
            .mcp()
            .sync_server_tools(&request.server_id)
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Discovered {} tools ({} new, {} updated). Use mcp_set_tool_enabled to disable unsafe tools.",
            report.discovered, report.created, report.updated
        );
        Ok(Json(McpSyncReceipt { report, next_hint }))
    }

    /// List discovered MCP tools from the catalog (optionally filtered by `server_id`) with their descriptions, JSON schemas, and global `enabled` switches.
    ///
    /// An agent can invoke an external MCP tool if and only if the triple intersection holds: the server is `enabled`, the tool is globally `enabled`, and the agent's `allowed_tools` matches `tool_name` or `server_id:tool_name`.
    #[tool(name = "mcp_list_tools")]
    pub async fn mcp_list_tools(
        &self,
        Parameters(request): Parameters<McpListToolsParams>,
    ) -> Result<Json<McpToolsReceipt>, ErrorData> {
        let tools = self
            .service
            .mcp()
            .list_tools(request.server_id.as_deref())
            .await
            .map_err(admin_error)?;
        let next_hint =
            "Call mcp_set_tool_enabled with the server_id/tool_name to change the global switch."
                .to_string();
        Ok(Json(McpToolsReceipt { tools, next_hint }))
    }

    /// Toggle the global `enabled` switch for one discovered MCP tool (`server_id` + `tool_name`) and publish cache invalidation.
    ///
    /// Disabling a tool globally blocks it across all agents immediately on their next run, even if listed in an agent's `allowed_tools`.
    #[tool(name = "mcp_set_tool_enabled")]
    pub async fn mcp_set_tool_enabled(
        &self,
        Parameters(request): Parameters<McpSetToolEnabledParams>,
    ) -> Result<Json<McpToolReceipt>, ErrorData> {
        let tool = self
            .service
            .mcp()
            .set_tool_enabled(&request.server_id, &request.tool_name, request.enabled)
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Tool `{}:{}` is now {}. The next agent run sees the updated global switch.",
            tool.server_id,
            tool.tool_name,
            if tool.enabled { "enabled" } else { "disabled" }
        );
        Ok(Json(McpToolReceipt { tool, next_hint }))
    }

    /// Delete a registered MCP server and cascade-delete all of its discovered `catalog.mcp_tools` entries, closing pooled connections across the runtime.
    #[tool(name = "mcp_delete_server")]
    pub async fn mcp_delete_server(
        &self,
        Parameters(request): Parameters<McpSyncParams>,
    ) -> Result<Json<DeleteReceipt>, ErrorData> {
        self.service
            .mcp()
            .delete_server(&request.server_id)
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Server `{}` deleted. Call mcp_list_servers to inspect remaining servers.",
            request.server_id
        );
        Ok(Json(DeleteReceipt {
            deleted: true,
            target: request.server_id,
            next_hint,
        }))
    }
}
