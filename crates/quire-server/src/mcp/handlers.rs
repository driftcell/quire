//! MCP tool implementations, Smart CAS reload helpers and document projections.

use std::collections::{BTreeSet, HashSet};

use axum::http::request::Parts;
use indexmap::IndexMap;
use quire_lint::{LintOptions, lint_document_with_options};
use quire_protocol::{
    Block, BlockId, BlockKind, COVER_VARIANTS, CommentAuthorType, CommentStatus, CoverConfig,
    Diagnostic, DocId, EphemeralClearEvent, EphemeralPayload, IdAssignment, MarkdownOptions,
    MarkdownProjector, PageChromeConfig, PatchOp, ProtocolError, ReportDocument, Revision,
    Severity, SourceId, TOC_VARIANTS, is_safe_css_token_value, is_valid_css_token_name,
    scan_document_citations, short_token,
};
use quire_store::{
    ApplyResult, CommitExtras, NewCommentReply, NewCommentThread, SourceUpsert, StoreError,
};
use rmcp::{
    ErrorData,
    handler::server::wrapper::{Json, Parameters},
    tool, tool_router,
};
use serde_json::json;

use crate::{
    auth::RequestContext,
    error::{invalid_params, mcp_error},
    guide::{guide, guide_workflow},
    state::AppState,
};

use super::QuireMcpServer;
use super::lint_hint;
use super::schema::*;

#[tool_router(router = tool_router, vis = "pub(crate)")]
impl QuireMcpServer {
    // ---------------------------------------------------------------------
    // Tool 1: protocol guide
    // ---------------------------------------------------------------------
    #[tool(
        name = "get_protocol_guide",
        annotations(read_only_hint = true),
        description = "Progressive protocol schema disclosure. Call quickstart first, then tables_and_charts, citations_and_css and parallel_workflow as needed."
    )]
    pub async fn get_protocol_guide(
        &self,
        Parameters(request): Parameters<GuideParams>,
    ) -> Result<Json<GuideReceipt>, ErrorData> {
        let receipt = guide(request.topic);
        let workflow = guide_workflow();
        Ok(Json(GuideReceipt {
            topic: receipt.topic,
            guide: receipt.guide,
            recommended_tools: receipt.recommended_tools,
            workflow,
            _next_hint: receipt._next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 2: create report
    // ---------------------------------------------------------------------
    #[tool(
        name = "create_report",
        description = "Create a new report. Uses X-Session-Id from the MCP HTTP header for frontend session discovery. Idempotent when idempotency_key is supplied."
    )]
    pub async fn create_report(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<CreateReportParams>,
    ) -> Result<Json<CreateReportReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_create_report(context, request).await
    }
    // ---------------------------------------------------------------------
    // Tool 3: list reports
    // ---------------------------------------------------------------------
    #[tool(
        name = "list_reports",
        annotations(read_only_hint = true),
        description = "List reports, newest first, optionally filtered by doc_id or title substring."
    )]
    pub async fn list_reports(
        &self,
        Parameters(request): Parameters<ListReportsParams>,
    ) -> Result<Json<ListReportsReceipt>, ErrorData> {
        let list = self
            .state
            .store
            .list_reports(request.query.as_deref(), request.limit.unwrap_or(20), 0)
            .await
            .map_err(mcp_error)?;
        let _next_hint = if list.reports.is_empty() {
            "No reports matched. Call create_report to start one.".to_string()
        } else {
            "Call get_document_digest(doc_id) for the report you want to edit.".to_string()
        };
        let reports = list
            .reports
            .into_iter()
            .map(ReportSummaryView::from)
            .collect();
        Ok(Json(ListReportsReceipt {
            reports,
            total: list.total,
            limit: list.limit,
            offset: list.offset,
            _next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 4: document digest
    // ---------------------------------------------------------------------
    #[tool(
        name = "get_document_digest",
        annotations(read_only_hint = true),
        description = "Low-token outline with current_rev, requirement aggregates (paragraphs, prose chars, visuals, cited sources) and compact blocks (id/type/level/preview/char_count/source_ids). Optional keyword query filters blocks."
    )]
    pub async fn get_document_digest(
        &self,
        Parameters(request): Parameters<DigestParams>,
    ) -> Result<Json<DigestReceipt>, ErrorData> {
        let doc_id = parse_doc_id(&request.doc_id)?;
        let document = self
            .state
            .store
            .get_document(&doc_id)
            .await
            .map_err(mcp_error)?;
        let digest = build_digest(&document, request.query.as_deref());
        let _next_hint = if digest.blocks.is_empty() {
            "No blocks matched. Append blocks with apply_operations.".to_string()
        } else {
            format!(
                "rev={}; read_blocks/read_as_markdown for details.",
                digest.current_rev
            )
        };
        Ok(Json(DigestReceipt { digest, _next_hint }))
    }

    // ---------------------------------------------------------------------
    // Tool 5: read as markdown
    // ---------------------------------------------------------------------
    #[tool(
        name = "read_as_markdown",
        annotations(read_only_hint = true),
        description = "Project an online report into token-efficient Markdown with block anchors, flattened tables, expanded chart data and expanded citations."
    )]
    pub async fn read_as_markdown(
        &self,
        Parameters(request): Parameters<ReadMarkdownParams>,
    ) -> Result<Json<ReadMarkdownReceipt>, ErrorData> {
        let markdown_options = MarkdownOptions {
            include_chart_data: request.include_chart_data.unwrap_or(true),
            include_front_matter: true,
        };
        let doc_id = parse_doc_id(&request.doc_id)?;
        let document = self
            .state
            .store
            .get_document(&doc_id)
            .await
            .map_err(mcp_error)?;
        let projector = MarkdownProjector::new(&document, &markdown_options);
        let markdown = if let Some(block_ids) = &request.block_ids {
            let block_ids = parse_block_ids(block_ids)?;
            projector.render_blocks(&block_ids).map_err(|error| {
                invalid_params(
                    error.to_string(),
                    "Use valid block IDs from get_document_digest.",
                    None,
                )
            })?
        } else if let Some(section) = &request.section_heading {
            projector.render_section(section).map_err(|error| {
                invalid_params(
                    error.to_string(),
                    "Use an exact or substring heading from get_document_digest.",
                    None,
                )
            })?
        } else {
            projector.render().map_err(|error| {
                invalid_params(error.to_string(), "Reload the document and retry.", None)
            })?
        };
        let _next_hint =
            "Patch only blocks that need changes; use the block anchors above as exact IDs."
                .to_string();
        Ok(Json(ReadMarkdownReceipt {
            source: format!("doc:{doc_id}"),
            doc_id: document.doc_id.to_string(),
            revision: document.revision.get(),
            markdown_chars: markdown.chars().count(),
            markdown,
            _next_hint,
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 6: read blocks
    // ---------------------------------------------------------------------
    #[tool(
        name = "read_blocks",
        annotations(read_only_hint = true),
        description = "Fetch full canonical protocol JSON for targeted blocks."
    )]
    pub async fn read_blocks(
        &self,
        Parameters(request): Parameters<ReadBlocksParams>,
    ) -> Result<Json<ReadBlocksReceipt>, ErrorData> {
        let doc_id = parse_doc_id(&request.doc_id)?;
        let block_ids = parse_block_ids(&request.block_ids)?;
        let document = self
            .state
            .store
            .get_document(&doc_id)
            .await
            .map_err(mcp_error)?;
        let mut blocks = Vec::new();
        let mut missing = Vec::new();
        for block_id in &block_ids {
            match document.block(block_id.as_str()) {
                Some(block) => blocks.push(block.clone()),
                None => missing.push(block_id.to_string()),
            }
        }
        if !missing.is_empty() {
            let valid: Vec<&str> = document
                .blocks
                .iter()
                .map(|block| block.id.as_str())
                .collect();
            return Err(invalid_params(
                format!("block IDs not found: {}", missing.join(", ")),
                format!("Valid block_ids: {}", valid.join(", ")),
                Some(json!({"missing": missing, "valid_block_ids": valid})),
            ));
        }
        Ok(Json(ReadBlocksReceipt {
            doc_id: document.doc_id.to_string(),
            revision: document.revision.get(),
            blocks,
            _next_hint: "Patch specific fields with apply_operations using the current revision."
                .into(),
        }))
    }

    // ---------------------------------------------------------------------
    // Tool 7: apply operations
    // ---------------------------------------------------------------------
    #[tool(
        name = "apply_operations",
        description = "Atomically apply declarative block patch operations with Smart CAS rebasing and optional same-transaction source upserts. Returns new_revision, assigned_ids, inline_warnings and _next_hint."
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
        description = "Revert document state by replaying stored inverse_ops in reverse order. Creates a new monotonic revision; history is never rewritten."
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
        description = "Register or update citation metadata with URL + normalized-title deduplication. Returns canonical_id_map; cite the canonical source_id."
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
        description = "List registered citation metadata and evidence excerpts. Optionally pass exact source_ids for claim-level verification; unknown IDs fail explicitly."
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
            let valid: Vec<&str> = document.sources.keys().map(|id| id.as_str()).collect();
            return Err(invalid_params(
                format!("source IDs not found: {}", missing.join(", ")),
                "Pass only registered IDs from list_sources or a citation receipt.".to_string(),
                Some(json!({"missing": missing, "valid_source_ids": valid})),
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
        description = "Partially update cover fields (enabled, variant, title, subtitle, authors, date, meta, custom_html)."
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
        description = "Enable/disable the auto-generated TOC and set title, max_depth (1..=3), variant (standard|numbered|dotted) and page breaks before/after the TOC."
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
        description = "Configure the bibliography: enabled, title, style (numeric|author_year) and page_break_before. Returns orphan (unreferenced) source IDs."
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
        description = "Configure A4 paged header/footer slots (left/center/right) with template placeholders ({page}, {total}, {title}, {subtitle}, {date}, {kicker}, {authors}), first-page visibility and automatic heading page breaks (none/h1/h1_and_h2)."
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
        description = "Update theme preset, CSS custom-property tokens and scoped custom_css. Validates token names/values and CSS braces/strings/comments with exact line/column errors."
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
        description = "Run zero-browser static validation: dangling/unused citations, table schema/row keys, CSS syntax, custom_html fallback, heading/TOC/chart rules."
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
        if report.diagnostics.is_empty() && report.summary.suppressed == 0 {
            return Ok(Json(LintReceipt {
                ok: true,
                rev,
                summary: None,
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
        description = "List side-channel review threads. Archived threads are omitted unless status=archived is supplied. Includes anchor_snapshot and live anchor_detached flags."
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
        description = "Create an agent review comment anchored to a block. Comments are side-channel data: they never increment current_rev or participate in Smart CAS."
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
        description = "Reply to a review thread. With mark_addressed=true an open thread moves to addressed and records addressed_rev; agents cannot archive or reopen threads."
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
                    Some(json!({"valid_variants": COVER_VARIANTS})),
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

fn block_digest(block: &Block) -> BlockDigest {
    let is_heading = matches!(&block.body, BlockKind::Heading { .. });
    let char_count = block.char_count();
    BlockDigest {
        id: block.id.to_string(),
        block_type: block.block_type().to_string(),
        level: match &block.body {
            BlockKind::Heading { level, .. } => Some(*level),
            _ => None,
        },
        preview: (char_count > 0).then(|| block.preview_30()),
        char_count: (!is_heading && char_count > 0).then_some(char_count),
        source_ids: block.source_ids.iter().map(ToString::to_string).collect(),
    }
}

/// Build the compact outline returned by `get_document_digest` and create receipts.
pub(crate) fn build_digest(document: &ReportDocument, query: Option<&str>) -> DocumentDigest {
    let query = query
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .map(str::to_lowercase);
    let blocks = document
        .blocks
        .iter()
        .filter(|block| {
            let Some(query) = &query else {
                return true;
            };
            block.id.to_lowercase().contains(query)
                || block.block_type().contains(query.as_str())
                || block.visible_text().to_lowercase().contains(query)
        })
        .map(block_digest)
        .collect::<Vec<_>>();
    let paragraph_count = document
        .blocks
        .iter()
        .filter(|block| matches!(&block.body, BlockKind::Paragraph { .. }))
        .count();
    let prose_char_count = document
        .blocks
        .iter()
        .filter(|block| matches!(&block.body, BlockKind::Paragraph { .. }))
        .map(|block| block.char_count())
        .sum();
    let visual_block_count = document
        .blocks
        .iter()
        .filter(|block| {
            matches!(
                &block.body,
                BlockKind::Table(_) | BlockKind::Echarts(_) | BlockKind::CustomHtml(_)
            )
        })
        .count();
    let cited_source_count = scan_document_citations(document).cited_source_ids.len();
    DocumentDigest {
        title: document.title(),
        current_rev: document.revision.get(),
        theme: document.theme.summary(),
        toc: document.toc.clone(),
        block_count: document.blocks.len(),
        source_count: document.sources.len(),
        paragraph_count,
        prose_char_count,
        visual_block_count,
        cited_source_count,
        blocks,
    }
}

impl QuireMcpServer {
    pub async fn exec_create_report(
        &self,
        context: RequestContext,
        request: CreateReportParams,
    ) -> Result<Json<CreateReportReceipt>, ErrorData> {
        let title = request.title.trim();
        if title.is_empty() {
            return Err(invalid_params(
                "title must not be empty",
                "Provide a short human-readable report title.",
                None,
            ));
        }
        if let Some(key) = request.idempotency_key.as_deref()
            && (key.trim().is_empty() || key.len() > 128)
        {
            return Err(invalid_params(
                "idempotency_key must be 1-128 characters",
                "Use a stable key such as `sess_01:create_report`.",
                None,
            ));
        }
        let session_id = context.session_id.ok_or_else(|| {
            invalid_params(
                "create_report requires a session identity",
                "Send X-Session-Id or per-call _meta.quire.session_id.",
                None,
            )
        })?;
        let actor = context.actor;
        let scoped_idempotency_key = request.idempotency_key.as_deref().map(|key| {
            let trimmed = key.trim();
            let prefix = format!("{}:", session_id.as_str());
            let scoped = if trimmed.starts_with(&prefix) {
                trimmed.to_string()
            } else {
                format!("{prefix}{trimmed}")
            };
            if scoped.len() > 128 {
                scoped.chars().take(128).collect()
            } else {
                scoped
            }
        });
        let doc_id = match request
            .doc_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(value) => {
                let requested_id = DocId::parse(value).map_err(protocol_error)?;
                match self.state.store.get_document(&requested_id).await {
                    Ok(existing) if existing.session_id.as_ref() != Some(&session_id) => {
                        let suffix = short_token(6);
                        let base: String = requested_id.as_str().chars().take(56).collect();
                        DocId::parse(format!("{base}-{suffix}")).map_err(protocol_error)?
                    }
                    _ => requested_id,
                }
            }
            None => generate_doc_id(title)?,
        };

        let mut document = ReportDocument::new(doc_id);
        document.session_id = Some(session_id);
        document.cover.enabled = request.enable_cover.unwrap_or(true);
        document.cover.title = title.to_string();
        if let Some(subtitle) = request
            .subtitle
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            document.cover.subtitle = Some(subtitle.to_string());
        }
        if let Some(authors) = request.authors {
            document.cover.authors = authors
                .into_iter()
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty())
                .collect();
        }
        if let Some(date) = request
            .date
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            document.cover.date = Some(date.to_string());
        }
        if let Some(meta) = request.cover_meta {
            document.cover.meta = meta;
        }
        document.toc.enabled = request.enable_toc.unwrap_or(true);
        if title.chars().any(is_cjk_char) {
            document.toc.title = "目录".to_string();
            document.references.title = "参考文献".to_string();
        }
        if let Some(preset) = request.theme_preset {
            document.theme.preset = preset;
        }
        document.blocks = request.initial_blocks.unwrap_or_default();
        let duplicates = document.duplicate_block_ids();
        if !duplicates.is_empty() {
            return Err(invalid_params(
                format!("duplicate initial block IDs: {}", duplicates.join(", ")),
                "Every block id must be unique. Rename duplicates before create_report.",
                Some(json!({"duplicate_block_ids": duplicates})),
            ));
        }

        let result = self
            .state
            .store
            .create_document(&document, &actor, scoped_idempotency_key.as_deref())
            .await
            .map_err(mcp_error)?;
        let stored = self
            .state
            .store
            .get_document(&result.doc_id)
            .await
            .map_err(mcp_error)?;
        let digest = build_digest(&stored, None);
        let revision = result.revision.get();
        let _next_hint = if result.idempotent_replay {
            format!(
                "Idempotent replay. Read get_document_digest or call apply_operations with expected_revision={}.",
                revision
            )
        } else {
            "Call get_protocol_guide(\"tables_and_charts\") and get_document_digest before authoring. Sub-agents should own disjoint block_id prefixes.".to_string()
        };
        Ok(Json(CreateReportReceipt {
            doc_id: result.doc_id.to_string(),
            revision,
            title: result.title,
            session_id: result
                .session_id
                .map(|session| session.as_str().to_string()),
            idempotent_replay: result.idempotent_replay,
            digest,
            _next_hint,
        }))
    }

    pub async fn exec_apply_operations(
        &self,
        context: RequestContext,
        request: ApplyOperationsParams,
    ) -> Result<Json<ApplyOperationsReceipt>, ErrorData> {
        if request.summary.trim().is_empty() {
            return Err(invalid_params(
                "summary must not be empty",
                "Describe the intent, for example `write chapter 2 executive summary`.",
                None,
            ));
        }
        let actor = context.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        let expected_revision = parse_revision(request.expected_revision)?;
        let source_upserts = inline_source_upserts(request.sources)?;
        let preview_ids = normalize_preview_ids(request.preview_ids)?;
        let result = self
            .state
            .store
            .apply_operations_with_extras(
                &doc_id,
                expected_revision,
                &actor,
                request.summary.trim(),
                &request.ops,
                CommitExtras {
                    sources: source_upserts,
                    cleared_preview_ids: preview_ids.clone(),
                },
            )
            .await
            .map_err(mcp_error)?;
        if result.outcome.no_op {
            publish_ephemeral_clear(&self.state, &doc_id, &preview_ids).await;
        }
        let touched = touched_blocks(&request.ops, &result.outcome.assigned_ids);
        let (blocking_errors, inline_warnings) =
            split_scoped_diagnostics(result.inline_warnings, &touched);
        let _next_hint = lint_hint::apply_operations_next_hint(
            result.new_revision.get(),
            &blocking_errors,
            &inline_warnings,
            result.outcome.no_op,
            result.rebased,
            result.rebased_over,
            result.base_revision.get(),
        );
        Ok(Json(ApplyOperationsReceipt {
            new_revision: result.new_revision.get(),
            base_revision: result.base_revision.get(),
            rebased: result.rebased,
            rebased_over: result.rebased_over,
            committed: true,
            assigned_ids: result.outcome.assigned_ids,
            inline_warnings,
            blocking_errors,
            _next_hint,
        }))
    }

    pub async fn exec_rollback_revision(
        &self,
        context: RequestContext,
        request: RollbackParams,
    ) -> Result<Json<RollbackReceipt>, ErrorData> {
        let actor = context.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        let target_revision = parse_revision(request.target_revision)?;
        let result = self
            .state
            .store
            .rollback_to_revision(&doc_id, target_revision, &actor)
            .await
            .map_err(mcp_error)?;
        let summary = format!(
            "Rolled back to revision {} by replaying {} inverse revision(s); history is preserved as revision {}.",
            result.target_revision, result.replayed_revisions, result.new_revision
        );
        let _next_hint = format!(
            "Call get_document_digest or read_blocks to inspect revision {}, then lint_document before continuing edits.",
            result.new_revision
        );
        Ok(Json(RollbackReceipt {
            doc_id: doc_id.to_string(),
            new_revision: result.new_revision.get(),
            rolled_back_to: result.target_revision.get(),
            replayed_revisions: result.replayed_revisions,
            summary,
            _next_hint,
        }))
    }

    pub async fn exec_upsert_sources(
        &self,
        context: RequestContext,
        request: UpsertSourcesParams,
    ) -> Result<Json<UpsertSourcesReceipt>, ErrorData> {
        if request.sources.is_empty() {
            return Err(invalid_params(
                "sources must not be empty",
                "Supply at least one source object with id/title (and preferably url).",
                None,
            ));
        }
        let actor = context.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        let upserts: Vec<SourceUpsert> = request
            .sources
            .into_iter()
            .map(|source| {
                let source_id = source.id.ok_or_else(|| {
                    invalid_params(
                        "source id is required",
                        "Every upsert_sources entry needs `id`, e.g. `\"id\":\"sec10\"`.",
                        None,
                    )
                })?;
                Ok(SourceUpsert {
                    source_id,
                    source: source.entry,
                })
            })
            .collect::<Result<Vec<_>, ErrorData>>()?;
        let result = self
            .state
            .store
            .upsert_sources(&doc_id, &actor, upserts)
            .await
            .map_err(mcp_error)?;
        let _next_hint = if result.changed {
            format!(
                "Sources committed at revision {}. Use canonical_id_map values in [^source_id] markers.",
                result.revision
            )
        } else {
            "Sources already existed; no revision was created. Use canonical_id_map when citing."
                .to_string()
        };
        Ok(Json(UpsertSourcesReceipt {
            doc_id: doc_id.to_string(),
            revision: result.revision.get(),
            changed: result.changed,
            canonical_id_map: result
                .canonical_id_map
                .into_iter()
                .map(|(from, to)| (from.to_string(), to.to_string()))
                .collect(),
            source_count: result.sources.len(),
            inline_warnings: result.inline_warnings,
            _next_hint,
        }))
    }

    pub async fn exec_set_cover(
        &self,
        context: RequestContext,
        request: SetCoverParams,
    ) -> Result<Json<SetCoverReceipt>, ErrorData> {
        let actor = context.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        let patch = request.cover_patch;
        let (result, updated) = self
            .apply_with_reload(&doc_id, &actor, "update_cover", |document| {
                let mut cover = document.cover.clone();
                patch.apply_to(&mut cover)?;
                Ok(vec![PatchOp::SetCover {
                    cover: cover.clone(),
                }])
            })
            .await?;
        let cover = updated.cover;
        let _next_hint = format!(
            "Cover committed at revision {}. Continue with configure_toc or authoring blocks.",
            result.new_revision
        );
        Ok(Json(SetCoverReceipt {
            doc_id: doc_id.to_string(),
            revision: result.new_revision.get(),
            cover,
            _next_hint,
        }))
    }

    pub async fn exec_configure_toc(
        &self,
        context: RequestContext,
        request: ConfigureTocParams,
    ) -> Result<Json<ConfigureTocReceipt>, ErrorData> {
        let actor = context.actor;
        if let Some(depth) = request.max_depth
            && !(1..=3).contains(&depth)
        {
            return Err(invalid_params(
                format!("max_depth {depth} is outside 1..=3"),
                "Use max_depth 1, 2 or 3.",
                Some(json!({"valid_max_depths":[1,2,3]})),
            ));
        }
        if let Some(variant) = &request.variant
            && !TOC_VARIANTS.contains(&variant.as_str())
        {
            return Err(invalid_params(
                format!("unknown TOC variant `{variant}`"),
                format!("Valid TOC variants: {}", TOC_VARIANTS.join(", ")),
                Some(json!({"valid_variants": TOC_VARIANTS})),
            ));
        }
        let doc_id = parse_doc_id(&request.doc_id)?;
        let (result, updated) = self
            .apply_with_reload(&doc_id, &actor, "configure_toc", |document| {
                let mut toc = document.toc.clone();
                toc.enabled = request.enabled;
                if let Some(title) = &request.title {
                    toc.title = title.clone();
                }
                if let Some(depth) = request.max_depth {
                    toc.max_depth = depth;
                }
                if let Some(variant) = &request.variant {
                    toc.variant = variant.clone();
                }
                if let Some(page_break_before) = request.page_break_before {
                    toc.page_break_before = page_break_before;
                }
                if let Some(page_break_after) = request.page_break_after {
                    toc.page_break_after = page_break_after;
                }
                Ok(vec![PatchOp::SetToc { toc: toc.clone() }])
            })
            .await?;
        let toc = updated.toc;
        let _next_hint = format!(
            "TOC committed at revision {}. Headings are auto-collected by the frontend; do not add TOC blocks.",
            result.new_revision
        );
        Ok(Json(ConfigureTocReceipt {
            doc_id: doc_id.to_string(),
            revision: result.new_revision.get(),
            toc,
            _next_hint,
        }))
    }

    pub async fn exec_configure_references(
        &self,
        context: RequestContext,
        request: ConfigureReferencesParams,
    ) -> Result<Json<ConfigureReferencesReceipt>, ErrorData> {
        let actor = context.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        let (result, document) = self
            .apply_with_reload(&doc_id, &actor, "configure_references", |document| {
                let mut references = document.references.clone();
                references.enabled = request.enabled;
                if let Some(title) = &request.title {
                    references.title = title.clone();
                }
                if let Some(style) = request.style {
                    references.style = style;
                }
                if let Some(page_break) = request.page_break_before {
                    references.page_break_before = page_break;
                }
                Ok(vec![PatchOp::SetReferences {
                    references: references.clone(),
                }])
            })
            .await?;
        let references = document.references.clone();
        let scan = quire_protocol::scan_document_citations(&document);
        let cited: HashSet<&str> = scan
            .uses
            .iter()
            .map(|usage| usage.source_id.as_str())
            .collect();
        let orphan_sources: Vec<String> = document
            .sources
            .keys()
            .filter(|source_id| !cited.contains(source_id.as_str()))
            .map(ToString::to_string)
            .collect();
        let _next_hint = if orphan_sources.is_empty() {
            "References committed. Bibliography labels follow citation first-appearance order."
                .to_string()
        } else {
            format!(
                "References committed. Orphan source_ids (registered but uncited): {}. Cite or remove them.",
                orphan_sources.join(", ")
            )
        };
        Ok(Json(ConfigureReferencesReceipt {
            doc_id: doc_id.to_string(),
            revision: result.new_revision.get(),
            references,
            orphan_sources,
            _next_hint,
        }))
    }

    pub async fn exec_configure_page_chrome(
        &self,
        context: RequestContext,
        request: ConfigurePageChromeParams,
    ) -> Result<Json<ConfigurePageChromeReceipt>, ErrorData> {
        let actor = context.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        if let Some(expected) = request.base_revision {
            let current = self
                .state
                .store
                .get_document(&doc_id)
                .await
                .map_err(mcp_error)?;
            let current_revision = current.revision.get();
            if u64::try_from(current_revision).ok() != Some(expected) {
                return Err(invalid_params(
                    format!(
                        "base_revision {expected} does not match current revision {current_revision}"
                    ),
                    "Reload the document and retry with the current revision.",
                    Some(json!({"current_revision": current_revision})),
                ));
            }
        }
        let (result, updated) = self
            .apply_with_reload(&doc_id, &actor, "configure_page_chrome", |document| {
                let mut theme = document.theme.clone();
                theme.page_chrome = Some(merge_page_chrome(theme.page_chrome.as_ref(), &request));
                Ok(vec![PatchOp::SetTheme { theme }])
            })
            .await?;
        let page_chrome = updated.theme.page_chrome.clone().unwrap_or_default();
        let _next_hint = format!(
            "Page chrome committed at revision {}. Slots support {{page}}, {{total}}, {{title}}, {{subtitle}}, {{date}}, {{kicker}} and {{authors}}; heading_page_break accepts none, h1 or h1_and_h2.",
            result.new_revision
        );
        Ok(Json(ConfigurePageChromeReceipt {
            doc_id: doc_id.to_string(),
            revision: result.new_revision.get(),
            page_chrome,
            _next_hint,
        }))
    }

    pub async fn exec_update_style_and_css(
        &self,
        context: RequestContext,
        request: UpdateStyleParams,
    ) -> Result<Json<UpdateStyleReceipt>, ErrorData> {
        let actor = context.actor;
        for (name, value) in &request.tokens_patch {
            if !is_valid_css_token_name(name) {
                return Err(invalid_params(
                    format!("invalid CSS token name `{name}`"),
                    "Token names must start with `--` and contain only ASCII letters, digits, '-' or '_'.",
                    Some(
                        json!({"valid_examples":["--rpt-primary","--rpt-accent","--rpt-font-body"]}),
                    ),
                ));
            }
            if !is_safe_css_token_value(value) {
                return Err(invalid_params(
                    format!("unsafe CSS token value for `{name}`"),
                    "Avoid `;`, `{`, `}`, `<`, `>` and backslashes in token values.",
                    None,
                ));
            }
        }
        if let Some(css) = &request.custom_css
            && let Err(error) = quire_protocol::validate_custom_css(css)
        {
            return Err(protocol_error(error));
        }
        let doc_id = parse_doc_id(&request.doc_id)?;
        let (result, updated) = self
            .apply_with_reload(&doc_id, &actor, "update_style_and_css", |document| {
                let mut theme = document.theme.clone();
                if let Some(preset) = &request.preset {
                    theme.preset = preset.clone();
                }
                for (name, value) in &request.tokens_patch {
                    theme.tokens.insert(name.clone(), value.clone());
                }
                if let Some(custom_css) = &request.custom_css {
                    theme.custom_css = match request.css_mode {
                        CssMode::Append if !theme.custom_css.is_empty() => {
                            format!("{}\n{}", theme.custom_css.trim_end(), custom_css)
                        }
                        _ => custom_css.clone(),
                    };
                }
                Ok(vec![PatchOp::SetTheme {
                    theme: theme.clone(),
                }])
            })
            .await?;
        let theme = updated.theme;
        let _next_hint = format!(
            "Theme committed at revision {}. Run lint_document to re-check CSS and source usage.",
            result.new_revision
        );
        Ok(Json(UpdateStyleReceipt {
            doc_id: doc_id.to_string(),
            revision: result.new_revision.get(),
            theme: theme.summary(),
            token_count: theme.tokens.len(),
            has_custom_css: !theme.custom_css.is_empty(),
            _next_hint,
        }))
    }

    pub async fn exec_add_comment(
        &self,
        context: RequestContext,
        request: AddCommentParams,
    ) -> Result<Json<AddCommentReceipt>, ErrorData> {
        let actor = context.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        let block_id = parse_block_id(&request.block_id)?;
        let end_block_id = request
            .end_block_id
            .as_deref()
            .map(parse_block_id)
            .transpose()?;
        let anchor_blocks = request
            .anchor_blocks
            .iter()
            .map(|raw| parse_block_id(raw))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(quote) = &request.quote {
            quote.validate().map_err(|reason| {
                invalid_params(
                    reason,
                    "Quote the exact selected phrase; prefix/suffix provide context.",
                    None,
                )
            })?;
        }
        if let Some(end_quote) = &request.end_quote {
            end_quote.validate().map_err(|reason| {
                invalid_params(
                    reason,
                    "Quote the exact phrase in the end block; prefix/suffix provide context.",
                    None,
                )
            })?;
        }
        let thread = self
            .state
            .store
            .create_comment_thread(
                &doc_id,
                NewCommentThread {
                    block_id,
                    end_block_id,
                    anchor_blocks,
                    author_type: CommentAuthorType::Agent,
                    author: actor,
                    body: request.body,
                    quote: request.quote,
                    end_quote: request.end_quote,
                    anchor_snapshot: request.anchor_snapshot,
                },
            )
            .await
            .map_err(mcp_error)?;
        self.state.publish_comment_updated(&thread).await;
        let _next_hint = format!(
            "Comment thread `{}` created without a revision change. Writers should reply with reply_comment after editing.",
            thread.thread_id
        );
        Ok(Json(AddCommentReceipt { thread, _next_hint }))
    }

    pub async fn exec_reply_comment(
        &self,
        context: RequestContext,
        request: ReplyCommentParams,
    ) -> Result<Json<ReplyCommentReceipt>, ErrorData> {
        let actor = context.actor;
        let doc_id = parse_doc_id(&request.doc_id)?;
        let addressed_rev = request.addressed_rev.map(parse_revision).transpose()?;
        let thread = self
            .state
            .store
            .reply_comment_thread(
                &doc_id,
                &request.thread_id,
                NewCommentReply {
                    author_type: CommentAuthorType::Agent,
                    author: actor,
                    body: request.body,
                    mark_addressed: request.mark_addressed,
                    addressed_rev,
                },
            )
            .await
            .map_err(mcp_error)?;
        self.state.publish_comment_updated(&thread).await;
        let _next_hint = match thread.status {
            CommentStatus::Addressed => format!(
                "Thread marked addressed at revision {}. Only a human can archive it through PATCH /api/docs/{doc_id}/comments/{{thread_id}}.",
                thread
                    .addressed_rev
                    .map(|revision| revision.get())
                    .unwrap_or(0)
            ),
            CommentStatus::Archived => {
                "Reply recorded on an archived thread; the thread stays archived.".to_string()
            }
            CommentStatus::Open => {
                "Reply recorded. Set mark_addressed=true after the fix to advance the thread."
                    .to_string()
            }
        };
        Ok(Json(ReplyCommentReceipt { thread, _next_hint }))
    }

    async fn apply_with_reload<F>(
        &self,
        doc_id: &DocId,
        actor: &str,
        summary: &str,
        mut build_ops: F,
    ) -> Result<(ApplyResult, ReportDocument), ErrorData>
    where
        F: FnMut(&ReportDocument) -> Result<Vec<PatchOp>, ErrorData>,
    {
        for attempt in 0..4 {
            let document = self
                .state
                .store
                .get_document(doc_id)
                .await
                .map_err(mcp_error)?;
            let ops = build_ops(&document)?;
            match self
                .state
                .store
                .apply_operations(doc_id, document.revision, actor, summary, &ops)
                .await
            {
                Ok(result) => {
                    let updated = self
                        .state
                        .store
                        .get_document(doc_id)
                        .await
                        .map_err(mcp_error)?;
                    return Ok((result, updated));
                }
                Err(StoreError::RevisionAhead { .. } | StoreError::Conflict(_)) if attempt < 3 => {
                    tokio::task::yield_now().await;
                    continue;
                }
                Err(error) => return Err(mcp_error(error)),
            }
        }
        Err(invalid_params(
            "configuration update could not be rebased after 4 attempts",
            "Reload the document and retry set_cover/configure_*.",
            None,
        ))
    }
}

fn touched_blocks(ops: &[PatchOp], assigned: &[IdAssignment]) -> BTreeSet<BlockId> {
    let mut touched: BTreeSet<BlockId> = ops.iter().flat_map(PatchOp::touched_block_ids).collect();
    touched.extend(
        assigned
            .iter()
            .map(|assignment| assignment.assigned.clone()),
    );
    touched
}

fn split_scoped_diagnostics(
    diagnostics: Vec<Diagnostic>,
    touched: &BTreeSet<BlockId>,
) -> (Vec<Diagnostic>, Vec<Diagnostic>) {
    let scoped: Vec<Diagnostic> = if touched.is_empty() {
        diagnostics
    } else {
        diagnostics
            .into_iter()
            .filter(|diagnostic| {
                diagnostic
                    .block_id
                    .as_ref()
                    .is_some_and(|block_id| touched.contains(block_id))
            })
            .collect()
    };
    scoped
        .into_iter()
        .partition(|diagnostic| diagnostic.severity == Severity::Error)
}

/// Map a structured protocol failure into an MCP error that exposes stable recovery fields.
fn protocol_error(error: ProtocolError) -> ErrorData {
    let code = error.code();
    let hint = error.hint();
    let message = error.to_string();
    ErrorData::invalid_params(
        message.clone(),
        Some(json!({
            "code": code,
            "message": message,
            "hint": hint,
            "protocol_error": error,
        })),
    )
}

// rmcp moves params._meta into RequestContext.meta before tool extraction; it is
// not guaranteed to remain in the tool's Extensions map.
fn request_context(
    context: &rmcp::service::RequestContext<rmcp::RoleServer>,
) -> Result<RequestContext, ErrorData> {
    let parts = context.extensions.get::<Parts>().ok_or_else(|| {
        invalid_params(
            "MCP request is missing HTTP context",
            "Use the Quire streamable HTTP transport.",
            None,
        )
    })?;
    RequestContext::from_mcp(parts, Some(&context.meta)).map_err(|error| {
        invalid_params(
            error,
            "Use matching X-Session-Id/X-Actor headers or _meta.quire.{session_id,run_id,agent_id}.",
            None,
        )
    })
}

fn parse_doc_id(raw: &str) -> Result<DocId, ErrorData> {
    DocId::parse(raw).map_err(protocol_error)
}

fn parse_block_id(raw: &str) -> Result<BlockId, ErrorData> {
    BlockId::parse(raw).map_err(protocol_error)
}

fn parse_block_ids(raw: &[String]) -> Result<Vec<BlockId>, ErrorData> {
    raw.iter().map(|value| parse_block_id(value)).collect()
}

fn parse_revision(value: i64) -> Result<Revision, ErrorData> {
    Revision::new(value).map_err(protocol_error)
}

fn generate_doc_id(title: &str) -> Result<DocId, ErrorData> {
    let slug = slugify_title(title);
    let suffix = short_token(8);
    let prefix: String = slug.chars().take(48).collect();
    DocId::parse(format!("{prefix}-{suffix}")).map_err(protocol_error)
}

fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}'
    )
}

fn slugify_title(input: &str) -> String {
    let mut output = String::new();
    let mut pending_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !output.is_empty() {
                output.push('-');
            }
            pending_dash = false;
            for lower in ch.to_lowercase() {
                output.push(lower);
            }
        } else {
            pending_dash = true;
        }
        if output.len() >= 80 {
            break;
        }
    }
    let trimmed = output.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "report".to_string()
    } else {
        trimmed
    }
}

fn inline_source_upserts(
    sources: Option<IndexMap<SourceId, SourceInput>>,
) -> Result<Vec<SourceUpsert>, ErrorData> {
    let Some(sources) = sources else {
        return Ok(Vec::new());
    };
    Ok(sources
        .into_iter()
        .map(|(source_id, input)| SourceUpsert {
            source_id,
            source: input.entry,
        })
        .collect())
}

/// Trim, validate and de-duplicate preview IDs while preserving producer order.
fn normalize_preview_ids(raw: Vec<String>) -> Result<Vec<String>, ErrorData> {
    let mut seen = HashSet::new();
    let mut preview_ids = Vec::with_capacity(raw.len());
    for preview_id in raw {
        let preview_id = preview_id.trim();
        if preview_id.is_empty() {
            return Err(invalid_params(
                "preview_ids must not contain empty values",
                "Pass the Runtime tool_call_id of each ephemeral preview to clear.",
                None,
            ));
        }
        if seen.insert(preview_id.to_string()) {
            preview_ids.push(preview_id.to_string());
        }
    }
    Ok(preview_ids)
}

/// Publish an immediate cleanup frame when a no-op commit leaves previews behind.
async fn publish_ephemeral_clear(state: &AppState, doc_id: &DocId, preview_ids: &[String]) {
    if preview_ids.is_empty() {
        return;
    }
    let payload = EphemeralPayload::Clear {
        event: EphemeralClearEvent::new(doc_id.clone(), preview_ids.to_vec()),
    };
    if let Err(error) = state
        .store
        .bus()
        .publish_ephemeral(doc_id.as_str(), &payload)
        .await
    {
        tracing::warn!(%error, %doc_id, "failed to publish ephemeral clear");
    }
}

fn merge_page_chrome(
    current: Option<&PageChromeConfig>,
    request: &ConfigurePageChromeParams,
) -> PageChromeConfig {
    let mut chrome = current.cloned().unwrap_or_default();
    if let Some(header) = request.header.clone() {
        chrome.header = header;
    }
    if let Some(footer) = request.footer.clone() {
        chrome.footer = footer;
    }
    if let Some(hide) = request.hide_header_on_first_page {
        chrome.hide_header_on_first_page = hide;
    }
    if let Some(hide) = request.hide_footer_on_first_page {
        chrome.hide_footer_on_first_page = hide;
    }
    if let Some(heading_page_break) = request.heading_page_break {
        chrome.heading_page_break = heading_page_break;
    }
    chrome
}

#[cfg(test)]
mod tests {
    use super::*;
    use quire_protocol::PageChromeSlots;

    #[test]
    fn slugify_title_drops_non_ascii_characters() {
        assert_eq!(slugify_title("Q3 Revenue 报告"), "q3-revenue");
        assert_eq!(slugify_title("你好"), "report");
        assert_eq!(slugify_title("Report_2026"), "report-2026");
        assert_eq!(slugify_title("!!!"), "report");
    }

    #[test]
    fn generated_doc_ids_start_with_ascii_letters() {
        let generated = generate_doc_id("你好");
        assert!(generated.is_ok());
        if let Ok(doc_id) = generated {
            assert!(doc_id.as_str().starts_with("report-"));
        }
    }

    #[test]
    fn page_chrome_merges_only_supplied_fields() {
        let current = PageChromeConfig {
            header: PageChromeSlots {
                left: "{kicker}".into(),
                center: String::new(),
                right: "{title}".into(),
            },
            footer: PageChromeSlots {
                left: "{title}".into(),
                center: String::new(),
                right: "第 {page} 页".into(),
            },
            hide_header_on_first_page: true,
            hide_footer_on_first_page: false,
            heading_page_break: quire_protocol::HeadingPageBreak::H1,
        };
        let request = ConfigurePageChromeParams {
            doc_id: "doc".into(),
            header: None,
            footer: Some(PageChromeSlots {
                left: String::new(),
                center: "— {page} / {total} —".into(),
                right: String::new(),
            }),
            hide_header_on_first_page: Some(false),
            hide_footer_on_first_page: None,
            heading_page_break: None,
            base_revision: None,
        };
        let merged = merge_page_chrome(Some(&current), &request);
        assert_eq!(merged.header.left, "{kicker}");
        assert_eq!(merged.footer.center, "— {page} / {total} —");
        assert!(!merged.hide_header_on_first_page);
        assert!(!merged.hide_footer_on_first_page);
        assert_eq!(
            merged.heading_page_break,
            quire_protocol::HeadingPageBreak::H1
        );

        let request = ConfigurePageChromeParams {
            heading_page_break: Some(quire_protocol::HeadingPageBreak::H1AndH2),
            ..request
        };
        let merged = merge_page_chrome(Some(&current), &request);
        assert_eq!(
            merged.heading_page_break,
            quire_protocol::HeadingPageBreak::H1AndH2
        );
    }
}
