use rmcp::model::CallToolResult;

use super::*;

fn preview_bridge(
    server: &QuireMcpServer,
) -> Result<std::sync::Arc<dyn crate::state::ExternalHarnessBridge>, ErrorData> {
    server
        .service
        .state()
        .harness_bridge
        .clone()
        .ok_or_else(|| {
            invalid_params(
                "preview tools require the unified quire-runtime harness",
                "Use the unified runtime /mcp endpoint instead of a standalone report store.",
                None,
            )
        })
}

#[tool_router(router = preview_router, vis = "pub(crate)")]
impl QuireMcpServer {
    /// Render a committed report in the controlled headless browser and return A4 pagination,
    /// overflow/CSS diagnostics and an optional PNG screenshot.
    ///
    /// Use `section_id`, `block_ids` or `page` for focused inspection. `include_image` defaults to false;
    /// set it only for visual clients. The caller must have read access to `doc_id`.
    #[tool(name = "preview_report", annotations(read_only_hint = true))]
    pub async fn preview_report(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<PreviewReportInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let bridge = preview_bridge(self)?;
        let context = request_context(&context)?;
        let result = bridge
            .preview_report(self.service.clone(), context, request)
            .await?;
        result.into_mcp_result()
    }

    /// Validate and render a native ECharts 6.0 option before committing it. The first call requires
    /// `option`; refinements use `based_on_preview_id` plus `option_patch`, never a repeated option.
    ///
    /// The transient preview is bound to the authenticated user and session for 30 minutes. Returns
    /// lint diagnostics, a preview URL and an optional screenshot (`include_image: true`).
    #[tool(name = "preview_echarts")]
    pub async fn preview_echarts(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<PreviewEchartsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let bridge = preview_bridge(self)?;
        let context = request_context(&context)?;
        let result = bridge
            .preview_echarts(self.service.clone(), context, request)
            .await?;
        result.into_mcp_result()
    }

    /// Commit a chart previously created by `preview_echarts` without re-sending its option. Supports
    /// in-place ECharts updates, before/after/section anchors, Smart CAS and durable chat scope.
    ///
    /// The caller must own the preview and have access to the target document or session. Expired or
    /// foreign preview IDs are rejected with a recovery hint to create a fresh preview.
    #[tool(name = "commit_chart")]
    pub async fn commit_chart(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<CommitChartParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let bridge = preview_bridge(self)?;
        let context = request_context(&context)?;
        let result = bridge
            .commit_chart(self.service.clone(), context, request)
            .await?;
        result.into_mcp_result()
    }
}
