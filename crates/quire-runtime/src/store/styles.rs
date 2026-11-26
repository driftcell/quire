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
    pub cover_variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toc_variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation_style: Option<CitationStyle>,
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

/// Built-in 5 styles spanning free exploration to formal decree.
pub fn builtin_style_presets() -> Vec<StylePresetRecord> {
    let ts = builtin_timestamp();
    vec![
        StylePresetRecord {
            style_id: "field-notes".into(),
            name: "灵思手札".into(),
            description: "现代无衬线与松石绿点缀，行距舒展，适合课题探索与调研随笔".into(),
            category_tag: "自由".into(),
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
                        ("--rpt-surface", "#F5F4EE"),
                        ("--rpt-paper", "#FAF9F5"),
                        ("--rpt-quote-bg", "#F0FDF4"),
                        ("--rpt-table-border", "#0F766E"),
                        ("--rpt-table-header-bg", "#F0FDFA"),
                        ("--rpt-table-stripe", "#F7F6F1"),
                        ("--rpt-line-height", "1.92"),
                        ("--rpt-body-size", "14.5px"),
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
                cover_variant: Some("minimal".into()),
                toc_variant: Some("standard".into()),
                citation_style: Some(CitationStyle::Numeric),
            },
            created_at: ts,
            updated_at: ts,
        },
        StylePresetRecord {
            style_id: "modern-brief".into(),
            name: "智库简报".into(),
            description: "高对比群青蓝与无衬线黑体，几何立线清晰，适合发展快报与数据解读".into(),
            category_tag: "轻快".into(),
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
                        ("--rpt-surface", "#F1F5F9"),
                        ("--rpt-paper", "#FFFFFF"),
                        ("--rpt-quote-bg", "#EFF6FF"),
                        ("--rpt-table-border", "#1E293B"),
                        ("--rpt-table-header-bg", "#F1F5F9"),
                        ("--rpt-table-stripe", "#F8FAFC"),
                        ("--rpt-line-height", "1.78"),
                        ("--rpt-body-size", "14.5px"),
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
                cover_variant: Some("modern".into()),
                toc_variant: Some("numbered".into()),
                citation_style: Some(CitationStyle::Numeric),
            },
            created_at: ts,
            updated_at: ts,
        },
        StylePresetRecord {
            style_id: "quire-folio".into(),
            name: "青塔研报".into(),
            description: "牛津蓝配人文衬线标题与现代无衬线正文，兼顾数字阅读与 A4 归档".into(),
            category_tag: "均衡".into(),
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
                cover_variant: Some("split".into()),
                toc_variant: Some("dotted".into()),
                citation_style: Some(CitationStyle::Numeric),
            },
            created_at: ts,
            updated_at: ts,
        },
        StylePresetRecord {
            style_id: "academic-press".into(),
            name: "学林刊印".into(),
            description: "全衬线宋体学术排版、纯粹细三线表与著者-出版年引文制，适配专刊与评审材料"
                .into(),
            category_tag: "严谨".into(),
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
                        ("--rpt-quote-bg", "#F9FAFB"),
                        ("--rpt-table-border", "#111111"),
                        ("--rpt-table-header-bg", "#FFFFFF"),
                        ("--rpt-table-stripe", "#FFFFFF"),
                        ("--rpt-line-height", "1.76"),
                        ("--rpt-body-size", "14px"),
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
                cover_variant: Some("classic".into()),
                toc_variant: Some("dotted".into()),
                citation_style: Some(CitationStyle::AuthorYear),
            },
            created_at: ts,
            updated_at: ts,
        },
        StylePresetRecord {
            style_id: "crimson-decree".into(),
            name: "丹宸公文".into(),
            description: "朱砂红文武双线、宋体庄重标题与标准公文页码，适配呈报校领导决策参阅件"
                .into(),
            category_tag: "正式".into(),
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
                        ("--rpt-line-height", "1.88"),
                        ("--rpt-body-size", "15px"),
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
                cover_variant: Some("classic".into()),
                toc_variant: Some("dotted".into()),
                citation_style: Some(CitationStyle::Numeric),
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
    fn all_five_builtin_style_presets_are_valid() {
        let presets = builtin_style_presets();
        assert_eq!(presets.len(), 5);
        for preset in &presets {
            assert!(preset.is_builtin);
            assert!(preset.is_public);
            preset.bundle.validate().expect("builtin bundle is valid");
        }
    }
}
