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
pub(crate) async fn resolve_existing_session_doc_id(
    context: &ToolContext,
    runtime: &RuntimeToolScope,
    explicit_doc_id: &str,
) -> Result<Option<String>, ToolExecutionError> {
    let trimmed = explicit_doc_id.trim();
    if !trimmed.is_empty() {
        return Ok(Some(trimmed.to_string()));
    }
    if let Some(existing) = super::super::current_doc_id(&runtime.doc_id)
        && !existing.trim().is_empty()
    {
        return Ok(Some(existing));
    }
    let turn = quire_turn_meta(context)?;
    if let Ok(summary) = runtime
        .engine
        .kb
        .store()
        .get_session_summary(&turn.session_id)
        .await
        && let Some(bound) = summary.doc_id.or(summary.linked_doc_id)
        && !bound.trim().is_empty()
    {
        super::super::update_doc_id(&runtime.doc_id, Some(bound.clone()));
        return Ok(Some(bound));
    }
    Ok(None)
}

pub(crate) struct UpsertSourcesTool;

impl Tool for UpsertSourcesTool {
    const NAME: &'static str = "upsert_sources";
    type Error = ToolExecutionError;
    type Args = quire_server::mcp::UpsertSourcesParams;
    type Output = Value;

    fn description(&self) -> String {
        quire_server::mcp::QuireMcpServer::tool_metadata("upsert_sources").0
    }

    fn parameters(&self) -> Value {
        quire_server::mcp::QuireMcpServer::tool_metadata("upsert_sources").1
    }

    async fn call(
        &self,
        context: &mut ToolContext,
        mut args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let runtime = runtime_tool_scope(context)?;
        if let Some(doc_id) =
            resolve_existing_session_doc_id(context, &runtime, &args.doc_id).await?
        {
            args.doc_id = doc_id;
            let saved_sources = args.sources.clone();
            let req_ctx = in_process_request_context(context)?;
            let rmcp::handler::server::wrapper::Json(receipt) = runtime
                .report_mcp
                .exec_upsert_sources(req_ctx, args)
                .await
                .map_err(mcp_error_to_tool_error)?;
            let mut val = serde_json::to_value(receipt).map_err(tool_other)?;
            if let Some(obj) = val.as_object_mut() {
                obj.insert(
                    "sources".to_string(),
                    serde_json::to_value(saved_sources).unwrap_or(Value::Null),
                );
            }
            return Ok(val);
        }
        // Pure chat-session mode: register sources for inline `<citation id="..."/>`
        // without creating or binding any report document.
        let upserted_ids: Vec<String> = args
            .sources
            .iter()
            .filter_map(|entry| entry.id.as_ref().map(|id| id.as_str().to_string()))
            .collect();
        Ok(serde_json::json!({
            "mode": "chat_session",
            "doc_id": Value::Null,
            "upserted_ids": upserted_ids,
            "total_sources": args.sources.len(),
            "sources": args.sources,
        }))
    }
}

pub(crate) struct ListSourcesTool;

impl Tool for ListSourcesTool {
    const NAME: &'static str = "list_sources";
    type Error = ToolExecutionError;
    type Args = quire_server::mcp::ListSourcesParams;
    type Output = Value;

    fn description(&self) -> String {
        quire_server::mcp::QuireMcpServer::tool_metadata("list_sources").0
    }

    fn parameters(&self) -> Value {
        quire_server::mcp::QuireMcpServer::tool_metadata("list_sources").1
    }

    async fn call(
        &self,
        context: &mut ToolContext,
        mut args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let runtime = runtime_tool_scope(context)?;
        if let Some(doc_id) =
            resolve_existing_session_doc_id(context, &runtime, &args.doc_id).await?
        {
            args.doc_id = doc_id;
            let rmcp::handler::server::wrapper::Json(receipt) = runtime
                .report_mcp
                .list_sources(rmcp::handler::server::wrapper::Parameters(args))
                .await
                .map_err(mcp_error_to_tool_error)?;
            return serde_json::to_value(receipt).map_err(tool_other);
        }
        Ok(serde_json::json!({
            "mode": "chat_session",
            "doc_id": Value::Null,
            "sources": [],
        }))
    }
}
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
