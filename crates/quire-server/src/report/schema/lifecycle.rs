//! MCP tool input schemas and compact receipts.
//!
//! Every tool has one `*Params` input struct and one `*Receipt` output struct. `schemars`
//! turns the `///` comments below into JSON Schema descriptions, so keep them short and
//! actionable for agents that discover tools dynamically.

use std::{collections::BTreeMap, num::NonZeroU32};

use quire_protocol::{Block, BlockId, ThemeSummary, TocConfig};
use quire_store::ReportSummary;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::guide::GuideTopic;

/// Input for `get_protocol_guide`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuideParams {
    /// Protocol topic to disclose: `quickstart` (workflow & patch basics), `tables_and_charts` (`ColumnNode` tables & ECharts 6.0 options), `citations_and_css` (`SourceEntry` & `.report-root` CSS tokens), or `parallel_workflow` (subagent prefix isolation & Smart CAS rebase).
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
    /// Human-readable report title shown on the cover and document header (non-empty, e.g. `"2026 Q3 营收分析报告"`).
    pub title: String,
    /// Agreed total prose length in visible paragraph Unicode characters (excluding headings, tables, charts, and citation markers, e.g. `30000` for "约 3 万字"). Stored permanently on the report and tracked via `prose_char_delta` in `get_document_digest`.
    #[serde(default)]
    pub target_prose_chars: Option<NonZeroU32>,
    /// Optional visual theme preset (`"default"`, `"editorial"`, `"technical"`, `"academic"`, `"executive"`, or `"minimal"`).
    #[serde(default)]
    pub theme_preset: Option<String>,
    /// Whether to render the cover page (`true` by default; set `false` for brief memos).
    #[serde(default)]
    pub enable_cover: Option<bool>,
    /// Whether to render the auto-generated Table of Contents (`true` by default).
    #[serde(default)]
    pub enable_toc: Option<bool>,
    /// Optional initial Block AST entries (for example top-level chapter headings seeded before spawning subagents). Every block `id` must be a unique semantic slug (`[a-zA-Z0-9_-]{1,64}`), never a UUID.
    #[serde(default)]
    pub initial_blocks: Option<Vec<Block>>,
    /// Optional stable idempotency key (`1..=128` chars, automatically prefixed with the current `session_id`) so retries return the existing document instead of creating duplicates.
    #[serde(default)]
    pub idempotency_key: Option<String>,
    /// Optional explicit `doc_id` slug (`[a-zA-Z0-9_-]{1,64}`). Omit to auto-generate a slug from `title`.
    #[serde(default)]
    pub doc_id: Option<String>,
    /// Optional cover subtitle initialized at report creation.
    #[serde(default)]
    pub subtitle: Option<String>,
    /// Optional cover author or organization lines initialized at report creation (e.g. `["战略研究组"]`).
    #[serde(default)]
    pub authors: Option<Vec<String>>,
    /// Optional cover publication date string initialized at report creation (e.g. `"2026-11-23"`).
    #[serde(default)]
    pub date: Option<String>,
    /// Optional cover key/value metadata badges initialized at report creation (e.g. `{"密级":"内部公开","版本":"v1.0"}`).
    #[serde(default)]
    pub cover_meta: Option<BTreeMap<String, String>>,
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
    /// Optional case-insensitive substring filter matched against `doc_id` and report `title`. Omit to list recent reports.
    #[serde(default)]
    pub query: Option<String>,
    /// Maximum number of report summaries to return (`1..=100`, defaults to `20`).
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
    /// Target report `doc_id` (check `create_report` output or `list_reports`).
    pub doc_id: String,
    /// Optional case-insensitive keyword filter matched against block `id`, block `type`, or visible text. Omit to return the full block outline.
    #[serde(default)]
    pub query: Option<String>,
    /// Optional read-only comparison target in paragraph characters for older reports that have no stored `target_prose_chars`. Must be omitted when the report already stores an agreed target.
    #[serde(default)]
    pub target_prose_chars: Option<NonZeroU32>,
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
    /// Visible Unicode characters across paragraphs, excluding inline citations and Markdown syntax.
    pub prose_char_count: usize,
    /// User's agreed length, or an explicit comparison target for an older report.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_prose_chars: Option<NonZeroU32>,
    /// Actual minus target; negative means below target. This is not a per-chapter quota.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prose_char_delta: Option<i64>,
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
    /// Target report `doc_id`.
    pub doc_id: String,
    /// Exact heading `block_id` from `get_document_digest`. Projects that heading and all blocks beneath it up to the next heading of the same or higher level. Mutually exclusive with `block_ids`.
    #[serde(default)]
    pub section_id: Option<BlockId>,
    /// Legacy input for older callers; new agents use the exact `section_id`.
    #[serde(default)]
    #[schemars(skip)]
    pub section_heading: Option<String>,
    /// Exact list of `block_id`s to project in document order (e.g. `["ch1_summary", "ch1_table"]`). Mutually exclusive with `section_id`.
    #[serde(default)]
    pub block_ids: Option<Vec<String>>,
    /// Whether to expand ECharts series into Markdown data tables in the projection (`true` by default).
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
    /// Target report `doc_id`.
    pub doc_id: String,
    /// Non-empty list of exact `block_id`s from `get_document_digest` whose canonical Block AST JSON should be returned (e.g. `["ch2_rev_table", "ch2_kpi_grid"]`).
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
