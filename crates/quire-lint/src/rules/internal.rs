//! Publication-surface checks for internal API, database and renderer identifiers.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::sync::LazyLock;

use quire_protocol::BlockKind;
use regex::Regex;

use crate::{
    context::LintContext,
    engine::{Engine, Finding},
    registry::RuleId,
};

static ASSIGNMENT_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(?-u:\b)[a-zA-Z_][a-zA-Z0-9_]{1,30}\s*=\s*[^\s，。；、）)]+"));
static YEAR_KEY_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(?-u:\b)20\d{2}\s*=\s*\d+"));
static LOCAL_METRIC_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"(?-u:\b)local:\d+(?:\.\d+)+"));
static PAREN_MODULE_RE: LazyLock<Result<Regex, regex::Error>> = LazyLock::new(|| {
    Regex::new(r"(?i)[（(]\s*(?:reporting|subject|subjectcloud|panorama_report|shuangyou)\s*[）)]")
});

const INTERNAL_NAMES: &[&str] = &[
    "FacultyInfo",
    "NOT_COLLECTED",
    "indicator_id",
    "block_id",
    "source_id",
    "record_id",
    "target_id",
    "target_level",
    "includeSub",
    "学科云index系统",
    "指标库口径",
    "指标库记录",
    "左panel",
    "右panel",
    "panorama_report",
    "subjectcloud",
];

const SUGGESTION: &str = "将内部参数、数据库主键、MCP 模块名或结构体名转译为正式出版级业务中文：删去 `(record_id=42)`、`(includeSub=true)`、`(local:4.1.1)`、`（reporting）`、`（subject）`、`panorama_report`、`subjectcloud` 等实现细节；内部库请使用“青塔高等教育全景数据库”“青塔学科发展监测平台”等正式名称，`FacultyInfo` 写为“高层次人才称号记录”，`指标库口径/指标库记录` 写为“全省统一统计口径”或“数据平台统计记录”，`左panel/右panel` 写为“左图/右图”，无数据单元格填 `—` 或“暂未收录”。";

struct VisibleField<'a> {
    text: Cow<'a, str>,
    path: String,
    table_cell: bool,
}

pub(crate) fn check(context: &LintContext<'_>) -> Vec<Finding> {
    let mut engine = Engine::new();
    for (index, block) in context.document.blocks.iter().enumerate() {
        let mut fields = Vec::new();
        match &block.body {
            BlockKind::Heading { text, .. }
            | BlockKind::Paragraph { text }
            | BlockKind::Quote { text } => {
                push(&mut fields, text, format!("blocks[{index}].text"), false);
            }
            BlockKind::Table(table) => {
                push_option(
                    &mut fields,
                    table.title.as_deref(),
                    format!("blocks[{index}].title"),
                );
                push_option(
                    &mut fields,
                    table.note.as_deref(),
                    format!("blocks[{index}].note"),
                );
                push_option(
                    &mut fields,
                    block.source_note.as_deref(),
                    format!("blocks[{index}].source_note"),
                );
                for (row_index, row) in table.rows.iter().enumerate() {
                    for (key, cell) in row {
                        let text = cell.display_text();
                        fields.push(VisibleField {
                            text: Cow::Owned(text),
                            path: format!("blocks[{index}].rows[{row_index}].{key}"),
                            table_cell: true,
                        });
                    }
                }
                for (row_index, row) in table.summary_rows.iter().enumerate() {
                    for (key, cell) in row {
                        let text = cell.display_text();
                        fields.push(VisibleField {
                            text: Cow::Owned(text),
                            path: format!("blocks[{index}].summary_rows[{row_index}].{key}"),
                            table_cell: true,
                        });
                    }
                }
            }
            BlockKind::Echarts(chart) => {
                push_option(
                    &mut fields,
                    chart.title.as_deref(),
                    format!("blocks[{index}].title"),
                );
                push_option(
                    &mut fields,
                    chart.note.as_deref(),
                    format!("blocks[{index}].note"),
                );
                push_option(
                    &mut fields,
                    block.source_note.as_deref(),
                    format!("blocks[{index}].source_note"),
                );
                push(
                    &mut fields,
                    &chart.fallback_summary,
                    format!("blocks[{index}].fallback_summary"),
                    false,
                );
            }
            BlockKind::CustomHtml(custom) => {
                push_option(
                    &mut fields,
                    custom.title.as_deref(),
                    format!("blocks[{index}].title"),
                );
                push_option(
                    &mut fields,
                    custom.note.as_deref(),
                    format!("blocks[{index}].note"),
                );
                push_option(
                    &mut fields,
                    block.source_note.as_deref(),
                    format!("blocks[{index}].source_note"),
                );
                push(
                    &mut fields,
                    &custom.fallback_text,
                    format!("blocks[{index}].fallback_text"),
                    false,
                );
            }
        }

        for field in fields {
            let leaks = internal_leaks(&field.text, field.table_cell);
            if leaks.is_empty() {
                continue;
            }
            engine.report(
                RuleId::InternalIdentifierLeak,
                format!(
                    "block `{}` exposes internal implementation identifiers: {}",
                    block.id,
                    leaks.into_iter().collect::<Vec<_>>().join("、")
                ),
                SUGGESTION,
                Some(block.id.clone()),
                Some(field.path),
            );
        }
    }
    for (source_id, entry) in &context.document.sources {
        let mut source_fields = Vec::new();
        push(
            &mut source_fields,
            &entry.title,
            format!("sources.{source_id}.title"),
            false,
        );
        push_option(
            &mut source_fields,
            entry.publisher.as_deref(),
            format!("sources.{source_id}.publisher"),
        );
        push_option(
            &mut source_fields,
            entry.note.as_deref(),
            format!("sources.{source_id}.note"),
        );
        for field in source_fields {
            let leaks = internal_leaks(&field.text, false);
            if leaks.is_empty() {
                continue;
            }
            engine.report(
                RuleId::InternalIdentifierLeak,
                format!(
                    "source `{}` exposes internal implementation identifiers: {}",
                    source_id,
                    leaks.into_iter().collect::<Vec<_>>().join("、")
                ),
                SUGGESTION,
                None,
                Some(field.path),
            );
        }
    }
    engine.into_findings()
}

fn push<'a>(fields: &mut Vec<VisibleField<'a>>, text: &'a str, path: String, table_cell: bool) {
    fields.push(VisibleField {
        text: Cow::Borrowed(text),
        path,
        table_cell,
    });
}

fn push_option<'a>(fields: &mut Vec<VisibleField<'a>>, text: Option<&'a str>, path: String) {
    if let Some(text) = text {
        push(fields, text, path, false);
    }
}

fn internal_leaks(text: &str, table_cell: bool) -> BTreeSet<String> {
    let mut hits = BTreeSet::new();
    for (name, pattern) in [
        ("assignment", &*ASSIGNMENT_RE),
        ("year key", &*YEAR_KEY_RE),
        ("local metric", &*LOCAL_METRIC_RE),
        ("parenthetical module", &*PAREN_MODULE_RE),
    ] {
        match pattern {
            Ok(pattern) => {
                for matched in pattern.find_iter(text) {
                    hits.insert(matched.as_str().to_string());
                }
            }
            Err(error) => {
                tracing::warn!(%error, %name, "internal identifier regex is invalid; skipping this check");
            }
        }
    }
    for name in INTERNAL_NAMES {
        if contains_identifier(text, name) {
            hits.insert((*name).to_string());
        }
    }
    if text.contains("LATEST") && text.chars().any(is_cjk) {
        hits.insert("LATEST".to_string());
    }
    if table_cell {
        for phrase in ["学科云无数据", "双优库无数据"] {
            if text.contains(phrase) {
                hits.insert(phrase.to_string());
            }
        }
    }
    hits
}

fn contains_identifier(text: &str, needle: &str) -> bool {
    text.match_indices(needle).any(|(position, _)| {
        let before = text
            .get(..position)
            .and_then(|prefix| prefix.chars().next_back());
        let after = text
            .get(position + needle.len()..)
            .and_then(|suffix| suffix.chars().next());
        before.is_none_or(|ch| !is_ascii_identifier_char(ch))
            && after.is_none_or(|ch| !is_ascii_identifier_char(ch))
    })
}

fn is_ascii_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn is_cjk(ch: char) -> bool {
    matches!(ch, '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignment_patterns_cover_api_and_year_keys() {
        assert!(ASSIGNMENT_RE.is_ok());
        assert!(YEAR_KEY_RE.is_ok());
        assert!(LOCAL_METRIC_RE.is_ok());
        for text in [
            "record_id=42",
            "target_id=0",
            "target_level = 0",
            "includeSub=true",
            "province=广东，detail=matrix",
            "rankable=false",
            "2021=4、2026=43",
            "local:4.1.1",
            "省部级科研平台（指标库口径）",
            "指标库记录显示为12个",
            "青塔数据库（reporting）",
            "青塔学科云（subject）",
            "panorama_report 统计数据",
            "subjectcloud 数据",
        ] {
            assert!(!internal_leaks(text, false).is_empty(), "{text}");
        }
    }

    #[test]
    fn publication_chinese_does_not_trigger() {
        for text in [
            "统计口径含参与单位与附属机构",
            "高层次人才称号记录",
            "全省统一统计口径",
            "数据平台统计记录",
            "左图与右图",
            "暂未收录",
            "2021—2026年",
            "最新期末快照",
        ] {
            assert!(internal_leaks(text, false).is_empty(), "{text}");
        }
        assert!(internal_leaks("暂未收录", true).is_empty());
    }

    #[test]
    fn sources_with_internal_module_tags_trigger_qr424() {
        let mut doc =
            quire_protocol::ReportDocument::new(quire_protocol::DocId::parse("doc_1").unwrap());
        doc.sources.insert(
            quire_protocol::SourceId::parse("qt_rep_25").unwrap(),
            quire_protocol::SourceEntry::new("青塔数据库（reporting）"),
        );
        doc.sources.insert(
            quire_protocol::SourceId::parse("qt_ok_25").unwrap(),
            quire_protocol::SourceEntry::new("青塔高等教育全景数据库 — 国家自然科学基金立项统计"),
        );
        let report = crate::lint_document(&doc);
        let qr424: Vec<_> = report
            .diagnostics
            .iter()
            .filter(|d| d.code == "QR424")
            .collect();
        assert_eq!(qr424.len(), 1);
        assert!(qr424[0].message.contains("qt_rep_25"));
        assert_eq!(qr424[0].path.as_deref(), Some("sources.qt_rep_25.title"));
    }
}
