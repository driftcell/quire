use super::*;

impl QuireReportService {
    // ---------------------------------------------------------------------
    // Tool 10: list sources
    // ---------------------------------------------------------------------
    /// List registered bibliography sources, including optional human-readable evidence excerpts.
    ///
    /// Call this before authoring citations in a shared report to reuse existing canonical `SourceId`s and inspect provenance. When an excerpt is present, verify it against the cited claim; when it is absent, use the original document or structured source data instead. Missing excerpts are valid.
    /// Passing any unregistered ID in `source_ids` returns an error listing all valid registered source IDs.
    pub async fn list_sources(
        &self,
        context: RequestContext,
        request: ListSourcesParams,
    ) -> Result<ListSourcesReceipt, ErrorData> {
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
            "Match each citation to a specific claim. Use an excerpt only when a short original passage is useful; otherwise verify the original document or structured source data. Fetch original pages before relying on search-result snippets.".to_string()
        };
        Ok(ListSourcesReceipt {
            doc_id: document.doc_id.to_string(),
            revision: document.revision.get(),
            count: sources.len(),
            sources,
            _next_hint,
        })
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
    pub async fn lint_document(
        &self,
        context: RequestContext,
        request: LintParams,
    ) -> Result<LintReceipt, ErrorData> {
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
            return Ok(LintReceipt {
                ok: true,
                rev,
                summary: None,
                suppressed_blocks: BTreeMap::new(),
                diagnostics: Vec::new(),
                _next_hint: None,
            });
        }
        let ok = !report.has_errors();
        let _next_hint = lint_hint::lint_document_next_hint(&report);
        Ok(LintReceipt {
            ok,
            rev,
            summary: Some(report.summary),
            suppressed_blocks,
            diagnostics: report.diagnostics,
            _next_hint: Some(_next_hint),
        })
    }

    // ---------------------------------------------------------------------
    // Tool 17: list comments
    // ---------------------------------------------------------------------
    /// List side-channel review comment threads anchored to blocks in a report.
    ///
    /// By default returns `open` and `addressed` threads (excluding `archived` unless `status: "archived"` is explicitly requested), including each thread's `block_id`, `anchor_blocks`, text `quote`, and `anchor_detached` flag.
    pub async fn list_comments(
        &self,
        context: RequestContext,
        request: ListCommentsParams,
    ) -> Result<ListCommentsReceipt, ErrorData> {
        let doc_id = parse_doc_id(&request.doc_id)?;
        self.state
            .authorize_doc_access(&context, &doc_id, false)
            .await
            .map_err(mcp_error)?;
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
            "Proceed with drafting or call lint_document when ready.".to_string()
        } else {
            "Reply with reply_comment(doc_id, thread_id, body, mark_addressed=true, addressed_rev=...) after fixing the block. Only humans can archive a thread.".to_string()
        };
        Ok(ListCommentsReceipt {
            doc_id: doc_id.to_string(),
            comments,
            count,
            _next_hint,
        })
    }
}
