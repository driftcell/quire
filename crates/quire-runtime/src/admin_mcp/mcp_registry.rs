use super::*;

#[tool_router(router = mcp_registry_router, vis = "pub(crate)")]
impl AdminMcpServer {
    #[tool(
        name = "mcp_list_servers",
        description = quire_protocol::tool_description!("mcp_list_servers")
    )]
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

    #[tool(
        name = "mcp_upsert_server",
        description = quire_protocol::tool_description!("mcp_upsert_server")
    )]
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

    #[tool(
        name = "mcp_sync_tools",
        description = quire_protocol::tool_description!("mcp_sync_tools")
    )]
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

    #[tool(
        name = "mcp_list_tools",
        description = quire_protocol::tool_description!("mcp_list_tools")
    )]
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

    #[tool(
        name = "mcp_set_tool_enabled",
        description = quire_protocol::tool_description!("mcp_set_tool_enabled")
    )]
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

    #[tool(
        name = "mcp_delete_server",
        description = quire_protocol::tool_description!("mcp_delete_server")
    )]
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

    // ---------------------------------------------------------------------
    // Session observation
    // ---------------------------------------------------------------------
}
