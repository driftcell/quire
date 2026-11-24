use std::{collections::BTreeMap, num::NonZeroU32};

use indexmap::IndexMap;
use quire_protocol::SourceId;
use quire_server::report as contracts;

use super::*;

/// Preserve producer order while advertising the same object-map schema as the shared params.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(transparent)]
#[schemars(with = "BTreeMap<SourceId, contracts::SourceInput>")]
pub(crate) struct SourceInputMap(IndexMap<SourceId, contracts::SourceInput>);

/// Disclose the Quire report protocol specification, schema patterns, and examples for one topic.
/// Call on demand before authoring complex structures: `quickstart` covers workflow and patches,
/// `tables_and_charts` covers recursive columns and ECharts options, `citations_and_css` covers
/// sources and scoped CSS, and `parallel_workflow` covers disjoint IDs and Smart CAS rebase.
#[rig_tool(name = "get_protocol_guide")]
pub(crate) async fn get_protocol_guide_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Protocol topic to disclose.
    topic: quire_server::guide::GuideTopic,
) -> Result<contracts::GuideReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    runtime
        .report
        .get_protocol_guide(contracts::GuideParams { topic })
        .await
        .map_err(mcp_error_to_tool_error)
}

/// Initialize a new report at revision 1 and bind it to the active session.
/// Use only when `<runtime_session_state>` has no bound `doc_id`; inspect an existing binding with
/// `get_document_digest` or `list_reports` instead of creating a duplicate. Pass the agreed prose
/// target when one exists. Initial block IDs must be unique semantic slugs, never UUIDs.
#[allow(
    clippy::too_many_arguments,
    reason = "The flat public tool schema requires these arguments; grouping them would change model calls."
)]
#[rig_tool(name = "create_report")]
pub(crate) async fn create_report_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Human-readable, non-empty report title.
    title: String,
    /// Agreed visible paragraph character target, excluding headings, tables and charts.
    target_prose_chars: Option<NonZeroU32>,
    /// Optional visual theme preset.
    theme_preset: Option<String>,
    /// Whether to render the cover (`true` by default).
    enable_cover: Option<bool>,
    /// Whether to render the automatic table of contents (`true` by default).
    enable_toc: Option<bool>,
    /// Optional initial Block AST entries with unique semantic IDs.
    initial_blocks: Option<Vec<Block>>,
    /// Optional stable idempotency key, automatically scoped to this session.
    idempotency_key: Option<String>,
    /// Optional explicit document slug; omit to generate one from the title.
    doc_id: Option<String>,
    /// Optional cover subtitle.
    subtitle: Option<String>,
    /// Optional cover author or organization lines.
    authors: Option<Vec<String>>,
    /// Optional cover publication date.
    date: Option<String>,
    /// Optional cover key/value metadata badges.
    cover_meta: Option<BTreeMap<String, String>>,
) -> Result<contracts::CreateReportReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let request_context = in_process_request_context(context)?;
    runtime
        .report
        .exec_create_report(
            request_context,
            contracts::CreateReportParams {
                title,
                target_prose_chars,
                theme_preset,
                enable_cover,
                enable_toc,
                initial_blocks,
                idempotency_key,
                doc_id,
                subtitle,
                authors,
                date,
                cover_meta,
            },
        )
        .await
        .map_err(mcp_error_to_tool_error)
}

/// Search or list existing reports with their latest revision, block count and bound session.
/// Use before `get_document_digest` or `create_report` when an existing report may already exist.
/// Read-only; does not load block bodies.
#[rig_tool(name = "list_reports")]
pub(crate) async fn list_reports_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Optional case-insensitive substring matched against document ID and title.
    query: Option<String>,
    /// Maximum number of summaries (`1..=100`, defaults to 20).
    limit: Option<i64>,
) -> Result<contracts::ListReportsReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    runtime
        .report
        .list_reports(contracts::ListReportsParams { query, limit })
        .await
        .map_err(mcp_error_to_tool_error)
}

/// Return a low-token report outline with revision, configuration, prose metrics and block metadata.
/// Call this first for `current_rev` and exact block IDs. Use `read_as_markdown` for prose and tables,
/// then `read_blocks` only when raw canonical AST fields are needed for surgical patching.
#[rig_tool(name = "get_document_digest")]
pub(crate) async fn get_document_digest_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Target report `doc_id` from runtime state or `list_reports`.
    doc_id: String,
    /// Optional case-insensitive filter on block ID, type or visible text.
    query: Option<String>,
    /// Optional comparison target for older reports without a stored prose target.
    target_prose_chars: Option<NonZeroU32>,
) -> Result<contracts::DigestReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    runtime
        .report
        .get_document_digest(contracts::DigestParams {
            doc_id,
            query,
            target_prose_chars,
        })
        .await
        .map_err(mcp_error_to_tool_error)
}

/// Project a report, section or block subset into Markdown with exact block anchors.
/// Prefer `section_id` or `block_ids` to limit context; they are mutually exclusive. Omit both only
/// for a full-document editorial pass. Use `read_blocks` for raw ECharts options or table columns.
#[rig_tool(name = "read_as_markdown")]
pub(crate) async fn read_as_markdown_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Target report `doc_id`.
    doc_id: String,
    /// Exact heading block ID whose whole section should be projected.
    section_id: Option<BlockId>,
    /// Legacy heading-text selector; prefer the exact `section_id` when available.
    section_heading: Option<String>,
    /// Exact block IDs to project in document order; mutually exclusive with section selectors.
    block_ids: Option<Vec<String>>,
    /// Whether to expand ECharts series into Markdown data tables (`true` by default).
    include_chart_data: Option<bool>,
) -> Result<contracts::ReadMarkdownReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    runtime
        .report
        .read_as_markdown(contracts::ReadMarkdownParams {
            doc_id,
            section_id,
            section_heading,
            block_ids,
            include_chart_data,
        })
        .await
        .map_err(mcp_error_to_tool_error)
}

/// Fetch full canonical Block AST JSON for exact block IDs plus the current revision.
/// Use before patching complex structured blocks where exact fields, table columns, ECharts options
/// or lint-ignore arrays matter. Unknown IDs fail with the valid document block IDs.
#[rig_tool(name = "read_blocks")]
pub(crate) async fn read_blocks_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Target report `doc_id`.
    doc_id: String,
    /// Non-empty list of exact block IDs from `get_document_digest`.
    block_ids: Vec<String>,
) -> Result<contracts::ReadBlocksReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    runtime
        .report
        .read_blocks(contracts::ReadBlocksParams { doc_id, block_ids })
        .await
        .map_err(mcp_error_to_tool_error)
}

/// Atomically apply block mutations and optional citation sources using Smart CAS.
/// Prefer `patch_block` for existing blocks so comment anchors survive, and use semantic prefixed
/// IDs for new blocks. On conflict, re-read only conflicting IDs and retry with the latest revision.
/// Always inspect `blocking_errors` and `inline_warnings`. Use preview plus `commit_chart` for new
/// ECharts blocks instead of embedding unverified chart options.
#[rig_tool(
    name = "apply_operations",
    required(doc_id, expected_revision, summary, ops)
)]
pub(crate) async fn apply_operations_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Target report `doc_id`.
    doc_id: String,
    /// Base revision read from digest, Markdown or raw blocks.
    expected_revision: i64,
    /// Concise audit summary of the mutation batch.
    summary: String,
    /// Ordered declarative patch operations.
    ops: Vec<PatchOp>,
    /// Optional bibliography source map committed atomically with the operations.
    sources: Option<SourceInputMap>,
    /// Runtime-managed streamed preview IDs superseded by this commit; normally omit.
    preview_ids: Vec<String>,
) -> Result<contracts::ApplyOperationsReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let request_context = in_process_request_context(context)?;
    runtime
        .report
        .exec_apply_operations(
            request_context,
            contracts::ApplyOperationsParams {
                doc_id,
                expected_revision,
                summary,
                ops,
                sources: sources.map(|sources| sources.0),
                preview_ids,
            },
        )
        .await
        .map_err(mcp_error_to_tool_error)
}

/// Restore a report to an earlier revision by committing a new forward revision.
/// This whole-document operation also undoes concurrent edits after the target. Prefer surgical
/// `apply_operations` for localized fixes; use rollback only for an explicit or broad restoration.
#[rig_tool(name = "rollback_revision")]
pub(crate) async fn rollback_revision_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Target report `doc_id`.
    doc_id: String,
    /// Historical revision to restore, below the current revision.
    target_revision: i64,
) -> Result<contracts::RollbackReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let request_context = in_process_request_context(context)?;
    runtime
        .report
        .exec_rollback_revision(
            request_context,
            contracts::RollbackParams {
                doc_id,
                target_revision,
            },
        )
        .await
        .map_err(mcp_error_to_tool_error)
}

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

/// Register or update bibliography sources with URL and normalized-title deduplication.
/// Register only sources actually retrieved and read. `excerpt` is optional and reader-facing:
/// include a short verbatim passage from one natural-language document only when directly useful;
/// omit it for structured records and never substitute query results, multi-document compilations,
/// analysis, rankings, internal IDs or editorial notes. Use `note` for concise scope and caliber.
/// Inspect `canonical_id_map` and cite mapped IDs. Chat claims use `<citation id="SOURCE_ID"/>`.
#[rig_tool(name = "upsert_sources", required(sources))]
pub(crate) async fn upsert_sources_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Report ID; omit to use the session's currently bound report.
    doc_id: String,
    /// Non-empty list of real sources. Include a short excerpt only when useful to a human reader.
    sources: Vec<contracts::SourceInput>,
) -> Result<Value, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    if let Some(doc_id) = resolve_existing_session_doc_id(context, &runtime, &doc_id).await? {
        let saved_sources = sources.clone();
        let request_context = in_process_request_context(context)?;
        let receipt = runtime
            .report
            .exec_upsert_sources(
                request_context,
                contracts::UpsertSourcesParams { doc_id, sources },
            )
            .await
            .map_err(mcp_error_to_tool_error)?;
        let mut value = serde_json::to_value(receipt).map_err(tool_other)?;
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "sources".to_string(),
                serde_json::to_value(saved_sources).unwrap_or(Value::Null),
            );
        }
        return Ok(value);
    }
    let upserted_ids: Vec<String> = sources
        .iter()
        .filter_map(|entry| entry.id.as_ref().map(|id| id.as_str().to_string()))
        .collect();
    Ok(serde_json::json!({
        "mode": "chat_session",
        "doc_id": Value::Null,
        "upserted_ids": upserted_ids,
        "total_sources": sources.len(),
        "sources": sources,
    }))
}

/// List registered bibliography sources and optional reader-focused evidence excerpts.
/// Reuse canonical IDs before authoring citations. Check a present excerpt for brevity, fidelity
/// and relevance; otherwise return to the original document or structured source data. Missing
/// excerpts are valid. Unknown IDs fail with the valid registered IDs.
#[rig_tool(name = "list_sources", required())]
pub(crate) async fn list_sources_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Report ID; omit to use the session's currently bound report.
    doc_id: String,
    /// Optional exact source IDs to fetch; omit to list all registered sources.
    source_ids: Option<Vec<String>>,
) -> Result<Value, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    if let Some(doc_id) = resolve_existing_session_doc_id(context, &runtime, &doc_id).await? {
        let receipt = runtime
            .report
            .list_sources(contracts::ListSourcesParams { doc_id, source_ids })
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

/// Partially update report cover configuration.
/// Omitted fields preserve stored values; an empty `subtitle`, `date` or `custom_html` clears it.
#[rig_tool(name = "set_cover")]
pub(crate) async fn set_cover_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Target report `doc_id`.
    doc_id: String,
    /// Partial cover fields to update.
    cover_patch: contracts::CoverPatch,
) -> Result<contracts::SetCoverReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let request_context = in_process_request_context(context)?;
    runtime
        .report
        .exec_set_cover(
            request_context,
            contracts::SetCoverParams {
                doc_id,
                cover_patch,
            },
        )
        .await
        .map_err(mcp_error_to_tool_error)
}

/// Configure the automatic table of contents and its page breaks.
/// The TOC derives from headings up to `max_depth`; never author a manual TOC block.
#[allow(
    clippy::too_many_arguments,
    reason = "Keep the established flat tool arguments rather than introducing a nested request object."
)]
#[rig_tool(name = "configure_toc")]
pub(crate) async fn configure_toc_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Target report `doc_id`.
    doc_id: String,
    /// Enable or disable the automatic TOC.
    enabled: bool,
    /// Optional TOC title; omit to preserve the current title.
    title: Option<String>,
    /// Maximum included heading level (`1..=3`).
    max_depth: Option<u8>,
    /// Optional TOC visual variant.
    variant: Option<String>,
    /// Whether to force a page break before the TOC.
    page_break_before: Option<bool>,
    /// Whether to force a page break after the TOC.
    page_break_after: Option<bool>,
) -> Result<contracts::ConfigureTocReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let request_context = in_process_request_context(context)?;
    runtime
        .report
        .exec_configure_toc(
            request_context,
            contracts::ConfigureTocParams {
                doc_id,
                enabled,
                title,
                max_depth,
                variant,
                page_break_before,
                page_break_after,
            },
        )
        .await
        .map_err(mcp_error_to_tool_error)
}

/// Configure the automatic bibliography section.
/// The receipt includes registered but uncited `orphan_sources` for cleanup before finalizing.
#[rig_tool(name = "configure_references")]
pub(crate) async fn configure_references_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Target report `doc_id`.
    doc_id: String,
    /// Enable or disable the bibliography.
    enabled: bool,
    /// Optional bibliography heading.
    title: Option<String>,
    /// Optional citation formatting style.
    style: Option<quire_protocol::CitationStyle>,
    /// Whether to start the bibliography on a new page.
    page_break_before: Option<bool>,
) -> Result<contracts::ConfigureReferencesReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let request_context = in_process_request_context(context)?;
    runtime
        .report
        .exec_configure_references(
            request_context,
            contracts::ConfigureReferencesParams {
                doc_id,
                enabled,
                title,
                style,
                page_break_before,
            },
        )
        .await
        .map_err(mcp_error_to_tool_error)
}

/// Configure A4 running headers, footers, first-page visibility and heading page breaks.
/// Omitted fields preserve current settings. Slots support `{page}`, `{total}`, `{title}`,
/// `{subtitle}`, `{date}`, `{kicker}` and `{authors}`.
#[allow(
    clippy::too_many_arguments,
    reason = "Keep the established flat tool arguments rather than introducing a nested request object."
)]
#[rig_tool(name = "configure_page_chrome")]
pub(crate) async fn configure_page_chrome_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Target report `doc_id`.
    doc_id: String,
    /// Optional running header slots; omit to preserve the current header.
    header: Option<quire_protocol::PageChromeSlots>,
    /// Optional running footer slots; omit to preserve the current footer.
    footer: Option<quire_protocol::PageChromeSlots>,
    /// Whether to hide the running header on the first page.
    hide_header_on_first_page: Option<bool>,
    /// Whether to hide the running footer on the first page.
    hide_footer_on_first_page: Option<bool>,
    /// Automatic page-break policy before headings.
    heading_page_break: Option<quire_protocol::HeadingPageBreak>,
    /// Optional expected revision for an optimistic concurrency check.
    base_revision: Option<u64>,
) -> Result<contracts::ConfigurePageChromeReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let request_context = in_process_request_context(context)?;
    runtime
        .report
        .exec_configure_page_chrome(
            request_context,
            contracts::ConfigurePageChromeParams {
                doc_id,
                header,
                footer,
                hide_header_on_first_page,
                hide_footer_on_first_page,
                heading_page_break,
                base_revision,
            },
        )
        .await
        .map_err(mcp_error_to_tool_error)
}

/// Update the report theme preset, `--rpt-*` tokens or scoped `.report-root` custom CSS.
/// Read the CSS protocol guide first. External imports, remote URLs and unscoped global selectors
/// are rejected; use `preview_report` to verify contrast and A4 layout.
#[rig_tool(name = "update_style_and_css", required(doc_id))]
pub(crate) async fn update_style_and_css_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Target report `doc_id`.
    doc_id: String,
    /// Optional theme preset; omit to preserve the current preset.
    preset: Option<String>,
    /// CSS custom-property overrides merged into the active theme.
    tokens_patch: BTreeMap<String, String>,
    /// Optional custom CSS scoped to `.report-root`.
    custom_css: Option<String>,
    /// Whether to append or replace custom CSS; defaults to append.
    css_mode: contracts::CssMode,
) -> Result<contracts::UpdateStyleReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let request_context = in_process_request_context(context)?;
    runtime
        .report
        .exec_update_style_and_css(
            request_context,
            contracts::UpdateStyleParams {
                doc_id,
                preset,
                tokens_patch,
                custom_css,
                css_mode,
            },
        )
        .await
        .map_err(mcp_error_to_tool_error)
}

/// Run structural, citation, chart, table and prose checks across the report.
/// Call during verification after edits. Resolve every error before completion, review warnings and
/// suppressed blocks, and suppress only deliberately inapplicable suppressible rules.
#[rig_tool(name = "lint_document", required(doc_id))]
pub(crate) async fn lint_document_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Target report `doc_id`.
    doc_id: String,
    /// Optional rule codes, names or category prefixes to select; omit for the full suite.
    select: Vec<String>,
    /// Optional suppressible rule codes, names or prefixes to hide during this check.
    ignore: Vec<String>,
) -> Result<contracts::LintReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    runtime
        .report
        .lint_document(contracts::LintParams {
            doc_id,
            select,
            ignore,
        })
        .await
        .map_err(mcp_error_to_tool_error)
}

/// List side-channel review threads anchored to report blocks.
/// By default returns open and addressed threads and hides archived threads, including anchor
/// blocks, text quotes and detached-anchor state.
#[rig_tool(name = "list_comments")]
pub(crate) async fn list_comments_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Target report `doc_id`.
    doc_id: String,
    /// Optional status filter; archived threads are returned only when explicitly requested.
    status: Option<quire_protocol::CommentStatus>,
    /// Optional block ID filter.
    block_id: Option<String>,
) -> Result<contracts::ListCommentsReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    runtime
        .report
        .list_comments(contracts::ListCommentsParams {
            doc_id,
            status,
            block_id,
        })
        .await
        .map_err(mcp_error_to_tool_error)
}

/// Create a review comment thread anchored to one or more blocks without changing the revision.
/// Use for reviewer/QA findings; an authoring agent asked to fix content should edit through
/// `apply_operations` instead.
#[allow(
    clippy::too_many_arguments,
    reason = "The review selector fields are independent public tool arguments and must stay flat."
)]
#[rig_tool(name = "add_comment", required(doc_id, block_id, body))]
pub(crate) async fn add_comment_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Target report `doc_id`.
    doc_id: String,
    /// Existing block where the review anchor starts.
    block_id: String,
    /// Optional ending block for a cross-block selection.
    end_block_id: Option<String>,
    /// Optional ordered list of all selected blocks.
    anchor_blocks: Vec<String>,
    /// Actionable review finding and expected fix.
    body: String,
    /// Optional exact text-quote selector in the starting block.
    quote: Option<quire_protocol::TextQuoteSelector>,
    /// Optional exact text-quote selector in the ending block.
    end_quote: Option<quire_protocol::TextQuoteSelector>,
    /// Optional Markdown/text snapshot of the anchored content.
    anchor_snapshot: Option<String>,
) -> Result<contracts::AddCommentReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let request_context = in_process_request_context(context)?;
    runtime
        .report
        .exec_add_comment(
            request_context,
            contracts::AddCommentParams {
                doc_id,
                block_id,
                end_block_id,
                anchor_blocks,
                body,
                quote,
                end_quote,
                anchor_snapshot,
            },
        )
        .await
        .map_err(mcp_error_to_tool_error)
}

/// Reply to a review thread and optionally mark it addressed after fixing the underlying blocks.
/// Pass the mutation revision as `addressed_rev`. Agents cannot archive threads; archiving is
/// reserved for human reviewers.
#[rig_tool(name = "reply_comment", required(doc_id, thread_id, body))]
pub(crate) async fn reply_comment_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Target report `doc_id`.
    doc_id: String,
    /// Thread ID from `list_comments` or a comment reference.
    thread_id: String,
    /// Concise explanation of the fix or answer.
    body: String,
    /// Set true after committing the fix to transition open to addressed.
    mark_addressed: bool,
    /// Revision containing the fix; defaults to the current document revision.
    addressed_rev: Option<i64>,
) -> Result<contracts::ReplyCommentReceipt, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let request_context = in_process_request_context(context)?;
    runtime
        .report
        .exec_reply_comment(
            request_context,
            contracts::ReplyCommentParams {
                doc_id,
                thread_id,
                body,
                mark_addressed,
                addressed_rev,
            },
        )
        .await
        .map_err(mcp_error_to_tool_error)
}
