use super::*;

#[tool_router(router = mutations_router, vis = "pub(crate)")]
impl QuireMcpServer {
    // ---------------------------------------------------------------------
    // Tool 7: apply operations
    // ---------------------------------------------------------------------
    /// Atomically apply a batch of block mutations (`ops`) and optional citation `sources` to a report using Smart CAS optimistic concurrency.
    ///
    /// Operation selection & comment-anchor preservation:
    /// - Prefer `patch_block` (`{"type":"patch_block","block_id":"...","patch":{...}}`) when updating an existing block so review comment threads stay anchored to `block_id`.
    /// - Use `insert_after`, `insert_before`, or `append` to add new blocks with semantic prefix-scoped IDs (e.g. `ch2_overview`, `ch2_rev_table`).
    /// - Use `replace_range` or `delete_blocks` only when restructuring or removing blocks; deleting a block detaches any review comment thread anchored to it.
    /// - When `preview_echarts` + `commit_chart` are in your toolset, author new ECharts blocks through `preview_echarts` -> `commit_chart` instead of embedding unverified raw chart options here.
    ///
    /// Concurrency & receipt handling:
    /// - Pass the `expected_revision` you read from `get_document_digest` / `read_blocks`. Smart CAS automatically rebases non-overlapping edits across parallel subagents; on a conflict error, re-read only the conflicting block IDs and retry.
    /// - Always inspect `blocking_errors` and `inline_warnings` in the receipt and fix any errors on your blocks before finishing.
    #[tool(name = "apply_operations")]
    pub async fn apply_operations(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<ApplyOperationsParams>,
    ) -> Result<Json<ApplyOperationsReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_apply_operations(context, request).await.map(Json)
    }

    // ---------------------------------------------------------------------
    // Tool 8: rollback
    // ---------------------------------------------------------------------
    /// Restore a report's content to an earlier `target_revision` by committing a new monotonic forward revision (`current_rev + 1`).
    ///
    /// Guardrails:
    /// - Rollback is a whole-document operation that undoes all changes committed after `target_revision`, including concurrent edits from sibling subagents.
    /// - Prefer surgical `apply_operations` (`patch_block`, `delete_blocks`, `replace_range`) when fixing localized mistakes; use `rollback_revision` only when the user explicitly requests a revision rollback or a broad batch corrupted document structure.
    #[tool(name = "rollback_revision")]
    pub async fn rollback_revision(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<RollbackParams>,
    ) -> Result<Json<RollbackReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_rollback_revision(context, request)
            .await
            .map(Json)
    }

    // ---------------------------------------------------------------------
    // Tool 9: upsert sources
    // ---------------------------------------------------------------------
    /// Register or update bibliography `SourceEntry` records on a report with automatic URL and normalized-title deduplication.
    ///
    /// Citation integrity rules:
    /// - Register ONLY real sources that you actually retrieved and read during research; never fabricate URLs, DOIs, authors, or evidence excerpts.
    /// - `excerpt` is optional and intended for human readers. For a natural-language document, include only a short verbatim passage from that same document when it directly supports a cited claim; it does not need to cover every claim made from the source.
    /// - Omit `excerpt` for structured database/API/tool records or when no meaningful original passage exists. Never replace it with full query results, multi-document compilations, derived analysis, rankings, internal IDs, or editorial notes. Use `note` for a concise human-readable scope, time window, and statistical caliber.
    /// - Always inspect `canonical_id_map` in the receipt: if a duplicate URL or title merged into an existing source ID, cite the mapped canonical ID in your prose (`[^id]`) and block `source_ids`.
    /// - You can also register sources atomically alongside block edits via the `sources` map on `apply_operations`.
    /// - In chat replies, place `<citation id="SOURCE_ID"/>` immediately after the claim it supports to render an interactive citation badge and a collapsible Sources drawer.
    #[tool(name = "upsert_sources")]
    pub async fn upsert_sources(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<UpsertSourcesParams>,
    ) -> Result<Json<UpsertSourcesReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_upsert_sources(context, request).await.map(Json)
    }

    /// Remove unused bibliography sources from a report (`source_ids` and/or `prune_unused: true`).
    /// Refuses to delete any source still cited by a block or inline `[^id]` marker.
    #[tool(name = "delete_sources")]
    pub async fn delete_sources(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<DeleteSourcesParams>,
    ) -> Result<Json<DeleteSourcesReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_delete_sources(context, request).await.map(Json)
    }

    // ---------------------------------------------------------------------
    // Tool 10: list sources
    // ---------------------------------------------------------------------
    /// List registered bibliography sources, including optional human-readable evidence excerpts.
    ///
    /// Call this before authoring citations in a shared report to reuse existing canonical `SourceId`s and inspect provenance. When an excerpt is present, verify it against the cited claim; when it is absent, use the original document or structured source data instead. Missing excerpts are valid.
    /// Passing any unregistered ID in `source_ids` returns an error listing all valid registered source IDs.
    #[tool(name = "list_sources", annotations(read_only_hint = true))]
    pub async fn list_sources(
        &self,
        Parameters(request): Parameters<ListSourcesParams>,
    ) -> Result<Json<ListSourcesReceipt>, ErrorData> {
        QuireReportService::list_sources(self, request)
            .await
            .map(Json)
    }

    // ---------------------------------------------------------------------
    // Tool 11: set cover
    // ---------------------------------------------------------------------
    /// Partially update the report cover page configuration (`enabled`, `variant`, `title`, `subtitle`, `authors`, `date`, `meta`, or `custom_html`).
    ///
    /// Omitted fields inside `cover_patch` preserve their existing stored values; supplying an empty string for `subtitle`, `date`, or `custom_html` clears that optional field.
    #[tool(name = "set_cover")]
    pub async fn set_cover(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<SetCoverParams>,
    ) -> Result<Json<SetCoverReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_set_cover(context, request).await.map(Json)
    }

    // ---------------------------------------------------------------------
    // Tool 12: configure TOC
    // ---------------------------------------------------------------------
    /// Configure the auto-generated Table of Contents (`enabled`, `title`, `max_depth`, visual `variant`, and page breaks).
    ///
    /// The TOC is derived automatically from document `heading` blocks up to `max_depth` (`1..=3`); never author a manual TOC block in the document body.
    #[tool(name = "configure_toc")]
    pub async fn configure_toc(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<ConfigureTocParams>,
    ) -> Result<Json<ConfigureTocReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_configure_toc(context, request).await.map(Json)
    }

    // ---------------------------------------------------------------------
    // Tool 13: configure references
    // ---------------------------------------------------------------------
    /// Configure the auto-generated bibliography section (`enabled`, `title`, citation `style`, and `page_break_before`).
    ///
    /// The receipt includes `orphan_sources` (registered source IDs not cited by any block or inline `[^id]` marker) so you can cite or clean them up before finalizing.
    #[tool(name = "configure_references")]
    pub async fn configure_references(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<ConfigureReferencesParams>,
    ) -> Result<Json<ConfigureReferencesReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_configure_references(context, request)
            .await
            .map(Json)
    }

    // ---------------------------------------------------------------------
    // Tool 14: configure page chrome
    // ---------------------------------------------------------------------
    /// Configure A4 print/paged running headers, footers, first-page visibility, and automatic heading page-break policy (`none`, `h1`, or `h1_and_h2`).
    ///
    /// Omitted optional fields keep their current settings. Header and footer slot templates support `{page}`, `{total}`, `{title}`, `{subtitle}`, `{date}`, `{kicker}`, and `{authors}` placeholders.
    #[tool(name = "configure_page_chrome")]
    pub async fn configure_page_chrome(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<ConfigurePageChromeParams>,
    ) -> Result<Json<ConfigurePageChromeReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_configure_page_chrome(context, request)
            .await
            .map(Json)
    }

    // ---------------------------------------------------------------------
    // Tool 15: update style and CSS
    // ---------------------------------------------------------------------
    /// Update the report's visual theme preset, `--rpt-*` CSS custom property tokens, or scoped `.report-root` custom CSS.
    ///
    /// Call `get_protocol_guide("citations_and_css")` before writing custom CSS rules. External `@import`, remote `url(...)`, and unscoped global selectors are rejected by validation; follow up with `preview_report` to verify visual contrast and A4 layout.
    #[tool(name = "update_style_and_css")]
    pub async fn update_style_and_css(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<UpdateStyleParams>,
    ) -> Result<Json<UpdateStyleReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_update_style_and_css(context, request)
            .await
            .map(Json)
    }

    // ---------------------------------------------------------------------
    // Tool 16: lint
    // ---------------------------------------------------------------------
    /// Run structural, citation, chart/table, and prose quality checks across the report (`QR1xx`–`QR4xx`).
    ///
    /// Verification contract:
    /// - Call `lint_document` during the `VERIFY` stage after authoring or editing blocks.
    /// - Resolve all `error`-severity diagnostics before declaring the report complete.
    /// - Review `warning` diagnostics and `suppressed_blocks` (`block_id -> lint_ignore` codes); only suppress a rule via a block's `lint_ignore` when `suppressible: true` and the exception is deliberate.
    #[tool(name = "lint_document", annotations(read_only_hint = true))]
    pub async fn lint_document(
        &self,
        Parameters(request): Parameters<LintParams>,
    ) -> Result<Json<LintReceipt>, ErrorData> {
        QuireReportService::lint_document(self, request)
            .await
            .map(Json)
    }

    // ---------------------------------------------------------------------
    // Tool 17: list comments
    // ---------------------------------------------------------------------
    /// List side-channel review comment threads anchored to blocks in a report.
    ///
    /// By default returns `open` and `addressed` threads (excluding `archived` unless `status: "archived"` is explicitly requested), including each thread's `block_id`, `anchor_blocks`, text `quote`, and `anchor_detached` flag.
    #[tool(name = "list_comments", annotations(read_only_hint = true))]
    pub async fn list_comments(
        &self,
        Parameters(request): Parameters<ListCommentsParams>,
    ) -> Result<Json<ListCommentsReceipt>, ErrorData> {
        QuireReportService::list_comments(self, request)
            .await
            .map(Json)
    }

    // ---------------------------------------------------------------------
    // Tool 18: add comment
    // ---------------------------------------------------------------------
    /// Create a new side-channel review comment thread anchored to one or more existing blocks without incrementing the document revision.
    ///
    /// Use this in reviewer/QA workflows when flagging issues for another agent or human without directly mutating the block; when you are the authoring agent asked to fix content, edit the block via `apply_operations` instead.
    #[tool(name = "add_comment")]
    pub async fn add_comment(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<AddCommentParams>,
    ) -> Result<Json<AddCommentReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_add_comment(context, request).await.map(Json)
    }

    // ---------------------------------------------------------------------
    // Tool 19: reply comment
    // ---------------------------------------------------------------------
    /// Reply to an existing review comment thread and optionally transition its status from `open` to `addressed`.
    ///
    /// Workflow:
    /// 1. First fix the underlying block(s) with `apply_operations` (`patch_block` preserves the thread anchor) and note the returned `new_revision`.
    /// 2. Then call `reply_comment` with a concise summary of the fix, `mark_addressed: true`, and `addressed_rev: new_revision`.
    /// Agents cannot archive threads (`archived` is reserved for human reviewers).
    #[tool(name = "reply_comment")]
    pub async fn reply_comment(
        &self,
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<ReplyCommentParams>,
    ) -> Result<Json<ReplyCommentReceipt>, ErrorData> {
        let context = request_context(&context)?;
        self.exec_reply_comment(context, request).await.map(Json)
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
