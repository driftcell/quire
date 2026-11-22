//! MCP tool input schemas and compact receipts.
//!
//! Every tool has one `*Params` input struct and one `*Receipt` output struct. `schemars`
//! turns the `///` comments below into JSON Schema descriptions, so keep them short and
//! actionable for agents that discover tools dynamically.

use std::collections::BTreeMap;

use indexmap::IndexMap;
use quire_protocol::{
    Block, CitationStyle, CommentStatus, CommentThread, CoverConfig, Diagnostic, HeadingPageBreak,
    IdAssignment, LintSummary, PageChromeConfig, PageChromeSlots, PatchOp, ReferencesConfig,
    SourceEntry, SourceId, TextQuoteSelector, ThemeSummary, TocConfig,
};
use quire_store::ReportSummary;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::guide::GuideTopic;

/// Input for `get_protocol_guide`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuideParams {
    /// Topic to disclose: quickstart, tables_and_charts, citations_and_css or parallel_workflow.
    pub topic: GuideTopic,
}

/// Output for `get_protocol_guide`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GuideReceipt {
    /// Disclosed topic.
    pub topic: GuideTopic,
    /// Markdown guide text with examples and common mistakes.
    pub guide: String,
    /// Tools relevant to this workflow stage.
    pub recommended_tools: Vec<String>,
    /// Current INIT -> SCHEMA -> AUTHOR -> VERIFY state machine.
    pub workflow: crate::guide::GuideWorkflow,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// Input for `create_report`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateReportParams {
    /// Human-readable report title, for example "Q3 Revenue Review".
    pub title: String,
    /// Optional frontend theme preset, for example "editorial" or "technical".
    #[serde(default)]
    pub theme_preset: Option<String>,
    /// Render a cover page. Default true.
    #[serde(default)]
    pub enable_cover: Option<bool>,
    /// Enable the auto-generated TOC. Default true.
    #[serde(default)]
    pub enable_toc: Option<bool>,
    /// Optional initial blocks (full Block AST). IDs must be unique semantic strings.
    #[serde(default)]
    pub initial_blocks: Option<Vec<Block>>,
    /// Stable de-duplication key for retries.
    #[serde(default)]
    pub idempotency_key: Option<String>,
    /// Optional caller-supplied doc_id; otherwise generated from the title.
    #[serde(default)]
    pub doc_id: Option<String>,
}

/// Output for `create_report`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CreateReportReceipt {
    pub doc_id: String,
    /// New revision, always 1 for a fresh report.
    pub revision: i64,
    pub title: String,
    /// Session bound from the `X-Session-Id` HTTP header.
    pub session_id: Option<String>,
    /// True when `idempotency_key` matched an earlier create call.
    pub idempotent_replay: bool,
    /// Compact outline of the created document.
    pub digest: DocumentDigest,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// Input for `list_reports`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListReportsParams {
    /// Case-insensitive doc_id/title substring.
    #[serde(default)]
    pub query: Option<String>,
    /// Page size, 1..=100, default 20.
    #[serde(default)]
    pub limit: Option<i64>,
}

/// Output for `list_reports`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListReportsReceipt {
    pub reports: Vec<ReportSummaryView>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// One report row inside `ListReportsReceipt`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReportSummaryView {
    pub doc_id: String,
    pub title: String,
    pub session_id: Option<String>,
    pub current_rev: i64,
    pub block_count: i64,
    /// RFC 3339 timestamp of the latest revision.
    pub updated_at: String,
}

impl From<ReportSummary> for ReportSummaryView {
    fn from(summary: ReportSummary) -> Self {
        Self {
            doc_id: summary.doc_id.to_string(),
            title: summary.title,
            session_id: summary
                .session_id
                .map(|session| session.as_str().to_string()),
            current_rev: summary.current_rev.get(),
            block_count: summary.block_count,
            updated_at: summary.updated_at.to_rfc3339(),
        }
    }
}

/// Input for `get_document_digest`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DigestParams {
    pub doc_id: String,
    /// Optional case-insensitive keyword filter over id/type/text.
    #[serde(default)]
    pub query: Option<String>,
}

/// Output for `get_document_digest`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct DigestReceipt {
    pub digest: DocumentDigest,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

fn is_zero_usize(value: &usize) -> bool {
    *value == 0
}

/// Low-token summary of one block.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BlockDigest {
    pub id: String,
    #[serde(rename = "type")]
    pub block_type: String,
    /// Heading level, when the block is a heading.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    /// First 30 characters of visible text (omitted for empty blocks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    /// Visible character count (omitted for headings and empty blocks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub char_count: Option<usize>,
    /// Source IDs referenced by this block (omitted when empty).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_ids: Vec<String>,
}

/// Low-token outline of a report: revision, theme, TOC and block summaries.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DocumentDigest {
    pub title: String,
    pub current_rev: i64,
    pub theme: ThemeSummary,
    pub toc: TocConfig,
    pub block_count: usize,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub source_count: usize,
    /// Number of paragraph blocks, excluding headings, quotations and visuals.
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub paragraph_count: usize,
    /// Visible prose characters across paragraph blocks, for requirement checks rather than quotas.
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub prose_char_count: usize,
    /// Combined Table, ECharts and CustomHtml block count.
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub visual_block_count: usize,
    /// Unique registered or dangling source IDs used by inline and block citations.
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub cited_source_count: usize,
    pub blocks: Vec<BlockDigest>,
}

/// Input for `read_as_markdown`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadMarkdownParams {
    /// Existing report ID.
    pub doc_id: String,
    /// Project one matching heading section.
    #[serde(default)]
    pub section_heading: Option<String>,
    /// Project specific block IDs, in document order. Example: `["ch1_summary"]`.
    #[serde(default)]
    pub block_ids: Option<Vec<String>>,
    /// Expand ECharts series into Markdown data tables. Default true.
    #[serde(default)]
    pub include_chart_data: Option<bool>,
}

/// Output for `read_as_markdown`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReadMarkdownReceipt {
    /// `doc:<id>` source identifier.
    pub source: String,
    pub doc_id: String,
    pub revision: i64,
    pub markdown_chars: usize,
    /// Markdown projection with `<!-- block:id | type:... -->` anchors.
    pub markdown: String,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// Input for `read_blocks`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadBlocksParams {
    pub doc_id: String,
    /// Block IDs to fetch, for example `["financials","kpi-grid"]`.
    pub block_ids: Vec<String>,
}

/// Output for `read_blocks`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReadBlocksReceipt {
    pub doc_id: String,
    pub revision: i64,
    /// Full canonical protocol JSON for the requested blocks.
    pub blocks: Vec<Block>,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// Input for `apply_operations`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyOperationsParams {
    pub doc_id: String,
    /// Revision you read. Smart CAS rebases disjoint edits; overlapping edits return a conflict.
    pub expected_revision: i64,
    /// Short intent summary, for example "write chapter 2 revenue section".
    pub summary: String,
    /// Declarative patch operations. Example: `[{"type":"append","blocks":[...]}]`.
    pub ops: Vec<PatchOp>,
    /// Optional sources upserted atomically in the same transaction as the blocks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<BTreeMap<SourceId, SourceInput>>")]
    pub sources: Option<IndexMap<SourceId, SourceInput>>,
    /// Ephemeral preview IDs superseded by this commit. The emitted revision carries
    /// `cleared_preview_ids` so the frontend replaces streamed placeholders with no flicker.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preview_ids: Vec<String>,
}

/// Output for `apply_operations`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ApplyOperationsReceipt {
    pub new_revision: i64,
    /// Revision supplied as `expected_revision`.
    pub base_revision: i64,
    /// True when Smart CAS committed the batch on top of newer revisions.
    pub rebased: bool,
    /// Number of committed revisions skipped by a successful rebase.
    pub rebased_over: usize,
    /// True when the batch committed; inspect `blocking_errors` before continuing.
    pub committed: bool,
    /// Block IDs auto-assigned by the patch engine.
    pub assigned_ids: Vec<IdAssignment>,
    /// Non-blocking warnings/info diagnostics scoped to blocks touched by this batch.
    pub inline_warnings: Vec<quire_protocol::Diagnostic>,
    /// Lint errors scoped to blocks touched by this batch. A non-empty list means the batch
    /// committed but the document is not clean yet.
    pub blocking_errors: Vec<quire_protocol::Diagnostic>,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// Input for `rollback_revision`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RollbackParams {
    pub doc_id: String,
    /// Target revision in 1..current_revision-1.
    pub target_revision: i64,
}

/// Output for `rollback_revision`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RollbackReceipt {
    pub doc_id: String,
    /// New monotonic revision created by the rollback.
    pub new_revision: i64,
    /// Revision state that was restored.
    pub rolled_back_to: i64,
    /// Number of inverse-op revisions replayed.
    pub replayed_revisions: usize,
    /// Compact human-readable outcome; inspect content with digest/read_blocks if needed.
    pub summary: String,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// Source payload accepted by `upsert_sources` and inline source maps.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SourceInput {
    /// Stable short source ID, e.g. "sec10". Required by `upsert_sources`; optional when the
    /// source is supplied as a map value (`sources: {"sec10": {...}}`).
    #[serde(default, alias = "source_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<SourceId>,
    #[serde(flatten)]
    pub entry: SourceEntry,
}

/// Input for `upsert_sources`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpsertSourcesParams {
    /// Report that owns the sources.
    pub doc_id: String,
    /// Sources to register/update. Duplicate URLs or normalized titles map to one canonical ID.
    pub sources: Vec<SourceInput>,
}

/// Output for `upsert_sources`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UpsertSourcesReceipt {
    pub doc_id: String,
    pub revision: i64,
    /// False when every source already existed and no revision was created.
    pub changed: bool,
    /// Requested source ID -> canonical source ID.
    pub canonical_id_map: BTreeMap<String, String>,
    pub source_count: usize,
    pub inline_warnings: Vec<quire_protocol::Diagnostic>,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// Input for `set_cover`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetCoverParams {
    pub doc_id: String,
    /// Partial cover fields. Omitted fields are preserved.
    pub cover_patch: CoverPatch,
}

/// Partial cover update; omitted fields are preserved.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoverPatch {
    /// Enable or disable the cover.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// classic | modern | minimal | split | custom
    #[serde(default)]
    pub variant: Option<String>,
    /// Cover title. Must be non-empty when supplied.
    #[serde(default)]
    pub title: Option<String>,
    /// Subtitle; empty string clears it.
    #[serde(default)]
    pub subtitle: Option<String>,
    /// Author line, for example `["Ada Lovelace"]`.
    #[serde(default)]
    pub authors: Option<Vec<String>>,
    /// Publication date; empty string clears it.
    #[serde(default)]
    pub date: Option<String>,
    /// Key/value metadata, for example `{"classification":"internal"}`.
    #[serde(default)]
    pub meta: Option<BTreeMap<String, String>>,
    /// Raw HTML override for the cover body; empty string clears it.
    #[serde(default)]
    pub custom_html: Option<String>,
}

/// Input for `list_sources`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListSourcesParams {
    pub doc_id: String,
    /// Optional exact source IDs. Unknown IDs return an error instead of being silently omitted.
    #[serde(default)]
    pub source_ids: Option<Vec<String>>,
}

/// Output for `list_sources`, including evidence excerpts needed for claim-level review.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListSourcesReceipt {
    pub doc_id: String,
    pub revision: i64,
    #[schemars(with = "BTreeMap<SourceId, SourceEntry>")]
    pub sources: IndexMap<SourceId, SourceEntry>,
    pub count: usize,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// Output for `set_cover`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SetCoverReceipt {
    pub doc_id: String,
    pub revision: i64,
    pub cover: CoverConfig,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// Input for `configure_toc`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigureTocParams {
    pub doc_id: String,
    /// Enable or disable the generated TOC.
    pub enabled: bool,
    /// TOC heading, for example `"Contents"`.
    #[serde(default)]
    pub title: Option<String>,
    /// Heading depth to include, 1..=3.
    #[serde(default)]
    pub max_depth: Option<u8>,
    /// standard | numbered | dotted
    #[serde(default)]
    pub variant: Option<String>,
    /// Start the TOC on a new page after the cover.
    #[serde(default)]
    pub page_break_before: Option<bool>,
    /// Start body content on a new page after the TOC.
    #[serde(default)]
    pub page_break_after: Option<bool>,
}

/// Output for `configure_toc`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ConfigureTocReceipt {
    pub doc_id: String,
    pub revision: i64,
    pub toc: TocConfig,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// Input for `configure_references`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigureReferencesParams {
    pub doc_id: String,
    /// Enable or disable the bibliography section.
    pub enabled: bool,
    /// Bibliography heading, for example `"References"`.
    #[serde(default)]
    pub title: Option<String>,
    /// numeric | author_year
    #[serde(default)]
    pub style: Option<CitationStyle>,
    /// Start the bibliography on a new page.
    #[serde(default)]
    pub page_break_before: Option<bool>,
}

/// Output for `configure_references`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ConfigureReferencesReceipt {
    pub doc_id: String,
    pub revision: i64,
    pub references: ReferencesConfig,
    /// Registered source IDs that are not cited by any block.
    pub orphan_sources: Vec<String>,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// Input for `configure_page_chrome`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigurePageChromeParams {
    pub doc_id: String,
    /// Header slots. Omit to keep the current header.
    #[serde(default)]
    pub header: Option<PageChromeSlots>,
    /// Footer slots. Omit to keep the current footer.
    #[serde(default)]
    pub footer: Option<PageChromeSlots>,
    /// Hide the header on the first page. Omit to keep the current setting.
    #[serde(default)]
    pub hide_header_on_first_page: Option<bool>,
    /// Hide the footer on the first page. Omit to keep the current setting.
    #[serde(default)]
    pub hide_footer_on_first_page: Option<bool>,
    /// Automatic page break before headings: `none`, `h1` or `h1_and_h2`.
    /// Omit to keep the current setting.
    #[serde(default)]
    pub heading_page_break: Option<HeadingPageBreak>,
    /// Optional expected revision for optimistic concurrency.
    #[serde(default)]
    pub base_revision: Option<u64>,
}

/// Output for `configure_page_chrome`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ConfigurePageChromeReceipt {
    pub doc_id: String,
    pub revision: i64,
    pub page_chrome: PageChromeConfig,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// How `update_style_and_css` merges custom CSS.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CssMode {
    /// Append to the existing custom CSS block.
    Append,
    /// Replace the existing custom CSS block.
    Replace,
}

/// Input for `update_style_and_css`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateStyleParams {
    pub doc_id: String,
    /// Theme preset, for example `editorial`, `technical` or `default`.
    #[serde(default)]
    pub preset: Option<String>,
    /// Merge into existing CSS custom properties. Example: `{"--rpt-primary":"#0b3d91"}`.
    #[serde(default)]
    pub tokens_patch: BTreeMap<String, String>,
    /// Arbitrary CSS scoped to `.report-root` by the frontend. Validated before saving.
    #[serde(default)]
    pub custom_css: Option<String>,
    /// append (default) or replace.
    #[serde(default = "default_css_mode")]
    pub css_mode: CssMode,
}

fn default_css_mode() -> CssMode {
    CssMode::Append
}

/// Output for `update_style_and_css`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UpdateStyleReceipt {
    pub doc_id: String,
    pub revision: i64,
    pub theme: ThemeSummary,
    pub token_count: usize,
    pub has_custom_css: bool,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// Input for `lint_document`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LintParams {
    pub doc_id: String,
    /// Only report rules matching these codes, names or category prefixes (e.g. `QR2`).
    #[serde(default)]
    pub select: Vec<String>,
    /// Hide suppressible rules matching these codes, names or category prefixes.
    /// Core contract and safety rules remain visible.
    #[serde(default)]
    pub ignore: Vec<String>,
}

/// Output for `lint_document`. Clean documents return only `{"ok":true,"rev":N}`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LintReceipt {
    pub ok: bool,
    pub rev: i64,
    /// Error, warning and info counts (omitted when clean).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<LintSummary>,
    /// Diagnostics (omitted when clean).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(
        rename = "_next_hint",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub _next_hint: Option<String>,
}

/// Input for `list_comments`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListCommentsParams {
    pub doc_id: String,
    /// Optional lifecycle filter. Archived threads are omitted when this is not supplied.
    #[serde(default)]
    pub status: Option<CommentStatus>,
    /// Optional block-ID soft-anchor filter.
    #[serde(default)]
    pub block_id: Option<String>,
}

/// Output for `list_comments`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListCommentsReceipt {
    pub doc_id: String,
    pub comments: Vec<CommentThread>,
    pub count: usize,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// Input for `add_comment`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddCommentParams {
    pub doc_id: String,
    /// Existing block the review thread is anchored to.
    pub block_id: String,
    /// Ending block for a cross-block selection.
    #[serde(default)]
    pub end_block_id: Option<String>,
    /// Every selected block in document order.
    #[serde(default)]
    pub anchor_blocks: Vec<String>,
    /// Review comment body.
    pub body: String,
    /// Optional start-block text-selection quote for highlighting.
    #[serde(default)]
    pub quote: Option<TextQuoteSelector>,
    /// Optional end-block text-selection quote for cross-block highlighting.
    #[serde(default)]
    pub end_quote: Option<TextQuoteSelector>,
    /// Optional frontend-authored snapshot containing media placeholders.
    #[serde(default)]
    pub anchor_snapshot: Option<String>,
}

/// Output for `add_comment`. Thread fields are flattened for compact receipts.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AddCommentReceipt {
    #[serde(flatten)]
    pub thread: CommentThread,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// Input for `reply_comment`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplyCommentParams {
    pub doc_id: String,
    pub thread_id: String,
    /// Reply body.
    pub body: String,
    /// Move an open thread to addressed after the writer fixes the issue.
    #[serde(default)]
    pub mark_addressed: bool,
    /// Revision containing the fix. Defaults to the current document revision.
    #[serde(default)]
    pub addressed_rev: Option<i64>,
}

/// Output for `reply_comment`. Thread fields are flattened for compact receipts.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReplyCommentReceipt {
    #[serde(flatten)]
    pub thread: CommentThread,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_params_reject_unknown_fields_with_field_name() {
        let error = serde_json::from_value::<ListReportsParams>(json!({
            "query": "atlas",
            "limitt": 5
        }))
        .expect_err("unknown field must be rejected");
        assert!(error.to_string().contains("limitt"), "{error}");

        let error = serde_json::from_value::<CreateReportParams>(json!({
            "title": "Synthetic",
            "enable_cvoer": true
        }))
        .expect_err("misspelled field must be rejected");
        assert!(error.to_string().contains("enable_cvoer"), "{error}");
    }

    #[test]
    fn nested_cover_patch_rejects_unknown_fields() {
        let error = serde_json::from_value::<SetCoverParams>(json!({
            "doc_id": "synthetic",
            "cover_patch": {"enabeld": true}
        }))
        .expect_err("unknown nested field must be rejected");
        assert!(error.to_string().contains("enabeld"), "{error}");
    }

    #[test]
    fn valid_params_still_deserialize() {
        let params: ApplyOperationsParams = serde_json::from_value(json!({
            "doc_id": "synthetic",
            "expected_revision": 7,
            "summary": "write section",
            "ops": [{"type": "append", "blocks": []}]
        }))
        .expect("valid params");
        assert_eq!(params.expected_revision, 7);

        let reply: ReplyCommentParams = serde_json::from_value(json!({
            "doc_id": "synthetic",
            "thread_id": "th_1",
            "body": "Fixed",
            "mark_addressed": true,
            "addressed_rev": 4
        }))
        .expect("valid comment reply");
        assert!(reply.mark_addressed);
        assert_eq!(reply.addressed_rev, Some(4));
    }

    #[test]
    fn configure_toc_accepts_page_break_options() {
        let params: ConfigureTocParams = serde_json::from_value(json!({
            "doc_id": "synthetic",
            "enabled": true,
            "page_break_before": true,
            "page_break_after": false
        }))
        .expect("valid TOC page-break params");
        assert_eq!(params.page_break_before, Some(true));
        assert_eq!(params.page_break_after, Some(false));
    }

    #[test]
    fn agent_comment_tools_reject_archive_status() {
        let error = serde_json::from_value::<ReplyCommentParams>(json!({
            "doc_id": "synthetic",
            "thread_id": "th_1",
            "body": "Archived",
            "status": "archived"
        }))
        .expect_err("agents cannot set archived through reply_comment");
        assert!(error.to_string().contains("status"), "{error}");
    }
}
