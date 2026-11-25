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
    ) -> Result<String, ErrorData> {
        let receipt = QuireReportService::get_protocol_guide(self, request).await?;
        receipt.render_markdown()
    }

    // ---------------------------------------------------------------------
    // Tool 2: create report
    // ---------------------------------------------------------------------
    /// Initialize a new Quire report document at revision 1 and bind it to the active session.
    ///
    /// When to use:
    /// - Only when starting a brand-new report and the session does not already bind a `doc_id`.
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
        self.exec_create_report(context, request).await.map(Json)
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
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<ListReportsParams>,
    ) -> Result<String, ErrorData> {
        let context = request_context(&context)?;
        let receipt = QuireReportService::list_reports(self, context, request).await?;
        receipt.render_markdown()
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
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<DigestParams>,
    ) -> Result<String, ErrorData> {
        let context = request_context(&context)?;
        let receipt = QuireReportService::get_document_digest(self, context, request).await?;
        receipt.render_markdown()
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
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<ReadMarkdownParams>,
    ) -> Result<String, ErrorData> {
        let context = request_context(&context)?;
        let receipt = QuireReportService::read_as_markdown(self, context, request).await?;
        receipt.render_markdown()
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
        context: rmcp::service::RequestContext<rmcp::RoleServer>,
        Parameters(request): Parameters<ReadBlocksParams>,
    ) -> Result<Json<ReadBlocksReceipt>, ErrorData> {
        let context = request_context(&context)?;
        QuireReportService::read_blocks(self, context, request)
            .await
            .map(Json)
    }
}
