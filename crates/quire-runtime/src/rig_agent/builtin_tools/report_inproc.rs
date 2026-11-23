use super::*;

macro_rules! define_stateless_report_tool {
    ($struct_name:ident, $tool_name:literal, $params_ty:ty, $method:ident) => {
        pub(crate) struct $struct_name;

        impl Tool for $struct_name {
            const NAME: &'static str = $tool_name;
            type Error = ToolExecutionError;
            type Args = $params_ty;
            type Output = Value;

            fn description(&self) -> String {
                quire_server::mcp::QuireMcpServer::tool_metadata($tool_name).0
            }

            fn parameters(&self) -> Value {
                quire_server::mcp::QuireMcpServer::tool_metadata($tool_name).1
            }

            async fn call(
                &self,
                context: &mut ToolContext,
                args: Self::Args,
            ) -> Result<Self::Output, Self::Error> {
                let runtime = runtime_tool_scope(context)?;
                let rmcp::handler::server::wrapper::Json(receipt) = runtime
                    .report_mcp
                    .$method(rmcp::handler::server::wrapper::Parameters(args))
                    .await
                    .map_err(mcp_error_to_tool_error)?;
                serde_json::to_value(receipt).map_err(tool_other)
            }
        }
    };
}

macro_rules! define_contextual_report_tool {
    ($struct_name:ident, $tool_name:literal, $params_ty:ty, $method:ident) => {
        pub(crate) struct $struct_name;

        impl Tool for $struct_name {
            const NAME: &'static str = $tool_name;
            type Error = ToolExecutionError;
            type Args = $params_ty;
            type Output = Value;

            fn description(&self) -> String {
                quire_server::mcp::QuireMcpServer::tool_metadata($tool_name).0
            }

            fn parameters(&self) -> Value {
                quire_server::mcp::QuireMcpServer::tool_metadata($tool_name).1
            }

            async fn call(
                &self,
                context: &mut ToolContext,
                args: Self::Args,
            ) -> Result<Self::Output, Self::Error> {
                let runtime = runtime_tool_scope(context)?;
                let req_ctx = in_process_request_context(context)?;
                let rmcp::handler::server::wrapper::Json(receipt) = runtime
                    .report_mcp
                    .$method(req_ctx, args)
                    .await
                    .map_err(mcp_error_to_tool_error)?;
                serde_json::to_value(receipt).map_err(tool_other)
            }
        }
    };
}

define_stateless_report_tool!(
    GetProtocolGuideTool,
    "get_protocol_guide",
    quire_server::mcp::GuideParams,
    get_protocol_guide
);
define_contextual_report_tool!(
    CreateReportTool,
    "create_report",
    quire_server::mcp::CreateReportParams,
    exec_create_report
);
define_stateless_report_tool!(
    ListReportsTool,
    "list_reports",
    quire_server::mcp::ListReportsParams,
    list_reports
);
define_stateless_report_tool!(
    GetDocumentDigestTool,
    "get_document_digest",
    quire_server::mcp::DigestParams,
    get_document_digest
);
define_stateless_report_tool!(
    ReadAsMarkdownTool,
    "read_as_markdown",
    quire_server::mcp::ReadMarkdownParams,
    read_as_markdown
);
define_stateless_report_tool!(
    ReadBlocksTool,
    "read_blocks",
    quire_server::mcp::ReadBlocksParams,
    read_blocks
);
define_contextual_report_tool!(
    ApplyOperationsTool,
    "apply_operations",
    quire_server::mcp::ApplyOperationsParams,
    exec_apply_operations
);
define_contextual_report_tool!(
    RollbackRevisionTool,
    "rollback_revision",
    quire_server::mcp::RollbackParams,
    exec_rollback_revision
);
define_contextual_report_tool!(
    UpsertSourcesTool,
    "upsert_sources",
    quire_server::mcp::UpsertSourcesParams,
    exec_upsert_sources
);
define_stateless_report_tool!(
    ListSourcesTool,
    "list_sources",
    quire_server::mcp::ListSourcesParams,
    list_sources
);
define_contextual_report_tool!(
    SetCoverTool,
    "set_cover",
    quire_server::mcp::SetCoverParams,
    exec_set_cover
);
define_contextual_report_tool!(
    ConfigureTocTool,
    "configure_toc",
    quire_server::mcp::ConfigureTocParams,
    exec_configure_toc
);
define_contextual_report_tool!(
    ConfigureReferencesTool,
    "configure_references",
    quire_server::mcp::ConfigureReferencesParams,
    exec_configure_references
);
define_contextual_report_tool!(
    ConfigurePageChromeTool,
    "configure_page_chrome",
    quire_server::mcp::ConfigurePageChromeParams,
    exec_configure_page_chrome
);
define_contextual_report_tool!(
    UpdateStyleAndCssTool,
    "update_style_and_css",
    quire_server::mcp::UpdateStyleParams,
    exec_update_style_and_css
);
define_stateless_report_tool!(
    LintDocumentTool,
    "lint_document",
    quire_server::mcp::LintParams,
    lint_document
);
define_stateless_report_tool!(
    ListCommentsTool,
    "list_comments",
    quire_server::mcp::ListCommentsParams,
    list_comments
);
define_contextual_report_tool!(
    AddCommentTool,
    "add_comment",
    quire_server::mcp::AddCommentParams,
    exec_add_comment
);
define_contextual_report_tool!(
    ReplyCommentTool,
    "reply_comment",
    quire_server::mcp::ReplyCommentParams,
    exec_reply_comment
);
