use std::collections::BTreeMap;

use chrono::{DateTime, TimeZone, Utc};
use quire_protocol::{
    CitationStyle, HeadingPageBreak, PageChromeConfig, PageChromeSlots, ThemeConfig,
    is_safe_css_token_value, is_valid_css_token_name, short_id, validate_custom_css,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, postgres::PgRow};

use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StylePresetBundle {
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toc_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toc_max_depth: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toc_variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toc_page_break_before: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toc_page_break_after: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation_style: Option<CitationStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub references_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub references_page_break_before: Option<bool>,
}

impl StylePresetBundle {
    pub fn validate(&self) -> RuntimeResult<()> {
        for (name, value) in &self.theme.tokens {
            if !is_valid_css_token_name(name) {
                return Err(RuntimeError::InvalidRequest(format!(
                    "无效的 CSS 变量名 `{name}`（必须以 `--` 开头，仅含字母、数字、`-` 或 `_`）"
                )));
            }
            if !is_safe_css_token_value(value) {
                return Err(RuntimeError::InvalidRequest(format!(
                    "CSS 变量 `{name}` 包含不安全字符"
                )));
            }
        }
        if !self.theme.custom_css.trim().is_empty() {
            validate_custom_css(&self.theme.custom_css)
                .map_err(|err| RuntimeError::InvalidRequest(err.to_string()))?;
        }
        if let Some(cover_variant) = &self.cover_variant
            && !quire_protocol::COVER_VARIANTS.contains(&cover_variant.as_str())
        {
            return Err(RuntimeError::InvalidRequest(format!(
                "无效的封面版式 `{cover_variant}`"
            )));
        }
        if let Some(toc_variant) = &self.toc_variant
            && !quire_protocol::TOC_VARIANTS.contains(&toc_variant.as_str())
        {
            return Err(RuntimeError::InvalidRequest(format!(
                "无效的目录版式 `{toc_variant}`"
            )));
        }
        if let Some(depth) = self.toc_max_depth
            && !(1..=3).contains(&depth)
        {
            return Err(RuntimeError::InvalidRequest(format!(
                "无效的目录层级深度 `{depth}`（仅支持 1..=3）"
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StylePresetRecord {
    pub style_id: String,
    pub name: String,
    pub description: String,
    pub category_tag: String,
    pub is_builtin: bool,
    pub is_public: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_user_id: Option<String>,
    pub owner_display_name: String,
    pub base_preset_id: String,
    pub bundle: StylePresetBundle,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct CreateStylePresetInput<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub category_tag: &'a str,
    pub is_public: bool,
    pub owner_user_id: &'a str,
    pub owner_display_name: &'a str,
    pub base_preset_id: &'a str,
    pub bundle: StylePresetBundle,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateStylePresetInput<'a> {
    pub name: Option<&'a str>,
    pub description: Option<&'a str>,
    pub category_tag: Option<&'a str>,
    pub is_public: Option<bool>,
    pub bundle: Option<StylePresetBundle>,
}

fn builtin_timestamp() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
        .single()
        .unwrap_or_else(Utc::now)
}

fn tokens_from_pairs(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

/// Built-in 10 distinct styles exploring the full spectrum of editorial & visual design.
pub fn builtin_style_presets() -> Vec<StylePresetRecord> {
    let ts = builtin_timestamp();
    vec![
        // 1. 灵思手札 — Borderless Notion-like exploratory notes, soft pill H2, minimal dividers
        StylePresetRecord {
            style_id: "field-notes".into(),
            name: "灵思手札".into(),
            description: "无下划线纯净大标题与松石绿柔和胶囊，暖米纸面，适配课题探索与随笔".into(),
            category_tag: "自由 · 随笔".into(),
            is_builtin: true,
            is_public: true,
            owner_user_id: None,
            owner_display_name: "Quire".into(),
            base_preset_id: "field-notes".into(),
            bundle: StylePresetBundle {
                theme: ThemeConfig {
                    preset: "field-notes".into(),
                    tokens: tokens_from_pairs(&[
                        ("--rpt-accent", "#0F766E"),
                        ("--rpt-ink", "#1C1917"),
                        ("--rpt-muted", "#57534E"),
                        ("--rpt-border", "#E7E5E4"),
                        ("--rpt-surface", "#EDF7F5"),
                        ("--rpt-paper", "#FAF8F3"),
                        ("--rpt-quote-bg", "#F0FDF4"),
                        ("--rpt-table-border", "#A7F3D0"),
                        ("--rpt-table-header-bg", "#ECFDF5"),
                        ("--rpt-table-stripe", "#F5F3EE"),
                        ("--rpt-line-height", "1.92"),
                        ("--rpt-body-size", "14.5px"),
                        ("--rpt-h1-size", "22px"),
                        ("--rpt-heading-weight", "700"),
                        ("--rpt-heading-tracking", "-0.01em"),
                        ("--rpt-para-gap", "1.05rem"),
                        ("--rpt-para-indent", "0em"),
                        ("--rpt-para-align", "left"),
                        ("--rpt-h1-variant", "none"),
                        ("--rpt-h2-variant", "soft-pill"),
                        ("--rpt-callout-variant", "solid-tint"),
                        ("--rpt-quote-variant", "editorial-mark"),
                        ("--rpt-table-variant", "minimal"),
                        ("--rpt-table-density", "relaxed"),
                        ("--rpt-radius", "12px"),
                        (
                            "--rpt-font-heading",
                            "\"Geist Variable\", \"PingFang SC\", \"Noto Sans SC\", system-ui, sans-serif",
                        ),
                        (
                            "--rpt-font-body",
                            "\"Geist Variable\", \"PingFang SC\", \"Noto Sans SC\", system-ui, sans-serif",
                        ),
                        ("--rpt-chart-1", "#0F766E"),
                        ("--rpt-chart-2", "#0284C7"),
                        ("--rpt-chart-3", "#D97706"),
                        ("--rpt-chart-4", "#65A30D"),
                        ("--rpt-chart-5", "#7C3AED"),
                        ("--rpt-chart-6", "#64748B"),
                    ]),
                    custom_css: String::new(),
                    page_chrome: Some(PageChromeConfig {
                        header: PageChromeSlots {
                            left: "{kicker}".into(),
                            center: String::new(),
                            right: "{date}".into(),
                        },
                        footer: PageChromeSlots {
                            left: "{title}".into(),
                            center: String::new(),
                            right: "{page} / {total}".into(),
                        },
                        hide_header_on_first_page: true,
                        hide_footer_on_first_page: false,
                        heading_page_break: HeadingPageBreak::None,
                    }),
                },
                cover_enabled: Some(true),
                cover_variant: Some("minimal".into()),
                toc_enabled: Some(true),
                toc_max_depth: Some(2),
                toc_variant: Some("standard".into()),
                toc_page_break_before: Some(false),
                toc_page_break_after: Some(false),
                citation_style: Some(CitationStyle::Numeric),
                references_enabled: Some(true),
                references_page_break_before: Some(false),
            },
            created_at: ts,
            updated_at: ts,
        },
        // 2. 智库简报 — McKinsey-style top-rule crown H1, geometric dot H2, top-banner callouts
        StylePresetRecord {
            style_id: "modern-brief".into(),
            name: "智库简报".into(),
            description: "顶冠短横线标题与几何方标导航，顶部色条提示卡，适配战略咨询与数据快报"
                .into(),
            category_tag: "轻快 · 咨询".into(),
            is_builtin: true,
            is_public: true,
            owner_user_id: None,
            owner_display_name: "Quire".into(),
            base_preset_id: "modern-brief".into(),
            bundle: StylePresetBundle {
                theme: ThemeConfig {
                    preset: "modern-brief".into(),
                    tokens: tokens_from_pairs(&[
                        ("--rpt-accent", "#1D4ED8"),
                        ("--rpt-ink", "#0F172A"),
                        ("--rpt-muted", "#475569"),
                        ("--rpt-border", "#CBD5E1"),
                        ("--rpt-surface", "#EFF6FF"),
                        ("--rpt-paper", "#FFFFFF"),
                        ("--rpt-quote-bg", "#F8FAFC"),
                        ("--rpt-table-border", "#1E293B"),
                        ("--rpt-table-header-bg", "#EFF6FF"),
                        ("--rpt-table-stripe", "#F8FAFC"),
                        ("--rpt-line-height", "1.76"),
                        ("--rpt-body-size", "14px"),
                        ("--rpt-h1-size", "22px"),
                        ("--rpt-heading-weight", "700"),
                        ("--rpt-heading-tracking", "-0.015em"),
                        ("--rpt-para-gap", "0.75rem"),
                        ("--rpt-para-indent", "0em"),
                        ("--rpt-para-align", "left"),
                        ("--rpt-h1-variant", "top-rule"),
                        ("--rpt-h2-variant", "dot-prefix"),
                        ("--rpt-callout-variant", "top-banner"),
                        ("--rpt-quote-variant", "left-rule"),
                        ("--rpt-table-variant", "striped"),
                        ("--rpt-table-density", "standard"),
                        ("--rpt-radius", "6px"),
                        (
                            "--rpt-font-heading",
                            "\"Geist Variable\", \"PingFang SC\", \"Noto Sans SC\", system-ui, sans-serif",
                        ),
                        (
                            "--rpt-font-body",
                            "\"Geist Variable\", \"PingFang SC\", \"Noto Sans SC\", system-ui, sans-serif",
                        ),
                        ("--rpt-chart-1", "#1D4ED8"),
                        ("--rpt-chart-2", "#0D9488"),
                        ("--rpt-chart-3", "#3B82F6"),
                        ("--rpt-chart-4", "#F59E0B"),
                        ("--rpt-chart-5", "#6366F1"),
                        ("--rpt-chart-6", "#64748B"),
                    ]),
                    custom_css: String::new(),
                    page_chrome: Some(PageChromeConfig {
                        header: PageChromeSlots {
                            left: "{kicker}".into(),
                            center: String::new(),
                            right: "{title}".into(),
                        },
                        footer: PageChromeSlots {
                            left: "{authors}".into(),
                            center: String::new(),
                            right: "P.{page} / {total}".into(),
                        },
                        hide_header_on_first_page: true,
                        hide_footer_on_first_page: false,
                        heading_page_break: HeadingPageBreak::None,
                    }),
                },
                cover_enabled: Some(true),
                cover_variant: Some("modern".into()),
                toc_enabled: Some(true),
                toc_max_depth: Some(3),
                toc_variant: Some("numbered".into()),
                toc_page_break_before: Some(false),
                toc_page_break_after: Some(false),
                citation_style: Some(CitationStyle::Numeric),
                references_enabled: Some(true),
                references_page_break_before: Some(false),
            },
            created_at: ts,
            updated_at: ts,
        },
        // 3. 青塔研报 — Default flagship style: Left pillar H1, serif heading + sans body, split cover
        StylePresetRecord {
            style_id: "quire-folio".into(),
            name: "青塔研报".into(),
            description: "牛津蓝厚立柱章节头、人文衬线标题配无衬线正文，兼顾数字阅读与 A4 归档"
                .into(),
            category_tag: "均衡 · 标杆".into(),
            is_builtin: true,
            is_public: true,
            owner_user_id: None,
            owner_display_name: "Quire".into(),
            base_preset_id: "quire-folio".into(),
            bundle: StylePresetBundle {
                theme: ThemeConfig {
                    preset: "quire-folio".into(),
                    tokens: tokens_from_pairs(&[
                        ("--rpt-accent", "#1E3A5F"),
                        ("--rpt-ink", "#18181B"),
                        ("--rpt-muted", "#575651"),
                        ("--rpt-border", "#D6D4CD"),
                        ("--rpt-surface", "#F7F6F2"),
                        ("--rpt-paper", "#FFFFFF"),
                        ("--rpt-quote-bg", "#F8FAFC"),
                        ("--rpt-table-border", "#1E293B"),
                        ("--rpt-table-header-bg", "#F1F5F9"),
                        ("--rpt-table-stripe", "#F8FAFC"),
                        ("--rpt-line-height", "1.82"),
                        ("--rpt-body-size", "14.5px"),
                        ("--rpt-h1-size", "22px"),
                        ("--rpt-heading-weight", "700"),
                        ("--rpt-heading-tracking", "0em"),
                        ("--rpt-para-gap", "0.75rem"),
                        ("--rpt-para-indent", "0em"),
                        ("--rpt-para-align", "left"),
                        ("--rpt-h1-variant", "left-pillar"),
                        ("--rpt-h2-variant", "left-bar"),
                        ("--rpt-callout-variant", "left-accent"),
                        ("--rpt-quote-variant", "left-rule"),
                        ("--rpt-table-variant", "three-line"),
                        ("--rpt-table-density", "standard"),
                        ("--rpt-radius", "6px"),
                        (
                            "--rpt-font-heading",
                            "\"Noto Serif SC\", \"Songti SC\", \"STSong\", \"SimSun\", Georgia, serif",
                        ),
                        (
                            "--rpt-font-body",
                            "\"Geist Variable\", \"PingFang SC\", \"Noto Sans SC\", system-ui, sans-serif",
                        ),
                        ("--rpt-chart-1", "#1E3A5F"),
                        ("--rpt-chart-2", "#3B6E9C"),
                        ("--rpt-chart-3", "#6495B8"),
                        ("--rpt-chart-4", "#94B4CE"),
                        ("--rpt-chart-5", "#9A6B2F"),
                        ("--rpt-chart-6", "#475569"),
                    ]),
                    custom_css: String::new(),
                    page_chrome: Some(PageChromeConfig {
                        header: PageChromeSlots {
                            left: "{kicker}".into(),
                            center: String::new(),
                            right: "{title}".into(),
                        },
                        footer: PageChromeSlots {
                            left: "{authors}".into(),
                            center: "第 {page} 页 / 共 {total} 页".into(),
                            right: "{date}".into(),
                        },
                        hide_header_on_first_page: true,
                        hide_footer_on_first_page: false,
                        heading_page_break: HeadingPageBreak::None,
                    }),
                },
                cover_enabled: Some(true),
                cover_variant: Some("split".into()),
                toc_enabled: Some(true),
                toc_max_depth: Some(3),
                toc_variant: Some("dotted".into()),
                toc_page_break_before: Some(false),
                toc_page_break_after: Some(false),
                citation_style: Some(CitationStyle::Numeric),
                references_enabled: Some(true),
                references_page_break_before: Some(false),
            },
            created_at: ts,
            updated_at: ts,
        },
        // 4. 学林刊印 — Academic Monograph: Centered ornament H1, unadorned H2, 2em indent, justify, pure 3-line table, author_year
        StylePresetRecord {
            style_id: "academic-press".into(),
            name: "学林刊印".into(),
            description: "居中刊头章节、首行缩进两字符与两端对齐宋体，纯白三线表与著者年份引文"
                .into(),
            category_tag: "严谨 · 期刊".into(),
            is_builtin: true,
            is_public: true,
            owner_user_id: None,
            owner_display_name: "Quire".into(),
            base_preset_id: "academic-press".into(),
            bundle: StylePresetBundle {
                theme: ThemeConfig {
                    preset: "academic-press".into(),
                    tokens: tokens_from_pairs(&[
                        ("--rpt-accent", "#27374D"),
                        ("--rpt-ink", "#111111"),
                        ("--rpt-muted", "#4B5563"),
                        ("--rpt-border", "#9CA3AF"),
                        ("--rpt-surface", "#F9FAFB"),
                        ("--rpt-paper", "#FFFFFF"),
                        ("--rpt-quote-bg", "#FFFFFF"),
                        ("--rpt-table-border", "#111111"),
                        ("--rpt-table-header-bg", "#FFFFFF"),
                        ("--rpt-table-stripe", "#FFFFFF"),
                        ("--rpt-line-height", "1.76"),
                        ("--rpt-body-size", "14px"),
                        ("--rpt-h1-size", "20px"),
                        ("--rpt-heading-weight", "700"),
                        ("--rpt-heading-tracking", "0.02em"),
                        ("--rpt-para-gap", "0.55rem"),
                        ("--rpt-para-indent", "2em"),
                        ("--rpt-para-align", "justify"),
                        ("--rpt-h1-variant", "center-ornament"),
                        ("--rpt-h2-variant", "none"),
                        ("--rpt-callout-variant", "outline-frame"),
                        ("--rpt-quote-variant", "boxed"),
                        ("--rpt-table-variant", "three-line"),
                        ("--rpt-table-density", "compact"),
                        ("--rpt-radius", "0px"),
                        (
                            "--rpt-font-heading",
                            "\"Noto Serif SC\", \"Songti SC\", \"STSong\", \"SimSun\", Georgia, serif",
                        ),
                        (
                            "--rpt-font-body",
                            "\"Noto Serif SC\", \"Songti SC\", \"STSong\", \"SimSun\", Georgia, serif",
                        ),
                        ("--rpt-chart-1", "#27374D"),
                        ("--rpt-chart-2", "#526D82"),
                        ("--rpt-chart-3", "#8E7AB5"),
                        ("--rpt-chart-4", "#3F72AF"),
                        ("--rpt-chart-5", "#7F8487"),
                        ("--rpt-chart-6", "#9DB2BF"),
                    ]),
                    custom_css: String::new(),
                    page_chrome: Some(PageChromeConfig {
                        header: PageChromeSlots {
                            left: "{title}".into(),
                            center: String::new(),
                            right: "{kicker}".into(),
                        },
                        footer: PageChromeSlots {
                            left: String::new(),
                            center: "— {page} —".into(),
                            right: String::new(),
                        },
                        hide_header_on_first_page: true,
                        hide_footer_on_first_page: false,
                        heading_page_break: HeadingPageBreak::H1,
                    }),
                },
                cover_enabled: Some(true),
                cover_variant: Some("classic".into()),
                toc_enabled: Some(true),
                toc_max_depth: Some(3),
                toc_variant: Some("dotted".into()),
                toc_page_break_before: Some(false),
                toc_page_break_after: Some(false),
                citation_style: Some(CitationStyle::AuthorYear),
                references_enabled: Some(true),
                references_page_break_before: Some(true),
            },
            created_at: ts,
            updated_at: ts,
        },
        // 5. 丹宸公文 — Formal Red-Header Decree: Double red rule H1, pure serif H2, 2em indent, 15px
        StylePresetRecord {
            style_id: "crimson-decree".into(),
            name: "丹宸公文".into(),
            description: "朱砂红文武双线、首行缩进两字符与 15px 宋体，适配呈报校领导决策参阅件"
                .into(),
            category_tag: "正式 · 红头".into(),
            is_builtin: true,
            is_public: true,
            owner_user_id: None,
            owner_display_name: "Quire".into(),
            base_preset_id: "crimson-decree".into(),
            bundle: StylePresetBundle {
                theme: ThemeConfig {
                    preset: "crimson-decree".into(),
                    tokens: tokens_from_pairs(&[
                        ("--rpt-accent", "#991B1B"),
                        ("--rpt-ink", "#0A0A0A"),
                        ("--rpt-muted", "#525252"),
                        ("--rpt-border", "#D4D4D4"),
                        ("--rpt-surface", "#FEF2F2"),
                        ("--rpt-paper", "#FFFFFF"),
                        ("--rpt-quote-bg", "#FFFBFB"),
                        ("--rpt-table-border", "#171717"),
                        ("--rpt-table-header-bg", "#FAFAFA"),
                        ("--rpt-table-stripe", "#FFFFFF"),
                        ("--rpt-line-height", "1.92"),
                        ("--rpt-body-size", "15px"),
                        ("--rpt-h1-size", "22px"),
                        ("--rpt-heading-weight", "700"),
                        ("--rpt-heading-tracking", "0.02em"),
                        ("--rpt-para-gap", "0.75rem"),
                        ("--rpt-para-indent", "2em"),
                        ("--rpt-para-align", "justify"),
                        ("--rpt-h1-variant", "double-decree"),
                        ("--rpt-h2-variant", "none"),
                        ("--rpt-callout-variant", "left-accent"),
                        ("--rpt-quote-variant", "boxed"),
                        ("--rpt-table-variant", "three-line"),
                        ("--rpt-table-density", "relaxed"),
                        ("--rpt-radius", "0px"),
                        (
                            "--rpt-font-heading",
                            "\"Noto Serif SC\", \"Songti SC\", \"STSong\", \"SimSun\", Georgia, serif",
                        ),
                        (
                            "--rpt-font-body",
                            "\"Noto Serif SC\", \"Songti SC\", \"STSong\", \"PingFang SC\", serif",
                        ),
                        ("--rpt-chart-1", "#991B1B"),
                        ("--rpt-chart-2", "#1E3A5F"),
                        ("--rpt-chart-3", "#B45309"),
                        ("--rpt-chart-4", "#334155"),
                        ("--rpt-chart-5", "#DC2626"),
                        ("--rpt-chart-6", "#78716C"),
                    ]),
                    custom_css: String::new(),
                    page_chrome: Some(PageChromeConfig {
                        header: PageChromeSlots {
                            left: "{kicker}".into(),
                            center: String::new(),
                            right: "{title}".into(),
                        },
                        footer: PageChromeSlots {
                            left: "{authors}".into(),
                            center: "— 第 {page} 页 / 共 {total} 页 —".into(),
                            right: "{date}".into(),
                        },
                        hide_header_on_first_page: true,
                        hide_footer_on_first_page: false,
                        heading_page_break: HeadingPageBreak::H1,
                    }),
                },
                cover_enabled: Some(true),
                cover_variant: Some("classic".into()),
                toc_enabled: Some(true),
                toc_max_depth: Some(3),
                toc_variant: Some("dotted".into()),
                toc_page_break_before: Some(false),
                toc_page_break_after: Some(true),
                citation_style: Some(CitationStyle::Numeric),
                references_enabled: Some(true),
                references_page_break_before: Some(true),
            },
            created_at: ts,
            updated_at: ts,
        },
        // 6. 瑞士网格 — Swiss International Brutalist Grid: Full-bleed dark banner H1, signal orange, 0px radius, full grid table
        StylePresetRecord {
            style_id: "swiss-grid".into(),
            name: "瑞士网格".into(),
            description: "黑底反白满幅章节横幅、信号橙几何构成与零圆角全网格表，现代主义视觉张力"
                .into(),
            category_tag: "先锋 · 构成".into(),
            is_builtin: true,
            is_public: true,
            owner_user_id: None,
            owner_display_name: "Quire".into(),
            base_preset_id: "swiss-grid".into(),
            bundle: StylePresetBundle {
                theme: ThemeConfig {
                    preset: "swiss-grid".into(),
                    tokens: tokens_from_pairs(&[
                        ("--rpt-accent", "#EA580C"),
                        ("--rpt-ink", "#09090B"),
                        ("--rpt-muted", "#52525B"),
                        ("--rpt-border", "#18181B"),
                        ("--rpt-surface", "#F4F4F5"),
                        ("--rpt-paper", "#FFFFFF"),
                        ("--rpt-quote-bg", "#FFF7ED"),
                        ("--rpt-table-border", "#09090B"),
                        ("--rpt-table-header-bg", "#F4F4F5"),
                        ("--rpt-table-stripe", "#FAFAFA"),
                        ("--rpt-line-height", "1.68"),
                        ("--rpt-body-size", "14px"),
                        ("--rpt-h1-size", "22px"),
                        ("--rpt-heading-weight", "800"),
                        ("--rpt-heading-tracking", "-0.02em"),
                        ("--rpt-para-gap", "0.75rem"),
                        ("--rpt-para-indent", "0em"),
                        ("--rpt-para-align", "left"),
                        ("--rpt-h1-variant", "pill-banner"),
                        ("--rpt-h2-variant", "top-rule"),
                        ("--rpt-callout-variant", "outline-frame"),
                        ("--rpt-quote-variant", "left-rule"),
                        ("--rpt-table-variant", "grid"),
                        ("--rpt-table-density", "compact"),
                        ("--rpt-radius", "0px"),
                        (
                            "--rpt-font-heading",
                            "\"Geist Variable\", \"Helvetica Neue\", \"PingFang SC\", sans-serif",
                        ),
                        (
                            "--rpt-font-body",
                            "\"Geist Variable\", \"Helvetica Neue\", \"PingFang SC\", sans-serif",
                        ),
                        ("--rpt-chart-1", "#EA580C"),
                        ("--rpt-chart-2", "#09090B"),
                        ("--rpt-chart-3", "#2563EB"),
                        ("--rpt-chart-4", "#71717A"),
                        ("--rpt-chart-5", "#D97706"),
                        ("--rpt-chart-6", "#0D9488"),
                    ]),
                    custom_css: String::new(),
                    page_chrome: Some(PageChromeConfig {
                        header: PageChromeSlots {
                            left: "SWISS GRID // {kicker}".into(),
                            center: String::new(),
                            right: "{date}".into(),
                        },
                        footer: PageChromeSlots {
                            left: "{title}".into(),
                            center: String::new(),
                            right: "[ {page} / {total} ]".into(),
                        },
                        hide_header_on_first_page: false,
                        hide_footer_on_first_page: false,
                        heading_page_break: HeadingPageBreak::None,
                    }),
                },
                cover_enabled: Some(true),
                cover_variant: Some("split".into()),
                toc_enabled: Some(true),
                toc_max_depth: Some(2),
                toc_variant: Some("numbered".into()),
                toc_page_break_before: Some(false),
                toc_page_break_after: Some(false),
                citation_style: Some(CitationStyle::Numeric),
                references_enabled: Some(true),
                references_page_break_before: Some(false),
            },
            created_at: ts,
            updated_at: ts,
        },
        // 7. 工程蓝图 — Technical Blueprint & Quantitative Ledger: Steel-blue paper, mono typography, full grid ledger
        StylePresetRecord {
            style_id: "blueprint-ledger".into(),
            name: "工程蓝图".into(),
            description:
                "冷钢蓝图纸底色、技术等宽字族与全网格精密账本，适配经费决算与实验室指标盘点".into(),
            category_tag: "精密 · 档案".into(),
            is_builtin: true,
            is_public: true,
            owner_user_id: None,
            owner_display_name: "Quire".into(),
            base_preset_id: "blueprint-ledger".into(),
            bundle: StylePresetBundle {
                theme: ThemeConfig {
                    preset: "blueprint-ledger".into(),
                    tokens: tokens_from_pairs(&[
                        ("--rpt-accent", "#0369A1"),
                        ("--rpt-ink", "#0C4A6E"),
                        ("--rpt-muted", "#475569"),
                        ("--rpt-border", "#BAE6FD"),
                        ("--rpt-surface", "#E0F2FE"),
                        ("--rpt-paper", "#F4F8FB"),
                        ("--rpt-quote-bg", "#E0F2FE"),
                        ("--rpt-table-border", "#0284C7"),
                        ("--rpt-table-header-bg", "#E0F2FE"),
                        ("--rpt-table-stripe", "#EDF5FA"),
                        ("--rpt-line-height", "1.76"),
                        ("--rpt-body-size", "13.5px"),
                        ("--rpt-h1-size", "20px"),
                        ("--rpt-heading-weight", "700"),
                        ("--rpt-heading-tracking", "0.02em"),
                        ("--rpt-para-gap", "0.75rem"),
                        ("--rpt-para-indent", "0em"),
                        ("--rpt-para-align", "left"),
                        ("--rpt-h1-variant", "pill-banner"),
                        ("--rpt-h2-variant", "dot-prefix"),
                        ("--rpt-callout-variant", "outline-frame"),
                        ("--rpt-quote-variant", "boxed"),
                        ("--rpt-table-variant", "grid"),
                        ("--rpt-table-density", "compact"),
                        ("--rpt-radius", "4px"),
                        (
                            "--rpt-font-heading",
                            "\"JetBrains Mono\", \"Geist Mono\", \"PingFang SC\", monospace",
                        ),
                        (
                            "--rpt-font-body",
                            "\"JetBrains Mono\", \"Geist Variable\", \"PingFang SC\", monospace",
                        ),
                        ("--rpt-chart-1", "#0369A1"),
                        ("--rpt-chart-2", "#0284C7"),
                        ("--rpt-chart-3", "#0D9488"),
                        ("--rpt-chart-4", "#6366F1"),
                        ("--rpt-chart-5", "#D97706"),
                        ("--rpt-chart-6", "#64748B"),
                    ]),
                    custom_css: String::new(),
                    page_chrome: Some(PageChromeConfig {
                        header: PageChromeSlots {
                            left: "SPEC // {kicker}".into(),
                            center: String::new(),
                            right: "{title}".into(),
                        },
                        footer: PageChromeSlots {
                            left: "{authors}".into(),
                            center: "SHEET {page} OF {total}".into(),
                            right: "{date}".into(),
                        },
                        hide_header_on_first_page: false,
                        hide_footer_on_first_page: false,
                        heading_page_break: HeadingPageBreak::None,
                    }),
                },
                cover_enabled: Some(true),
                cover_variant: Some("modern".into()),
                toc_enabled: Some(true),
                toc_max_depth: Some(3),
                toc_variant: Some("numbered".into()),
                toc_page_break_before: Some(false),
                toc_page_break_after: Some(false),
                citation_style: Some(CitationStyle::Numeric),
                references_enabled: Some(true),
                references_page_break_before: Some(false),
            },
            created_at: ts,
            updated_at: ts,
        },
        // 8. 琥珀画报 — Monocle / Economist Magazine Feature: Warm cream paper, amber-burgundy, centered ornament H1, huge editorial quotes
        StylePresetRecord {
            style_id: "monocle-magazine".into(),
            name: "琥珀画报".into(),
            description: "羊皮奶油画报纸面、琥珀金居中特稿标题与巨型装饰引号，极具人文杂志叙事感"
                .into(),
            category_tag: "杂志 · 叙事".into(),
            is_builtin: true,
            is_public: true,
            owner_user_id: None,
            owner_display_name: "Quire".into(),
            base_preset_id: "monocle-magazine".into(),
            bundle: StylePresetBundle {
                theme: ThemeConfig {
                    preset: "monocle-magazine".into(),
                    tokens: tokens_from_pairs(&[
                        ("--rpt-accent", "#9A3412"),
                        ("--rpt-ink", "#27221D"),
                        ("--rpt-muted", "#78716C"),
                        ("--rpt-border", "#E7DEC8"),
                        ("--rpt-surface", "#F5EFE4"),
                        ("--rpt-paper", "#FBF7F0"),
                        ("--rpt-quote-bg", "#F5EFE4"),
                        ("--rpt-table-border", "#9A3412"),
                        ("--rpt-table-header-bg", "#F5EFE4"),
                        ("--rpt-table-stripe", "#F8F3E9"),
                        ("--rpt-line-height", "1.92"),
                        ("--rpt-body-size", "15px"),
                        ("--rpt-h1-size", "24px"),
                        ("--rpt-heading-weight", "700"),
                        ("--rpt-heading-tracking", "0.04em"),
                        ("--rpt-para-gap", "1.05rem"),
                        ("--rpt-para-indent", "0em"),
                        ("--rpt-para-align", "justify"),
                        ("--rpt-h1-variant", "center-ornament"),
                        ("--rpt-h2-variant", "none"),
                        ("--rpt-callout-variant", "top-banner"),
                        ("--rpt-quote-variant", "editorial-mark"),
                        ("--rpt-table-variant", "minimal"),
                        ("--rpt-table-density", "relaxed"),
                        ("--rpt-radius", "6px"),
                        (
                            "--rpt-font-heading",
                            "\"Noto Serif SC\", \"Songti SC\", \"Playfair Display\", Georgia, serif",
                        ),
                        (
                            "--rpt-font-body",
                            "\"Noto Serif SC\", \"Songti SC\", Georgia, serif",
                        ),
                        ("--rpt-chart-1", "#9A3412"),
                        ("--rpt-chart-2", "#D97706"),
                        ("--rpt-chart-3", "#1E3A5F"),
                        ("--rpt-chart-4", "#0F766E"),
                        ("--rpt-chart-5", "#78716C"),
                        ("--rpt-chart-6", "#B45309"),
                    ]),
                    custom_css: String::new(),
                    page_chrome: Some(PageChromeConfig {
                        header: PageChromeSlots {
                            left: String::new(),
                            center: "{kicker}".into(),
                            right: String::new(),
                        },
                        footer: PageChromeSlots {
                            left: "{authors}".into(),
                            center: "No. {page}".into(),
                            right: "{date}".into(),
                        },
                        hide_header_on_first_page: true,
                        hide_footer_on_first_page: false,
                        heading_page_break: HeadingPageBreak::None,
                    }),
                },
                cover_enabled: Some(true),
                cover_variant: Some("classic".into()),
                toc_enabled: Some(true),
                toc_max_depth: Some(2),
                toc_variant: Some("dotted".into()),
                toc_page_break_before: Some(false),
                toc_page_break_after: Some(false),
                citation_style: Some(CitationStyle::AuthorYear),
                references_enabled: Some(true),
                references_page_break_before: Some(false),
            },
            created_at: ts,
            updated_at: ts,
        },
        // 9. 曜石终端 — Dark Obsidian Terminal: Deep slate-black canvas, emerald neon accents, dark quantitative grid
        StylePresetRecord {
            style_id: "cyber-terminal".into(),
            name: "曜石终端".into(),
            description:
                "突破白纸局限的深邃曜石暗色画布、荧光翠绿立柱与霓虹多维图表，适配大屏演示与数字看板"
                    .into(),
            category_tag: "暗色 · 极客".into(),
            is_builtin: true,
            is_public: true,
            owner_user_id: None,
            owner_display_name: "Quire".into(),
            base_preset_id: "cyber-terminal".into(),
            bundle: StylePresetBundle {
                theme: ThemeConfig {
                    preset: "cyber-terminal".into(),
                    tokens: tokens_from_pairs(&[
                        ("--rpt-accent", "#10B981"),
                        ("--rpt-ink", "#E2E8F0"),
                        ("--rpt-muted", "#94A3B8"),
                        ("--rpt-border", "#263345"),
                        ("--rpt-surface", "#16202E"),
                        ("--rpt-paper", "#0F141C"),
                        ("--rpt-quote-bg", "#131C28"),
                        ("--rpt-table-border", "#334155"),
                        ("--rpt-table-header-bg", "#1E293B"),
                        ("--rpt-table-stripe", "#131B26"),
                        ("--rpt-line-height", "1.82"),
                        ("--rpt-body-size", "14px"),
                        ("--rpt-h1-size", "22px"),
                        ("--rpt-heading-weight", "700"),
                        ("--rpt-heading-tracking", "0.01em"),
                        ("--rpt-para-gap", "0.75rem"),
                        ("--rpt-para-indent", "0em"),
                        ("--rpt-para-align", "left"),
                        ("--rpt-h1-variant", "left-pillar"),
                        ("--rpt-h2-variant", "dot-prefix"),
                        ("--rpt-callout-variant", "outline-frame"),
                        ("--rpt-quote-variant", "left-rule"),
                        ("--rpt-table-variant", "grid"),
                        ("--rpt-table-density", "compact"),
                        ("--rpt-radius", "6px"),
                        (
                            "--rpt-font-heading",
                            "\"JetBrains Mono\", \"Geist Variable\", \"PingFang SC\", monospace",
                        ),
                        (
                            "--rpt-font-body",
                            "\"Geist Variable\", \"PingFang SC\", \"Noto Sans SC\", system-ui, sans-serif",
                        ),
                        ("--rpt-chart-1", "#10B981"),
                        ("--rpt-chart-2", "#38BDF8"),
                        ("--rpt-chart-3", "#F59E0B"),
                        ("--rpt-chart-4", "#A78BFA"),
                        ("--rpt-chart-5", "#F43F5E"),
                        ("--rpt-chart-6", "#2DD4BF"),
                    ]),
                    custom_css: String::new(),
                    page_chrome: Some(PageChromeConfig {
                        header: PageChromeSlots {
                            left: "TERMINAL // {kicker}".into(),
                            center: String::new(),
                            right: "{date}".into(),
                        },
                        footer: PageChromeSlots {
                            left: "{title}".into(),
                            center: String::new(),
                            right: "PAGE {page} / {total}".into(),
                        },
                        hide_header_on_first_page: true,
                        hide_footer_on_first_page: false,
                        heading_page_break: HeadingPageBreak::None,
                    }),
                },
                cover_enabled: Some(true),
                cover_variant: Some("modern".into()),
                toc_enabled: Some(true),
                toc_max_depth: Some(3),
                toc_variant: Some("numbered".into()),
                toc_page_break_before: Some(false),
                toc_page_break_after: Some(false),
                citation_style: Some(CitationStyle::Numeric),
                references_enabled: Some(true),
                references_page_break_before: Some(false),
            },
            created_at: ts,
            updated_at: ts,
        },
        // 10. 竹韵书院 — Oriental Academy & Bamboo Scroll: Xuan paper, bamboo green, Kaiti/Songti humanistic calligraphy, 2.05 line-height
        StylePresetRecord {
            style_id: "bamboo-scroll".into(),
            name: "竹韵书院".into(),
            description:
                "温润宣纸底色、竹青黛绿卷首刊头与楷宋人文排版，适配文史哲学科评估与书院专题".into(),
            category_tag: "东方 · 雅集".into(),
            is_builtin: true,
            is_public: true,
            owner_user_id: None,
            owner_display_name: "Quire".into(),
            base_preset_id: "bamboo-scroll".into(),
            bundle: StylePresetBundle {
                theme: ThemeConfig {
                    preset: "bamboo-scroll".into(),
                    tokens: tokens_from_pairs(&[
                        ("--rpt-accent", "#2D5A4C"),
                        ("--rpt-ink", "#1F2421"),
                        ("--rpt-muted", "#6B7068"),
                        ("--rpt-border", "#D5CFC0"),
                        ("--rpt-surface", "#EFECE2"),
                        ("--rpt-paper", "#F7F4EB"),
                        ("--rpt-quote-bg", "#EFECE2"),
                        ("--rpt-table-border", "#2D5A4C"),
                        ("--rpt-table-header-bg", "#EFECE2"),
                        ("--rpt-table-stripe", "#F3F0E6"),
                        ("--rpt-line-height", "2.05"),
                        ("--rpt-body-size", "15px"),
                        ("--rpt-h1-size", "24px"),
                        ("--rpt-heading-weight", "700"),
                        ("--rpt-heading-tracking", "0.08em"),
                        ("--rpt-para-gap", "1.05rem"),
                        ("--rpt-para-indent", "2em"),
                        ("--rpt-para-align", "justify"),
                        ("--rpt-h1-variant", "center-ornament"),
                        ("--rpt-h2-variant", "soft-pill"),
                        ("--rpt-callout-variant", "outline-frame"),
                        ("--rpt-quote-variant", "boxed"),
                        ("--rpt-table-variant", "three-line"),
                        ("--rpt-table-density", "relaxed"),
                        ("--rpt-radius", "6px"),
                        (
                            "--rpt-font-heading",
                            "\"STKaiti\", \"Kaiti SC\", \"KaiTi\", \"Noto Serif SC\", \"Songti SC\", serif",
                        ),
                        (
                            "--rpt-font-body",
                            "\"Noto Serif SC\", \"Songti SC\", \"STSong\", \"STKaiti\", Georgia, serif",
                        ),
                        ("--rpt-chart-1", "#2D5A4C"),
                        ("--rpt-chart-2", "#B45309"),
                        ("--rpt-chart-3", "#3B6E9C"),
                        ("--rpt-chart-4", "#6D4C41"),
                        ("--rpt-chart-5", "#557A6E"),
                        ("--rpt-chart-6", "#8D6E63"),
                    ]),
                    custom_css: String::new(),
                    page_chrome: Some(PageChromeConfig {
                        header: PageChromeSlots {
                            left: "{kicker}".into(),
                            center: String::new(),
                            right: "{title}".into(),
                        },
                        footer: PageChromeSlots {
                            left: "{authors}".into(),
                            center: "卷叶 · 第 {page} 页 / 共 {total} 页".into(),
                            right: "{date}".into(),
                        },
                        hide_header_on_first_page: true,
                        hide_footer_on_first_page: false,
                        heading_page_break: HeadingPageBreak::H1,
                    }),
                },
                cover_enabled: Some(true),
                cover_variant: Some("classic".into()),
                toc_enabled: Some(true),
                toc_max_depth: Some(3),
                toc_variant: Some("dotted".into()),
                toc_page_break_before: Some(false),
                toc_page_break_after: Some(false),
                citation_style: Some(CitationStyle::Numeric),
                references_enabled: Some(true),
                references_page_break_before: Some(false),
            },
            created_at: ts,
            updated_at: ts,
        },
    ]
}

pub fn find_builtin_style_preset(style_id: &str) -> Option<StylePresetRecord> {
    builtin_style_presets()
        .into_iter()
        .find(|preset| preset.style_id == style_id)
}

impl RuntimeStore {
    pub async fn list_style_presets_for_user(
        &self,
        user_id: &str,
    ) -> RuntimeResult<Vec<StylePresetRecord>> {
        let mut presets = builtin_style_presets();
        let rows = sqlx::query(
            r#"SELECT style_id, name, description, category_tag, is_public,
                      owner_user_id, owner_display_name, base_preset_id, bundle,
                      created_at, updated_at
               FROM catalog.style_presets
               WHERE is_public = TRUE OR owner_user_id = $1
               ORDER BY updated_at DESC, style_id ASC"#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        for row in &rows {
            presets.push(row_to_style_preset(row)?);
        }
        Ok(presets)
    }

    pub async fn get_style_preset(
        &self,
        style_id: &str,
    ) -> RuntimeResult<Option<StylePresetRecord>> {
        if let Some(builtin) = find_builtin_style_preset(style_id) {
            return Ok(Some(builtin));
        }
        let row = sqlx::query(
            r#"SELECT style_id, name, description, category_tag, is_public,
                      owner_user_id, owner_display_name, base_preset_id, bundle,
                      created_at, updated_at
               FROM catalog.style_presets
               WHERE style_id = $1"#,
        )
        .bind(style_id.trim())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(row_to_style_preset).transpose()
    }

    pub async fn set_default_style_preset(
        &self,
        user_id: &str,
        style_id: &str,
        is_admin: bool,
    ) -> RuntimeResult<()> {
        let record = self
            .get_style_preset(style_id)
            .await?
            .ok_or_else(|| RuntimeError::InvalidRequest(format!("样式 `{style_id}` 不存在")))?;
        if !record.is_builtin
            && !record.is_public
            && !is_admin
            && record.owner_user_id.as_deref() != Some(user_id)
        {
            return Err(RuntimeError::Forbidden("无权使用该私有样式".into()));
        }
        let result = sqlx::query(
            "UPDATE identity.users SET default_style_id = $2, updated_at = NOW() WHERE user_id = $1",
        )
        .bind(user_id)
        .bind(record.style_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(RuntimeError::InvalidRequest("user not found".into()));
        }
        Ok(())
    }

    pub async fn default_style_preset_for_user(
        &self,
        user_id: &str,
    ) -> RuntimeResult<StylePresetRecord> {
        let style_id = sqlx::query_scalar::<_, String>(
            "SELECT default_style_id FROM identity.users WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        if let Some(record) = self.get_style_preset(&style_id).await?
            && (record.is_builtin
                || record.is_public
                || record.owner_user_id.as_deref() == Some(user_id))
        {
            return Ok(record);
        }
        find_builtin_style_preset("quire-folio").ok_or_else(|| {
            RuntimeError::Internal("missing built-in quire-folio style preset".into())
        })
    }

    pub async fn create_style_preset(
        &self,
        input: CreateStylePresetInput<'_>,
    ) -> RuntimeResult<StylePresetRecord> {
        let name = input.name.trim();
        if name.is_empty() || name.chars().count() > 48 {
            return Err(RuntimeError::InvalidRequest(
                "样式名称不能为空且不超过 48 个字符".into(),
            ));
        }
        input.bundle.validate()?;
        let style_id = short_id("stl");
        let category_tag = if input.category_tag.trim().is_empty() {
            "自定义"
        } else {
            input.category_tag.trim()
        };
        let base_preset_id = if input.base_preset_id.trim().is_empty() {
            "quire-folio"
        } else {
            input.base_preset_id.trim()
        };
        let bundle_json = serde_json::to_value(&input.bundle)?;
        let row = sqlx::query(
            r#"INSERT INTO catalog.style_presets
               (style_id, name, description, category_tag, is_public,
                owner_user_id, owner_display_name, base_preset_id, bundle)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
               RETURNING style_id, name, description, category_tag, is_public,
                         owner_user_id, owner_display_name, base_preset_id, bundle,
                         created_at, updated_at"#,
        )
        .bind(&style_id)
        .bind(name)
        .bind(input.description.trim())
        .bind(category_tag)
        .bind(input.is_public)
        .bind(input.owner_user_id)
        .bind(input.owner_display_name.trim())
        .bind(base_preset_id)
        .bind(bundle_json)
        .fetch_one(&self.pool)
        .await?;
        row_to_style_preset(&row)
    }

    pub async fn update_style_preset(
        &self,
        style_id: &str,
        actor_user_id: &str,
        is_admin: bool,
        patch: UpdateStylePresetInput<'_>,
    ) -> RuntimeResult<StylePresetRecord> {
        if find_builtin_style_preset(style_id).is_some() {
            return Err(RuntimeError::Forbidden(
                "内置样式不可直接覆盖，请另存为新样式预设".into(),
            ));
        }
        let current = self
            .get_style_preset(style_id)
            .await?
            .ok_or_else(|| RuntimeError::InvalidRequest(format!("样式 `{style_id}` 不存在")))?;
        if !is_admin && current.owner_user_id.as_deref() != Some(actor_user_id) {
            return Err(RuntimeError::Forbidden(
                "仅样式创建者或管理员可修改该预设".into(),
            ));
        }

        let next_name = match patch.name {
            Some(raw) => {
                let trimmed = raw.trim();
                if trimmed.is_empty() || trimmed.chars().count() > 48 {
                    return Err(RuntimeError::InvalidRequest(
                        "样式名称不能为空且不超过 48 个字符".into(),
                    ));
                }
                trimmed.to_string()
            }
            None => current.name,
        };
        let next_desc = patch
            .description
            .map(|d| d.trim().to_string())
            .unwrap_or(current.description);
        let next_tag = patch
            .category_tag
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(ToString::to_string)
            .unwrap_or(current.category_tag);
        let next_public = patch.is_public.unwrap_or(current.is_public);
        let next_bundle = match patch.bundle {
            Some(bundle) => {
                bundle.validate()?;
                bundle
            }
            None => current.bundle,
        };
        let bundle_json = serde_json::to_value(&next_bundle)?;

        let row = sqlx::query(
            r#"UPDATE catalog.style_presets
               SET name = $2,
                   description = $3,
                   category_tag = $4,
                   is_public = $5,
                   bundle = $6,
                   updated_at = NOW()
               WHERE style_id = $1
               RETURNING style_id, name, description, category_tag, is_public,
                         owner_user_id, owner_display_name, base_preset_id, bundle,
                         created_at, updated_at"#,
        )
        .bind(style_id)
        .bind(&next_name)
        .bind(&next_desc)
        .bind(&next_tag)
        .bind(next_public)
        .bind(bundle_json)
        .fetch_one(&self.pool)
        .await?;
        row_to_style_preset(&row)
    }

    pub async fn delete_style_preset(
        &self,
        style_id: &str,
        actor_user_id: &str,
        is_admin: bool,
    ) -> RuntimeResult<()> {
        if find_builtin_style_preset(style_id).is_some() {
            return Err(RuntimeError::Forbidden("内置样式不可删除".into()));
        }
        let current = self
            .get_style_preset(style_id)
            .await?
            .ok_or_else(|| RuntimeError::InvalidRequest(format!("样式 `{style_id}` 不存在")))?;
        if !is_admin && current.owner_user_id.as_deref() != Some(actor_user_id) {
            return Err(RuntimeError::Forbidden(
                "仅样式创建者或管理员可删除该预设".into(),
            ));
        }
        sqlx::query("DELETE FROM catalog.style_presets WHERE style_id = $1")
            .bind(style_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

fn row_to_style_preset(row: &PgRow) -> RuntimeResult<StylePresetRecord> {
    let bundle_val: serde_json::Value = row.try_get("bundle")?;
    let bundle: StylePresetBundle = serde_json::from_value(bundle_val)?;
    Ok(StylePresetRecord {
        style_id: row.try_get("style_id")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        category_tag: row.try_get("category_tag")?,
        is_builtin: false,
        is_public: row.try_get("is_public")?,
        owner_user_id: row.try_get("owner_user_id")?,
        owner_display_name: row.try_get("owner_display_name")?,
        base_preset_id: row.try_get("base_preset_id")?,
        bundle,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[cfg(test)]
mod style_preset_tests {
    use super::*;

    #[test]
    fn all_ten_builtin_style_presets_are_valid() {
        let presets = builtin_style_presets();
        assert_eq!(presets.len(), 10);
        for preset in &presets {
            assert!(preset.is_builtin);
            assert!(preset.is_public);
            preset.bundle.validate().expect("builtin bundle is valid");
        }
    }
}
