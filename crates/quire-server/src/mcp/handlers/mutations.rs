use super::*;

#[tool_router(router = mutations_router, vis = "pub(crate)")]
impl QuireMcpServer {
    #[tool(
        name = "apply_operations",
        description = quire_protocol::tool_description!("apply_operations")
    )]
    pub async fn apply_operations(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<ApplyOperationsParams>,
    ) -> Result<Json<ApplyOperationsReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_apply_operations(context, request).await
    }
    // ---------------------------------------------------------------------
    // Tool 8: rollback
    // ---------------------------------------------------------------------
    #[tool(
        name = "rollback_revision",
        description = quire_protocol::tool_description!("rollback_revision")
    )]
    pub async fn rollback_revision(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<RollbackParams>,
    ) -> Result<Json<RollbackReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_rollback_revision(context, request).await
    }
    // ---------------------------------------------------------------------
    // Tool 9: upsert sources
    // ---------------------------------------------------------------------
    #[tool(
        name = "upsert_sources",
        description = quire_protocol::tool_description!("upsert_sources")
    )]
    pub async fn upsert_sources(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<UpsertSourcesParams>,
    ) -> Result<Json<UpsertSourcesReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_upsert_sources(context, request).await
    }
    // ---------------------------------------------------------------------
    // Tool 10: list sources
    // ---------------------------------------------------------------------
    #[tool(
        name = "list_sources",
        annotations(read_only_hint = true),
        description = quire_protocol::tool_description!("list_sources")
    )]
    pub async fn list_sources(
        &self,
        Parameters(request): Parameters<ListSourcesParams>,
    ) -> Result<Json<ListSourcesReceipt>, ErrorData> {
        let doc_id = parse_doc_id(&request.doc_id)?;
        let document = self
            .state
            .store
            .get_document(&doc_id)
            .await
            .map_err(mcp_error)?;
        let mut sources = IndexMap::new();
        let mut missing = Vec::new();
        if let Some(source_ids) = &request.source_ids {
            for raw_id in source_ids {
                let source_id = SourceId::parse(raw_id).map_err(protocol_error)?;
                match document.sources.get(&source_id) {
                    Some(source) => {
                        sources.insert(source_id, source.clone());
                    }
                    None => missing.push(source_id.to_string()),
                }
            }
        } else {
            sources = document.sources.clone();
        }
        if !missing.is_empty() {
            let valid: Vec<String> = document.sources.keys().map(|id| id.to_string()).collect();
            return Err(invalid_params(
                format!("source IDs not found: {}", missing.join(", ")),
                "Pass only registered IDs from list_sources or a citation receipt.".to_string(),
                Some(McpErrorDetails::MissingSourceIds {
                    missing,
                    valid_source_ids: valid,
                }),
            ));
        }
        let _next_hint = if sources.is_empty() {
            "No sources are registered. Add only sources actually read and used with upsert_sources."
                .to_string()
        } else {
            "Match each citation to a specific claim and its evidence excerpt; fetch the original page before relying on a search-result snippet.".to_string()
        };
        Ok(Json(ListSourcesReceipt {
            doc_id: document.doc_id.to_string(),
            revision: document.revision.get(),
            count: sources.len(),
            sources,
            _next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 11: set cover
    // ---------------------------------------------------------------------
    #[tool(
        name = "set_cover",
        description = quire_protocol::tool_description!("set_cover")
    )]
    pub async fn set_cover(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<SetCoverParams>,
    ) -> Result<Json<SetCoverReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_set_cover(context, request).await
    }
    // ---------------------------------------------------------------------
    // Tool 12: configure TOC
    // ---------------------------------------------------------------------
    #[tool(
        name = "configure_toc",
        description = quire_protocol::tool_description!("configure_toc")
    )]
    pub async fn configure_toc(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<ConfigureTocParams>,
    ) -> Result<Json<ConfigureTocReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_configure_toc(context, request).await
    }
    // ---------------------------------------------------------------------
    // Tool 13: configure references
    // ---------------------------------------------------------------------
    #[tool(
        name = "configure_references",
        description = quire_protocol::tool_description!("configure_references")
    )]
    pub async fn configure_references(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<ConfigureReferencesParams>,
    ) -> Result<Json<ConfigureReferencesReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_configure_references(context, request).await
    }
    // ---------------------------------------------------------------------
    // Tool 14: configure page chrome
    // ---------------------------------------------------------------------
    #[tool(
        name = "configure_page_chrome",
        description = quire_protocol::tool_description!("configure_page_chrome")
    )]
    pub async fn configure_page_chrome(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<ConfigurePageChromeParams>,
    ) -> Result<Json<ConfigurePageChromeReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_configure_page_chrome(context, request).await
    }
    // ---------------------------------------------------------------------
    // Tool 15: update style and CSS
    // ---------------------------------------------------------------------
    #[tool(
        name = "update_style_and_css",
        description = quire_protocol::tool_description!("update_style_and_css")
    )]
    pub async fn update_style_and_css(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<UpdateStyleParams>,
    ) -> Result<Json<UpdateStyleReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_update_style_and_css(context, request).await
    }
    // ---------------------------------------------------------------------
    // Tool 16: lint
    // ---------------------------------------------------------------------
    #[tool(
        name = "lint_document",
        annotations(read_only_hint = true),
        description = quire_protocol::tool_description!("lint_document")
    )]
    pub async fn lint_document(
        &self,
        Parameters(request): Parameters<LintParams>,
    ) -> Result<Json<LintReceipt>, ErrorData> {
        let doc_id = parse_doc_id(&request.doc_id)?;
        let document = self
            .state
            .store
            .get_document(&doc_id)
            .await
            .map_err(mcp_error)?;
        let report = lint_document_with_options(
            &document,
            &LintOptions {
                select: request.select,
                ignore: request.ignore,
            },
        );
        let rev = document.revision.get();
        let suppressed_blocks: BTreeMap<String, Vec<String>> = document
            .blocks
            .iter()
            .filter(|block| !block.lint_ignore.is_empty())
            .map(|block| (block.id.to_string(), block.lint_ignore.clone()))
            .collect();
        if report.diagnostics.is_empty()
            && report.summary.suppressed == 0
            && suppressed_blocks.is_empty()
        {
            return Ok(Json(LintReceipt {
                ok: true,
                rev,
                summary: None,
                suppressed_blocks: BTreeMap::new(),
                diagnostics: Vec::new(),
                _next_hint: None,
            }));
        }
        let ok = !report.has_errors();
        let _next_hint = lint_hint::lint_document_next_hint(&report);
        Ok(Json(LintReceipt {
            ok,
            rev,
            summary: Some(report.summary),
            suppressed_blocks,
            diagnostics: report.diagnostics,
            _next_hint: Some(_next_hint),
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 17: list comments
    // ---------------------------------------------------------------------
    #[tool(
        name = "list_comments",
        annotations(read_only_hint = true),
        description = quire_protocol::tool_description!("list_comments")
    )]
    pub async fn list_comments(
        &self,
        Parameters(request): Parameters<ListCommentsParams>,
    ) -> Result<Json<ListCommentsReceipt>, ErrorData> {
        let doc_id = parse_doc_id(&request.doc_id)?;
        let block_id = request
            .block_id
            .as_deref()
            .map(parse_block_id)
            .transpose()?;
        let mut comments = self
            .state
            .store
            .list_comment_threads(&doc_id, request.status, block_id.as_ref())
            .await
            .map_err(mcp_error)?;
        if request.status.is_none() {
            comments.retain(|thread| thread.status != CommentStatus::Archived);
        }
        let count = comments.len();
        let _next_hint = if comments.is_empty() {
            "No matching review comments. Human reviewers can create threads with POST /api/docs/{doc_id}/comments.".to_string()
        } else {
            "Reply with reply_comment(doc_id, thread_id, body, mark_addressed=true, addressed_rev=...) after fixing the block. Only humans can archive a thread.".to_string()
        };
        Ok(Json(ListCommentsReceipt {
            doc_id: doc_id.to_string(),
            comments,
            count,
            _next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 18: add comment
    // ---------------------------------------------------------------------
    #[tool(
        name = "add_comment",
        description = quire_protocol::tool_description!("add_comment")
    )]
    pub async fn add_comment(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<AddCommentParams>,
    ) -> Result<Json<AddCommentReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_add_comment(context, request).await
    }
    // ---------------------------------------------------------------------
    // Tool 19: reply comment
    // ---------------------------------------------------------------------
    #[tool(
        name = "reply_comment",
        description = quire_protocol::tool_description!("reply_comment")
    )]
    pub async fn reply_comment(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<ReplyCommentParams>,
    ) -> Result<Json<ReplyCommentReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_reply_comment(context, request).await
    }
}

// -------------------------------------------------------------------------
// Internal helpers
// -------------------------------------------------------------------------

impl CoverPatch {
    /// Validate and apply a partial cover update, returning self-healing errors on bad input.
    pub(crate) fn apply_to(&self, cover: &mut CoverConfig) -> Result<(), ErrorData> {
        if let Some(enabled) = self.enabled {
            cover.enabled = enabled;
        }
        if let Some(variant) = &self.variant {
            if !COVER_VARIANTS.contains(&variant.as_str()) {
                return Err(invalid_params(
                    format!("unknown cover variant `{variant}`"),
                    format!("Valid cover variants: {}", COVER_VARIANTS.join(", ")),
                    Some(McpErrorDetails::ValidVariants {
                        valid_variants: COVER_VARIANTS,
                    }),
                ));
            }
            cover.variant = variant.clone();
        }
        if let Some(title) = &self.title {
            if title.trim().is_empty() {
                return Err(invalid_params(
                    "cover title must not be empty",
                    "Supply a title or omit the field.",
                    None,
                ));
            }
            cover.title = title.clone();
        }
        if let Some(subtitle) = &self.subtitle {
            cover.subtitle = (!subtitle.trim().is_empty()).then(|| subtitle.clone());
        }
        if let Some(authors) = &self.authors {
            cover.authors = authors.clone();
        }
        if let Some(date) = &self.date {
            cover.date = (!date.trim().is_empty()).then(|| date.clone());
        }
        if let Some(meta) = &self.meta {
            cover.meta = meta.clone();
        }
        if let Some(custom_html) = &self.custom_html {
            cover.custom_html = (!custom_html.trim().is_empty()).then(|| custom_html.clone());
        }
        Ok(())
    }
}
