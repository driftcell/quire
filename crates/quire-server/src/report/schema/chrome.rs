use std::collections::BTreeMap;

use quire_protocol::{
    CitationStyle, HeadingPageBreak, PageChromeConfig, PageChromeSlots, ReferencesConfig,
    ThemeSummary, TocConfig,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetCoverParams {
    /// Target report `doc_id`.
    pub doc_id: String,
    /// Partial cover fields to update; omitted fields retain their current stored values.
    pub cover_patch: CoverPatch,
}

/// Partial cover update; omitted fields are preserved.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoverPatch {
    /// Enable or disable rendering the cover page.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// Cover layout variant: `"classic"`, `"modern"`, `"minimal"`, `"split"`, or `"custom"`.
    #[serde(default)]
    pub variant: Option<String>,
    /// Primary report title on the cover (must be non-empty when supplied).
    #[serde(default)]
    pub title: Option<String>,
    /// Optional subtitle; pass an empty string `""` to clear an existing subtitle.
    #[serde(default)]
    pub subtitle: Option<String>,
    /// Author or organization lines shown on the cover (e.g. `["战略研究组"]`); replaces the stored author list when supplied.
    #[serde(default)]
    pub authors: Option<Vec<String>>,
    /// Publication date string shown on the cover; pass `""` to clear.
    #[serde(default)]
    pub date: Option<String>,
    /// Key/value metadata rows rendered on the cover (e.g. `{"classification":"internal"}`); replaces stored metadata when supplied.
    #[serde(default)]
    pub meta: Option<BTreeMap<String, String>>,
    /// Raw HTML override for the cover body (used with `variant: "custom"`); pass `""` to clear.
    #[serde(default)]
    pub custom_html: Option<String>,
}

/// Input for `configure_toc`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigureTocParams {
    /// Target report `doc_id`.
    pub doc_id: String,
    /// Enable or disable the auto-generated Table of Contents.
    pub enabled: bool,
    /// Optional TOC section title (e.g. `"目录"` or `"Contents"`). Omit to keep the current title.
    #[serde(default)]
    pub title: Option<String>,
    /// Maximum heading level included in the TOC (`1..=3`). Omit to keep the current depth.
    #[serde(default)]
    pub max_depth: Option<u8>,
    /// TOC layout style: `"standard"`, `"numbered"`, or `"dotted"`.
    #[serde(default)]
    pub variant: Option<String>,
    /// Whether to force a page break before the TOC in paged/A4 mode.
    #[serde(default)]
    pub page_break_before: Option<bool>,
    /// Whether to force a page break after the TOC before body chapters begin.
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
    /// Target report `doc_id`.
    pub doc_id: String,
    /// Enable or disable rendering the bibliography section at the end of the report.
    pub enabled: bool,
    /// Optional bibliography heading (e.g. `"参考文献"` or `"References"`). Omit to keep the current heading.
    #[serde(default)]
    pub title: Option<String>,
    /// Citation formatting style: `"numeric"` (`[1]`) or `"author_year"`.
    #[serde(default)]
    pub style: Option<CitationStyle>,
    /// Whether to start the bibliography section on a new A4 page.
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
    /// Target report `doc_id`.
    pub doc_id: String,
    /// Running page header slots (`left`, `center`, `right`, supporting `{page}`, `{total}`, `{title}`, `{subtitle}`, `{date}`, `{kicker}`, and `{authors}`). Omit to keep the current header.
    #[serde(default)]
    pub header: Option<PageChromeSlots>,
    /// Running page footer slots (`left`, `center`, `right`, supporting `{page}`, `{total}`, `{title}`, `{subtitle}`, `{date}`, `{kicker}`, and `{authors}`). Omit to keep the current footer.
    #[serde(default)]
    pub footer: Option<PageChromeSlots>,
    /// Whether to hide the running header on the first page. Omit to keep the current setting.
    #[serde(default)]
    pub hide_header_on_first_page: Option<bool>,
    /// Whether to hide the running footer on the first page. Omit to keep the current setting.
    #[serde(default)]
    pub hide_footer_on_first_page: Option<bool>,
    /// Automatic page break policy before headings in paged/A4 mode: `"none"`, `"h1"`, or `"h1_and_h2"`. Omit to keep the current setting.
    #[serde(default)]
    pub heading_page_break: Option<HeadingPageBreak>,
    /// Optional expected document revision for optimistic concurrency check.
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
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CssMode {
    /// Append to the existing custom CSS block.
    #[default]
    Append,
    /// Replace the existing custom CSS block.
    Replace,
}

/// Input for `update_style_and_css`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateStyleParams {
    /// Target report `doc_id`.
    pub doc_id: String,
    /// Optional theme preset (`"default"`, `"editorial"`, `"technical"`, `"academic"`, `"executive"`, or `"minimal"`). Omit to keep the current preset.
    #[serde(default)]
    pub preset: Option<String>,
    /// CSS custom property overrides merged into the active theme tokens (keys must start with `--rpt-`, e.g. `{"--rpt-primary":"#0b3d91"}`).
    #[serde(default)]
    pub tokens_patch: BTreeMap<String, String>,
    /// Custom CSS rules scoped to `.report-root`. Validated against disallowed `@import`, external `url(...)`, and unsafe properties before saving.
    #[serde(default)]
    pub custom_css: Option<String>,
    /// How `custom_css` is applied: `"append"` (default) appends to existing custom CSS; `"replace"` overwrites it (pass `custom_css: ""` with `"replace"` to clear custom CSS).
    #[serde(default = "default_css_mode")]
    pub css_mode: CssMode,
}

fn default_css_mode() -> CssMode {
    CssMode::default()
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
