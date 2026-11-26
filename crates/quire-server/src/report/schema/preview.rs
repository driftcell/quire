use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PreviewViewMode {
    #[default]
    Paged,
    Continuous,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreviewReportInput {
    /// Target report `doc_id` to render in the headless preview route.
    pub doc_id: String,
    /// Layout mode: `"paged"` (default, A4 pagination with page count and page-break checks) or `"continuous"` (single-column scroll).
    #[serde(default)]
    pub mode: PreviewViewMode,
    /// Optional heading `block_id` from `get_document_digest` to preview only that section (automatically suppresses cover, TOC, and bibliography chrome).
    #[serde(default)]
    pub section_id: Option<String>,
    /// Optional list of specific `block_id`s to preview in isolation (automatically suppresses cover, TOC, and bibliography chrome).
    #[serde(default)]
    pub block_ids: Option<Vec<String>>,
    /// Optional 1-based A4 page number to focus and screenshot when `mode` is `"paged"`.
    #[serde(default)]
    pub page: Option<usize>,
    /// Optional exact committed document revision (`>= 1`) assertion; fails if the rendered document revision does not match.
    #[serde(default)]
    pub rev: Option<u64>,
    /// Whether to collect and return active `--rpt-*` theme tokens, `custom_css`, and computed styles (`true` by default).
    #[serde(default = "default_include_styles")]
    pub include_styles: bool,
    /// Whether to capture and return a PNG screenshot. Text-only clients should omit this.
    #[serde(default)]
    pub include_image: bool,
}

const fn default_include_styles() -> bool {
    true
}

/// Create or refine a transient ECharts preview without committing a report mutation.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreviewEchartsParams {
    /// Complete native ECharts 6.0 option JSON object on the first call. Omit when `based_on_preview_id` is set.
    pub option: Option<Value>,
    /// Minimal incremental JSON patch applied on top of `based_on_preview_id`.
    pub option_patch: Option<Value>,
    /// Previous `preview_id` from `preview_echarts` (cached for 30 minutes).
    pub based_on_preview_id: Option<String>,
    /// Chart canvas height in pixels (defaults to `360`).
    pub height: Option<u32>,
    /// Outer chart block title. Do not duplicate the main title inside `option.title`.
    pub title: Option<String>,
    /// Concise Chinese caption (`<= 120` chars) stating units and statistical scope.
    pub note: Option<String>,
    /// One-sentence Chinese quantitative takeaway stating subject, extremes, delta, or trend.
    pub fallback_summary: Option<String>,
    /// Canonical source IDs persisted with the preview and inherited by `commit_chart`.
    pub source_ids: Option<Vec<String>>,
    /// Optional source attribution note inherited by `commit_chart`.
    pub source_note: Option<String>,
    /// Whether to capture a PNG screenshot through the controlled Playwright preview path.
    #[serde(default)]
    pub include_image: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TargetScope {
    #[default]
    Document,
    Chat,
}

/// Commit a previously validated chart preview without re-sending its ECharts option.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CommitChartParams {
    /// Cached `preview_id` returned by `preview_echarts` (valid for 30 minutes).
    pub preview_id: String,
    /// Target report ID; omit to use the session's bound report or chat scope.
    pub doc_id: Option<String>,
    /// Short semantic block ID for the chart. Existing ECharts blocks are updated in place.
    pub block_id: String,
    /// Insert or move immediately after this existing block ID.
    pub after_block_id: Option<String>,
    /// Insert or move immediately before this existing block ID.
    pub before_block_id: Option<String>,
    /// Append to this heading section when no before/after anchor is set.
    pub section_id: Option<String>,
    /// Base revision for Smart CAS; omit to read the current revision first.
    pub expected_revision: Option<i64>,
    /// Short audit summary describing the chart insertion or update.
    pub summary: Option<String>,
    /// Optional canonical source IDs overriding those stored during preview.
    pub source_ids: Option<Vec<String>>,
    /// Optional source attribution note overriding the preview value.
    pub source_note: Option<String>,
    /// `"document"` (default) or `"chat"` for durable inline chat rendering.
    pub scope: Option<TargetScope>,
}
