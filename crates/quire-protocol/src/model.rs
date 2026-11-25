//! Document-level configuration and metadata.

use std::{collections::BTreeMap, num::NonZeroU32};

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    block::Block,
    ids::{DocId, Revision, SessionId, SourceId},
    lint::LintConfig,
};

/// Default value for fields that are enabled unless explicitly disabled.
const fn default_true() -> bool {
    true
}

/// Known cover variants. The field itself is a `String` so renderers can accept future variants.
pub const COVER_VARIANTS: &[&str] = &["classic", "modern", "minimal", "split", "custom"];

/// Known TOC variants. The field itself is a `String` for forward compatibility.
pub const TOC_VARIANTS: &[&str] = &["standard", "numbered", "dotted"];

/// Configuration for the generated cover page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CoverConfig {
    #[serde(default)]
    pub enabled: bool,
    /// One of `classic`, `modern`, `minimal`, `split`, `custom`; unknown values render as classic.
    #[serde(default = "default_cover_variant")]
    pub variant: String,
    #[serde(default)]
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    /// Human-readable publication date. Free-form on purpose (for example "Q3 2026").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// Optional key/value metadata rendered on the cover.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub meta: BTreeMap<String, String>,
    /// Full HTML override for the cover body. When present it replaces the generated cover body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_html: Option<String>,
}

fn default_cover_variant() -> String {
    "classic".to_string()
}

impl Default for CoverConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            variant: default_cover_variant(),
            title: String::new(),
            subtitle: None,
            authors: Vec::new(),
            date: None,
            meta: BTreeMap::new(),
            custom_html: None,
        }
    }
}

/// Table-of-contents configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TocConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_toc_title")]
    pub title: String,
    /// Maximum heading depth to include. Protocol range is 1..=3.
    #[serde(default = "default_toc_depth")]
    pub max_depth: u8,
    /// One of `standard`, `numbered`, `dotted`; unknown values render as standard.
    #[serde(default = "default_toc_variant")]
    pub variant: String,
    /// Start the TOC on a new page after the cover.
    #[serde(default)]
    pub page_break_before: bool,
    /// Start body content on a new page after the TOC.
    #[serde(default)]
    pub page_break_after: bool,
}

fn default_toc_variant() -> String {
    "dotted".to_string()
}

impl TocConfig {
    /// Normalize known variants; unknown values fall back to `standard`.
    pub fn variant_key(&self) -> &'static str {
        match self.variant.to_ascii_lowercase().as_str() {
            "numbered" => "numbered",
            "dotted" => "dotted",
            _ => "standard",
        }
    }
}

const fn default_toc_depth() -> u8 {
    3
}

fn default_toc_title() -> String {
    "Contents".to_string()
}

impl Default for TocConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            title: default_toc_title(),
            max_depth: default_toc_depth(),
            variant: default_toc_variant(),
            page_break_before: false,
            page_break_after: false,
        }
    }
}

/// A three-slot page header or footer row.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PageChromeSlots {
    #[serde(default)]
    pub left: String,
    #[serde(default)]
    pub center: String,
    #[serde(default)]
    pub right: String,
}

/// Automatic page-break policy for document headings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HeadingPageBreak {
    #[default]
    None,
    H1,
    H1AndH2,
}

/// A4 paged header/footer configuration stored under `theme.page_chrome`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PageChromeConfig {
    #[serde(default)]
    pub header: PageChromeSlots,
    #[serde(default)]
    pub footer: PageChromeSlots,
    #[serde(default = "default_true")]
    pub hide_header_on_first_page: bool,
    #[serde(default)]
    pub hide_footer_on_first_page: bool,
    #[serde(default)]
    pub heading_page_break: HeadingPageBreak,
}

/// Theme configuration. Tokens are CSS custom properties applied to `.report-root`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ThemeConfig {
    /// Built-in preset name. Unknown presets fall back to `default`.
    #[serde(default = "default_preset")]
    pub preset: String,
    /// Additional or overriding CSS custom properties, for example `--rpt-primary`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tokens: BTreeMap<String, String>,
    /// Arbitrary CSS. It is automatically scoped under `.report-root`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub custom_css: String,
    /// Optional A4 paged header/footer slots with template placeholders.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_chrome: Option<PageChromeConfig>,
}

fn default_preset() -> String {
    "default".to_string()
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            preset: default_preset(),
            tokens: BTreeMap::new(),
            custom_css: String::new(),
            page_chrome: None,
        }
    }
}

/// Return whether a CSS custom-property name is safe to serialize into a `<style>` block.
pub fn is_valid_css_token_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("--") else {
        return false;
    };
    !rest.is_empty()
        && rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// Return whether a CSS token value can be emitted without allowing declaration injection.
pub fn is_safe_css_token_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 2048
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, ';' | '{' | '}' | '<' | '>') || c == '\\')
        && !value.to_ascii_lowercase().contains("</style")
}

impl ThemeConfig {
    /// Tokens that can safely be emitted, sorted deterministically by property name.
    pub fn safe_tokens(&self) -> Vec<(&str, &str)> {
        let mut tokens: Vec<(&str, &str)> = self
            .tokens
            .iter()
            .filter(|(name, value)| is_valid_css_token_name(name) && is_safe_css_token_value(value))
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        tokens.sort_unstable_by(|left, right| left.0.cmp(right.0));
        tokens
    }

    /// A compact, deterministic summary for LLM digests and HTTP responses.
    pub fn summary(&self) -> ThemeSummary {
        ThemeSummary {
            preset: self.preset.clone(),
            token_count: self.tokens.len(),
            has_custom_css: !self.custom_css.is_empty(),
        }
    }
}

/// Low-token theme representation used by document digests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ThemeSummary {
    pub preset: String,
    pub token_count: usize,
    pub has_custom_css: bool,
}

/// Bibliographic label style.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CitationStyle {
    #[default]
    Numeric,
    AuthorYear,
}

/// Backwards-friendly alias used internally by citation/rendering code.
pub type ReferenceStyle = CitationStyle;

/// End-of-report bibliography configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReferencesConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_references_title")]
    pub title: String,
    #[serde(default)]
    pub style: CitationStyle,
    #[serde(default = "default_true")]
    pub page_break_before: bool,
}

fn default_references_title() -> String {
    "References".to_string()
}

impl Default for ReferencesConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            title: default_references_title(),
            style: ReferenceStyle::Numeric,
            page_break_before: true,
        }
    }
}

/// A citation source. Source IDs live in the key of [`ReportDocument::sources`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SourceEntry {
    /// Official publication-grade title of the document, announcement, or dataset. Required when registering a new
    /// source; may be omitted when partially updating an existing source by `id`.
    /// Never include internal MCP/warehouse module tags or English parenthetical suffixes (e.g. `（reporting）`, `（subject）`, `panorama_report`, `subjectcloud`).
    /// Good: `"2025年国家自然科学基金集中接收申请项目评审结果通告"` or `"青塔高等教育全景数据库 — 国家自然科学基金立项统计"`;
    /// Bad: `"国自然官网"`, `"青塔数据库（reporting）"`, `"青塔学科云（subject）"`.
    #[serde(default)]
    pub title: String,
    /// Direct canonical URL of the original webpage, PDF, or official announcement (never an internal query API URL).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Individual or institutional authors when explicitly named in the source.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    /// Full formal name of the publishing institution or platform (never a bare domain, URL, or internal MCP module name).
    /// Good: `"国家自然科学基金委员会"` or `"青塔教育"`; Bad: `"nsfc.gov.cn"`, `"reporting"`, `"subjectcloud"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    /// Original publication date, preferably ISO-8601 (`"2025-08-27"`). Do not put retrieval date here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// Date when the agent retrieved or verified this source, preferably ISO-8601 (`"2026-11-24"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accessed: Option<String>,
    /// Optional concise provenance note stating statistical scope, time window, or limitations in natural language.
    /// Good: `"统计口径：2025年国自然集中接收期资助项目，不含后期补充立项。"`; Bad: raw SQL/API params, internal MCP names, or module tags.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Optional short verbatim quote from one original prose/policy document to help a human reader
    /// verify a claim. Omit for structured database tables or when no crisp quote exists.
    /// Good: `"到2025年，浙江省力争新增博士学位授予单位2—3个，省属高校优势特色学科进入全国前列。"`;
    /// Bad: `"经查询数据库，浙江大学2025年立项..."` (never write query logs, derived analysis, or synthetic summaries).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

impl SourceEntry {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            url: None,
            authors: Vec::new(),
            publisher: None,
            date: None,
            accessed: None,
            note: None,
            excerpt: None,
        }
    }

    /// Extract a four-digit publication year if one can be found.
    pub fn year(&self) -> Option<&str> {
        let date = self.date.as_deref()?;
        let bytes = date.as_bytes();
        if bytes.len() < 4 {
            return None;
        }
        bytes.windows(4).enumerate().find_map(|(index, window)| {
            if window.iter().all(u8::is_ascii_digit) {
                let before_ok = index
                    .checked_sub(1)
                    .and_then(|previous| bytes.get(previous))
                    .is_none_or(|byte| !byte.is_ascii_digit());
                let after_ok = bytes
                    .get(index + 4)
                    .is_none_or(|byte| !byte.is_ascii_digit());
                if before_ok && after_ok {
                    return date.get(index..index + 4);
                }
            }
            None
        })
    }

    /// Short author label: first author's family name and an et-al marker when needed.
    pub fn author_label(&self) -> Option<String> {
        let first = self.authors.first()?.trim();
        if first.is_empty() {
            return None;
        }
        let family = first.split_whitespace().last().unwrap_or(first);
        let family = family.trim_end_matches(',');
        if self.authors.len() > 1 {
            Some(format!("{family} et al."))
        } else {
            Some(crate::text::truncate_chars(family, 40))
        }
    }
}

/// The root report document. Blocks are stored in display order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReportDocument {
    pub doc_id: DocId,
    /// Optional agent session used for frontend session discovery (`X-Session-Id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    /// Optional document-wide target for readable paragraph characters, agreed at creation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_prose_chars: Option<NonZeroU32>,
    #[serde(default)]
    pub revision: Revision,
    #[serde(default)]
    pub cover: CoverConfig,
    #[serde(default)]
    pub toc: TocConfig,
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    #[schemars(with = "std::collections::BTreeMap<String, SourceEntry>")]
    pub sources: IndexMap<SourceId, SourceEntry>,
    #[serde(default)]
    pub blocks: Vec<Block>,
    #[serde(default)]
    pub references: ReferencesConfig,
    #[serde(default)]
    pub lint: LintConfig,
}

impl ReportDocument {
    /// Create an empty document at revision 0.
    pub fn new(doc_id: DocId) -> Self {
        Self {
            doc_id,
            session_id: None,
            target_prose_chars: None,
            revision: Revision::ZERO,
            cover: CoverConfig::default(),
            toc: TocConfig::default(),
            theme: ThemeConfig::default(),
            sources: IndexMap::new(),
            blocks: Vec::new(),
            references: ReferencesConfig::default(),
            lint: LintConfig::default(),
        }
    }

    /// Document title: cover title, then first level-1 heading, then the document ID.
    pub fn title(&self) -> String {
        if !self.cover.title.trim().is_empty() {
            return self.cover.title.trim().to_string();
        }
        for block in &self.blocks {
            if let crate::block::BlockKind::Heading { level: 1, text } = &block.body
                && !text.trim().is_empty()
            {
                return text.trim().to_string();
            }
        }
        self.doc_id.to_string()
    }

    pub fn block(&self, block_id: &str) -> Option<&Block> {
        self.blocks.iter().find(|block| block.id == block_id)
    }

    pub fn block_mut(&mut self, block_id: &str) -> Option<&mut Block> {
        self.blocks.iter_mut().find(|block| block.id == block_id)
    }

    pub fn position(&self, block_id: &str) -> Option<usize> {
        self.blocks.iter().position(|block| block.id == block_id)
    }

    /// Validate basic document invariants without generating lint diagnostics.
    pub fn duplicate_block_ids(&self) -> Vec<crate::ids::BlockId> {
        let mut seen = std::collections::BTreeSet::new();
        let mut duplicates = Vec::new();
        for block in &self.blocks {
            if !seen.insert(block.id.clone()) {
                duplicates.push(block.id.clone());
            }
        }
        duplicates
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn year_extraction_prefers_standalone_four_digit_runs() {
        let mut source = SourceEntry::new("Example");
        source.date = Some("revised 2025-07-04".into());
        assert_eq!(source.year(), Some("2025"));
        source.date = Some("123456".into());
        assert_eq!(source.year(), None);
        source.date = Some("2024".into());
        assert_eq!(source.year(), Some("2024"));
    }

    #[test]
    fn token_safety_rejects_injection() {
        assert!(is_valid_css_token_name("--rpt-primary"));
        assert!(!is_valid_css_token_name("color"));
        assert!(is_safe_css_token_value("#2563eb"));
        assert!(!is_safe_css_token_value("#fff; body { display:none }"));
        assert!(!is_safe_css_token_value("url(</style>)"));
    }

    #[test]
    fn default_references_break_before_page() {
        assert!(ReferencesConfig::default().page_break_before);
    }

    #[test]
    fn toc_page_breaks_default_false_and_roundtrip() {
        let default = TocConfig::default();
        assert_eq!(default.variant, "dotted");
        assert!(!default.page_break_before);
        assert!(!default.page_break_after);

        let serialized = serde_json::to_value(&default).expect("serialize toc");
        assert_eq!(serialized["variant"], "dotted");
        assert_eq!(serialized["page_break_before"], false);
        assert_eq!(serialized["page_break_after"], false);

        let config: TocConfig = serde_json::from_value(serde_json::json!({
            "enabled": true,
            "title": "Contents",
            "max_depth": 2,
            "variant": "numbered",
            "page_break_before": true,
            "page_break_after": true
        }))
        .expect("toc config");
        assert!(config.page_break_before);
        assert!(config.page_break_after);
        assert_eq!(config.variant_key(), "numbered");
    }

    #[test]
    fn page_chrome_roundtrips_with_serde_defaults() {
        let value = serde_json::json!({
            "header": {"left": "{kicker}", "center": "", "right": "{title}"},
            "footer": {"left": "{authors}", "center": "{page}/{total}", "right": "{date}"}
        });
        let config: PageChromeConfig = serde_json::from_value(value).expect("page chrome config");
        assert!(config.hide_header_on_first_page);
        assert!(!config.hide_footer_on_first_page);
        assert_eq!(config.heading_page_break, HeadingPageBreak::None);
        assert_eq!(config.header.left, "{kicker}");
        let serialized = serde_json::to_value(&config).expect("serialize page chrome");
        assert_eq!(serialized["footer"]["center"], "{page}/{total}");
        assert_eq!(serialized["heading_page_break"], "none");
        assert!(ThemeConfig::default().page_chrome.is_none());
    }

    #[test]
    fn heading_page_break_defaults_and_roundtrips() {
        assert_eq!(HeadingPageBreak::default(), HeadingPageBreak::None);
        assert_eq!(
            serde_json::to_value(HeadingPageBreak::H1).expect("serialize h1"),
            serde_json::json!("h1")
        );
        assert_eq!(
            serde_json::to_value(HeadingPageBreak::H1AndH2).expect("serialize h1_and_h2"),
            serde_json::json!("h1_and_h2")
        );
        let config: PageChromeConfig =
            serde_json::from_value(serde_json::json!({"heading_page_break": "h1_and_h2"}))
                .expect("page chrome config");
        assert_eq!(config.heading_page_break, HeadingPageBreak::H1AndH2);
        let defaulted: PageChromeConfig =
            serde_json::from_value(serde_json::json!({})).expect("default page chrome");
        assert_eq!(defaulted.heading_page_break, HeadingPageBreak::None);
    }
}
