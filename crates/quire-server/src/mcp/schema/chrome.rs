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
