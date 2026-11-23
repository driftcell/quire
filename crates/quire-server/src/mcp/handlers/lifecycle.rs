use super::*;

#[tool_router(router = lifecycle_router, vis = "pub(crate)")]
impl QuireMcpServer {
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
    #[tool(name = "get_protocol_guide", annotations(read_only_hint = true))]
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
    /// Initialize a new Quire report document at revision 1 and bind it to the active session.
    ///
    /// When to use:
    /// - Only when starting a brand-new report and `<runtime_session_state>` does not already bind a `doc_id`.
    /// - When a session already has a bound report (or you need to check existing reports first), call `get_document_digest` or `list_reports` instead of creating a duplicate document.
    ///
    /// Key contracts:
    /// - Pass `target_prose_chars` whenever the user specifies or agrees to a target character count (e.g., `30000` for "约 3 万字"); `get_document_digest` tracks `prose_char_count` and `prose_char_delta` against this stored agreement.
    /// - When seeding `initial_blocks` (such as chapter headings for parallel subagents), every block `id` must be a unique semantic slug (`ch1_heading`, `exec_summary`), never a UUID.
    /// - CJK titles automatically localize default TOC (`目录`) and bibliography (`参考文献`) headings.
    #[tool(name = "create_report")]
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
    /// Search or list existing report documents with their latest revision, block count, and bound `session_id`.
    ///
    /// Use this to locate an existing report by title or `doc_id` substring before deciding whether to call `get_document_digest` or `create_report`.
    /// Read-only; does not load block bodies.
    #[tool(name = "list_reports", annotations(read_only_hint = true))]
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
    /// Return a low-token structural outline of a report (`current_rev`, theme/TOC summary, character-budget metrics, and per-block metadata).
    ///
    /// Read routing (cheapest to most detailed):
    /// 1. `get_document_digest` (this tool): ALWAYS call first to discover `current_rev`, block IDs, heading structure, 30-char previews, and `prose_char_count` / `prose_char_delta`.
    /// 2. `read_as_markdown`: read human-readable prose and tables for one section (`section_id`) or specific blocks (`block_ids`).
    /// 3. `read_blocks`: inspect raw canonical Block AST JSON (for example `TableBlock` `ColumnNode` trees or `EchartsBlock` `option`) before surgical patching.
    #[tool(name = "get_document_digest", annotations(read_only_hint = true))]
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
            format!(
                "rev={}; pass a heading's id to read_as_markdown(section_id) to read the section.",
                digest.current_rev
            )
        };
        Ok(Json(DigestReceipt { digest, _next_hint }))
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
    #[tool(name = "read_as_markdown", annotations(read_only_hint = true))]
    pub async fn read_as_markdown(
        &self,
        Parameters(request): Parameters<ReadMarkdownParams>,
    ) -> Result<Json<ReadMarkdownReceipt>, ErrorData> {
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
    /// Fetch the full canonical Block AST JSON for specific `block_ids` alongside the current document `revision`.
    ///
    /// When to use:
    /// - Before patching complex structured blocks (`table`, `echarts`, `custom_html`, `kpi_grid`, `callout`) where you need exact field names, `ColumnNode` trees, `option` payloads, or `lint_ignore` arrays.
    /// - For reading narrative prose or checking section flow, prefer `read_as_markdown(section_id)` to save context tokens.
    /// - Any unknown `block_id` fails the call with the list of valid document block IDs.
    #[tool(name = "read_blocks", annotations(read_only_hint = true))]
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
        Ok(Json(ReadBlocksReceipt {
            doc_id: document.doc_id.to_string(),
            revision: document.revision.get(),
            blocks,
            _next_hint: "Patch specific fields with apply_operations using the current revision."
                .into(),
        }))
    }

    // ---------------------------------------------------------------------
    // Digest helpers
    // ---------------------------------------------------------------------
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
    fn build_digest(
        document: &ReportDocument,
        query: Option<&str>,
        fallback_target: Option<NonZeroU32>,
    ) -> DocumentDigest {
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
            .map(Self::block_digest)
            .collect::<Vec<_>>();
        let paragraph_count = document
            .blocks
            .iter()
            .filter(|block| matches!(&block.body, BlockKind::Paragraph { .. }))
            .count();
        let prose_char_count: usize = document
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
        let target_prose_chars = document.target_prose_chars.or(fallback_target);
        DocumentDigest {
            title: document.title(),
            current_rev: document.revision.get(),
            theme: document.theme.summary(),
            toc: document.toc.clone(),
            block_count: document.blocks.len(),
            source_count: document.sources.len(),
            paragraph_count,
            prose_char_count,
            target_prose_chars,
            prose_char_delta: target_prose_chars.map(|target| {
                i64::try_from(prose_char_count).unwrap_or(i64::MAX) - i64::from(target.get())
            }),
            visual_block_count,
            cited_source_count,
            blocks,
        }
    }
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
        document.target_prose_chars = request.target_prose_chars;
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
                Some(McpErrorDetails::DuplicateBlockIds {
                    duplicate_block_ids: duplicates,
                }),
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
        let digest = Self::build_digest(&stored, None, None);
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
}
