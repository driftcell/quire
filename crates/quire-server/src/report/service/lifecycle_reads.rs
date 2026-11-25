use super::*;

impl QuireReportService {
    // ---------------------------------------------------------------------
    // Tool 1: protocol guide
    // ---------------------------------------------------------------------
    /// Disclose the Quire report protocol specification, schema patterns, and examples for one topic.
    ///
    /// Call this on-demand before authoring complex structures rather than guessing field shapes:
    /// - `quickstart`: `INIT -> SCHEMA -> AUTHOR -> VERIFY` state machine, semantic `block_id` rules, and patch envelopes.
    /// - `tables_and_charts`: recursive `ColumnNode` table headers, row cells, and ECharts 6.0 `option` rules.
    /// - `citations_and_css`: `SourceEntry` registration, inline `[^src_id]` vs block `source_ids`, and `.report-root` CSS tokens.
    /// - `parallel_workflow`: disjoint `block_id` prefix ownership (`ch1_*`, `ch2_*`) and Smart CAS auto-rebase rules.
    ///
    /// Read-only; safe to call at any point in the turn.
    pub async fn get_protocol_guide(
        &self,
        request: GuideParams,
    ) -> Result<GuideReceipt, ErrorData> {
        let receipt = guide(request.topic);
        let workflow = guide_workflow();
        Ok(GuideReceipt {
            topic: receipt.topic,
            guide: receipt.guide,
            recommended_tools: receipt.recommended_tools,
            workflow,
            _next_hint: receipt._next_hint,
        })
    }

    // ---------------------------------------------------------------------
    // Tool 3: list reports
    // ---------------------------------------------------------------------
    /// Search or list existing report documents with their latest revision, block count, and bound `session_id`.
    ///
    /// Use this to locate an existing report by title or `doc_id` substring before deciding whether to call `get_document_digest` or `create_report`.
    /// Read-only; does not load block bodies.
    pub async fn list_reports(
        &self,
        context: RequestContext,
        request: ListReportsParams,
    ) -> Result<ListReportsReceipt, ErrorData> {
        let list = self
            .state
            .list_reports_authorized(
                &context,
                request.query.as_deref(),
                request.limit.unwrap_or(20),
                0,
            )
            .await
            .map_err(mcp_error)?;
        let _next_hint = if list.reports.is_empty() {
            "Call create_report to start a new report.".to_string()
        } else {
            "Call get_document_digest(doc_id) for the report you want to edit.".to_string()
        };
        let reports = list
            .reports
            .into_iter()
            .map(ReportSummaryView::from)
            .collect();
        Ok(ListReportsReceipt {
            reports,
            total: list.total,
            limit: list.limit,
            offset: list.offset,
            _next_hint,
        })
    }

    // ---------------------------------------------------------------------
    // Tool 4: document digest
    // ---------------------------------------------------------------------
    /// Return a low-token structural outline of a report (`current_rev`, theme/TOC summary, character-budget metrics, and per-block metadata).
    ///
    /// Read routing (cheapest to most detailed):
    /// 1. `get_document_digest` (this tool): ALWAYS call first to discover `current_rev`, block IDs, heading structure, 30-char previews, and `prose_char_count` / `prose_char_delta`.
    /// 2. `read_as_markdown`: read human-readable prose and tables for one section (`section_id`) or specific blocks (`block_ids`).
    /// 3. `read_blocks`: inspect raw canonical Block AST JSON (for example `TableBlock` `ColumnNode` trees or `EchartsBlock` `option`) before surgical patching.
    pub async fn get_document_digest(
        &self,
        context: RequestContext,
        request: DigestParams,
    ) -> Result<DigestReceipt, ErrorData> {
        let doc_id = parse_doc_id(&request.doc_id)?;
        self.state
            .authorize_doc_access(&context, &doc_id, false)
            .await
            .map_err(mcp_error)?;
        let document = self
            .state
            .store
            .get_document(&doc_id)
            .await
            .map_err(mcp_error)?;
        if let (Some(stored), Some(requested)) =
            (document.target_prose_chars, request.target_prose_chars)
            && stored != requested
        {
            return Err(invalid_params(
                format!("This report already has an agreed target of {stored} characters."),
                "Omit target_prose_chars to read the stored agreement.",
                Some(McpErrorDetails::StoredTargetProseChars {
                    stored_target_prose_chars: stored.get(),
                }),
            ));
        }
        let digest = Self::build_digest(
            &document,
            request.query.as_deref(),
            request.target_prose_chars,
        );
        let _next_hint = if digest.blocks.is_empty() {
            "No blocks matched this query. Omit query to list the document outline.".to_string()
        } else {
            "Pass a heading's id to read_as_markdown(section_id) to read the section.".to_string()
        };
        Ok(DigestReceipt { digest, _next_hint })
    }

    // ---------------------------------------------------------------------
    // Tool 5: read as markdown
    // ---------------------------------------------------------------------
    /// Project a report (or a scoped section / block subset) into Markdown annotated with `<!-- block:ID | type:... -->` anchors.
    ///
    /// Scoping & token discipline:
    /// - Prefer `section_id` (an exact heading `block_id` from `get_document_digest`) or `block_ids` to read only the chapter or blocks you are reviewing or editing.
    /// - `section_id` and `block_ids` are mutually exclusive; omit both only when performing a full-document editorial pass.
    /// - Does not return raw ECharts `option` JSON or recursive `ColumnNode` definitions; use `read_blocks` when you need raw AST fields for those blocks.
    pub async fn read_as_markdown(
        &self,
        context: RequestContext,
        request: ReadMarkdownParams,
    ) -> Result<ReadMarkdownReceipt, ErrorData> {
        if [
            request.section_id.is_some(),
            request.section_heading.is_some(),
            request.block_ids.is_some(),
        ]
        .into_iter()
        .filter(|v| *v)
        .count()
            > 1
        {
            return Err(invalid_params(
                "Conflicting read selectors.",
                "Use section_id for a whole section or block_ids for individual blocks.",
                None,
            ));
        }
        let markdown_options = MarkdownOptions {
            include_chart_data: request.include_chart_data.unwrap_or(true),
            include_front_matter: true,
        };
        let doc_id = parse_doc_id(&request.doc_id)?;
        self.state
            .authorize_doc_access(&context, &doc_id, false)
            .await
            .map_err(mcp_error)?;
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
        } else if let Some(id) = &request.section_id {
            projector.render_section_by_id(id).map_err(|error| {
                invalid_params(
                    error.to_string(),
                    "Copy the id of a heading from get_document_digest.",
                    None,
                )
            })?
        } else if let Some(section) = &request.section_heading {
            projector.render_section(section).map_err(|error| {
                invalid_params(
                    error.to_string(),
                    "Pass section_id with the exact heading id from get_document_digest.",
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
        Ok(ReadMarkdownReceipt {
            source: format!("doc:{doc_id}"),
            doc_id: document.doc_id.to_string(),
            revision: document.revision.get(),
            markdown_chars: markdown.chars().count(),
            markdown,
            _next_hint,
        })
    }

    // ---------------------------------------------------------------------
    // Tool 6: read blocks
    // ---------------------------------------------------------------------
    /// Fetch the full canonical Block AST JSON for specific `block_ids` alongside the current document `revision`.
    ///
    /// When to use:
    /// - Before patching complex structured blocks (`table`, `echarts`, `custom_html`, `kpi_grid`, `callout`) where you need exact field names, `ColumnNode` trees, `option` payloads, or `lint_ignore` arrays.
    /// - For reading narrative prose or checking section flow, prefer `read_as_markdown(section_id)` to save context tokens.
    /// - Any unknown `block_id` fails the call with the list of valid document block IDs.
    pub async fn read_blocks(
        &self,
        context: RequestContext,
        request: ReadBlocksParams,
    ) -> Result<ReadBlocksReceipt, ErrorData> {
        let doc_id = parse_doc_id(&request.doc_id)?;
        let block_ids = parse_block_ids(&request.block_ids)?;
        self.state
            .authorize_doc_access(&context, &doc_id, false)
            .await
            .map_err(mcp_error)?;
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
            let valid: Vec<String> = document
                .blocks
                .iter()
                .map(|block| block.id.to_string())
                .collect();
            return Err(invalid_params(
                format!("block IDs not found: {}", missing.join(", ")),
                format!("Valid block_ids: {}", valid.join(", ")),
                Some(McpErrorDetails::MissingBlockIds {
                    missing,
                    valid_block_ids: valid,
                }),
            ));
        }
        Ok(ReadBlocksReceipt {
            doc_id: document.doc_id.to_string(),
            revision: document.revision.get(),
            blocks,
            _next_hint: "Patch specific fields with apply_operations using the current revision."
                .into(),
        })
    }
}
