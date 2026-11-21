//! Focused higher-education report writing and formatting checks.

use std::collections::BTreeSet;

use quire_protocol::{
    BlockKind, CellValue, ReportDocument, chart_series, effective_chart_option, entries,
    validate_table,
};
use serde_json::Value;

use super::prose::mask_text;
use crate::{engine::Engine, registry::RuleId};

const ACADEMIC_JARGON_ZH: &[&str] = &[
    "研究设计",
    "样本高校",
    "样本量",
    "样本容量",
    "自变量",
    "因变量",
    "回归分析",
    "回归系数",
    "显著性检验",
    "显著性水平",
    "研究假设",
    "赫芬达尔",
    "基尼系数",
    "皮尔逊",
];
const ACADEMIC_JARGON_ASCII: &[&str] = &["Spearman", "Pearson", "HHI"];
const ORDINAL_EVALUATIONS: &[&str] = &[
    "占优",
    "领先",
    "落后",
    "居首",
    "较高",
    "较低",
    "不足",
    "为",
    "更强",
    "更好",
    "较弱",
    "偏低",
    "偏高",
    "表现突出",
    "存在短板",
    "突出",
    "短板",
    "反超",
    "高于",
    "低于",
];
const POLICY_TERMS_REQUIRING_QUOTES: &[&str] = &["双一流", "双优", "十四五", "十五五"];

pub(crate) fn check(document: &ReportDocument, engine: &mut Engine<'_>) {
    check_blind_ordinals_and_academic_jargon(document, engine);
    check_visual_unit_notes(document, engine);
    check_policy_and_document_format(document, engine);
}

fn check_blind_ordinals_and_academic_jargon(document: &ReportDocument, engine: &mut Engine<'_>) {
    for (index, block) in document.blocks.iter().enumerate() {
        let BlockKind::Paragraph { text } = &block.body else {
            continue;
        };
        let mut hits = BTreeSet::new();
        if let Some(reference) = blind_ordinal_reference(text) {
            hits.insert(format!("盲指序号 `{reference}`"));
        }
        let masked = mask_text(text);
        for phrase in ACADEMIC_JARGON_ZH {
            if masked.contains(phrase) {
                hits.insert(format!("论文腔术语 `{phrase}`"));
            }
        }
        for phrase in ACADEMIC_JARGON_ASCII {
            if contains_ascii_term(&masked, phrase) {
                hits.insert(format!("论文腔术语 `{phrase}`"));
            }
        }
        if has_p_value_statement(&masked) {
            hits.insert("论文腔术语 `p<0.05`".to_string());
        }
        if hits.is_empty() {
            continue;
        }
        engine.report(
            RuleId::BlindOrdinalOrAcademicJargon,
            format!(
                "block `{}` contains blind ordinal references or academic jargon: {}",
                block.id,
                hits.into_iter().collect::<Vec<_>>().join("、")
            ),
            "Replace blind ordinals with the exact metric/entity name, and translate statistical-method terminology into a concrete management finding; do not make readers count table columns.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].text")),
        );
    }
}

fn blind_ordinal_reference(text: &str) -> Option<String> {
    for (start, _) in text.match_indices('第') {
        if inside_quoted_or_code_span(text, start) {
            continue;
        }
        let after_marker = text.get(start + '第'.len_utf8()..)?;
        let numeral_len = ordinal_numeral_len(after_marker);
        if numeral_len == 0 {
            continue;
        }
        let after_numeral = after_marker.get(numeral_len..)?;
        let Some(suffix) = ["项指标", "项", "列", "个", "指标"]
            .into_iter()
            .find(|suffix| after_numeral.starts_with(suffix))
        else {
            continue;
        };
        let clause = after_numeral
            .get(suffix.len()..)?
            .split(['，', '。', '；', '！', '？', '\n'])
            .next()
            .unwrap_or_default();
        let Some((evaluation_index, evaluation)) = earliest_ordinal_evaluation(clause) else {
            continue;
        };
        if has_explicit_metric_label(clause, evaluation_index, evaluation) {
            continue;
        }
        let end = start + '第'.len_utf8() + numeral_len + suffix.len();
        if let Some(reference) = text.get(start..end) {
            return Some(reference.to_string());
        }
    }
    None
}

fn ordinal_numeral_len(text: &str) -> usize {
    let mut end = 0usize;
    for (index, ch) in text.char_indices() {
        if ch.is_ascii_digit()
            || matches!(
                ch,
                '零' | '一' | '二' | '两' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十'
            )
        {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    end
}

fn earliest_ordinal_evaluation(clause: &str) -> Option<(usize, &'static str)> {
    ORDINAL_EVALUATIONS
        .iter()
        .filter_map(|evaluation| clause.find(evaluation).map(|index| (index, *evaluation)))
        .min_by_key(|(index, _)| *index)
}

fn has_explicit_metric_label(clause: &str, evaluation_index: usize, evaluation: &str) -> bool {
    let prefix = clause.get(..evaluation_index).unwrap_or_default().trim();
    if prefix.is_empty() {
        if evaluation != "为" {
            return false;
        }
        let after = clause.get('为'.len_utf8()..).unwrap_or_default().trim();
        return !after.is_empty()
            && !after.chars().any(|ch| ch.is_ascii_digit())
            && !["最高", "最低", "最优", "最差", "领先", "落后"]
                .iter()
                .any(|word| after.starts_with(word));
    }
    if ["与", "和", "及", "、"]
        .iter()
        .any(|marker| prefix.starts_with(marker))
    {
        return false;
    }
    let prefix = prefix.strip_prefix("指标").unwrap_or(prefix);
    let prefix = prefix.trim_end_matches(|ch: char| {
        ch.is_whitespace() || matches!(ch, '均' | '都' | '更' | '最' | '较')
    });
    !prefix.is_empty() && !matches!(prefix, "方面" | "项目" | "列")
}

fn inside_quoted_or_code_span(text: &str, position: usize) -> bool {
    let prefix = text.get(..position).unwrap_or_default();
    prefix.matches('`').count() % 2 == 1
        || prefix.matches('“').count() > prefix.matches('”').count()
        || prefix.matches('「').count() > prefix.matches('」').count()
        || prefix.matches('《').count() > prefix.matches('》').count()
        || prefix.matches('"').count() % 2 == 1
}

fn contains_ascii_term(text: &str, term: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    let needle = term.to_ascii_lowercase();
    let mut start = 0usize;
    while let Some(found) = lowered.get(start..).and_then(|rest| rest.find(&needle)) {
        let absolute = start + found;
        let before = lowered.get(..absolute).and_then(|s| s.chars().next_back());
        let after = lowered
            .get(absolute + needle.len()..)
            .and_then(|s| s.chars().next());
        if before.is_none_or(|ch| !ch.is_ascii_alphabetic())
            && after.is_none_or(|ch| !ch.is_ascii_alphabetic())
        {
            return true;
        }
        start = absolute + needle.len();
    }
    false
}

fn has_p_value_statement(text: &str) -> bool {
    let compact: String = text
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    compact.contains("p<0.0") || compact.contains("p＜0.0")
}

fn check_visual_unit_notes(document: &ReportDocument, engine: &mut Engine<'_>) {
    for (index, block) in document.blocks.iter().enumerate() {
        match &block.body {
            BlockKind::Table(table) => {
                if table
                    .note
                    .as_deref()
                    .is_some_and(|note| !note.trim().is_empty())
                {
                    continue;
                }
                let Ok(shape) = validate_table(table) else {
                    continue;
                };
                let missing: Vec<String> = shape
                    .leaves
                    .iter()
                    .filter(|leaf| {
                        table
                            .rows
                            .iter()
                            .chain(table.summary_rows.iter())
                            .filter_map(|row| row.get(&leaf.key))
                            .any(cell_is_numeric)
                    })
                    .filter(|leaf| !label_has_unit(&leaf.flattened_label()))
                    .map(|leaf| leaf.flattened_label())
                    .collect();
                if !missing.is_empty() {
                    report_visual_note(
                        engine,
                        block,
                        index,
                        format!("numeric columns lack units: {}", missing.join(", ")),
                    );
                }
            }
            BlockKind::Echarts(chart) => {
                if chart
                    .note
                    .as_deref()
                    .is_some_and(|note| !note.trim().is_empty())
                {
                    continue;
                }
                let option = effective_chart_option(chart);
                if chart_has_numeric_data(&option) && !chart_units_complete(&option) {
                    report_visual_note(
                        engine,
                        block,
                        index,
                        "numeric series lack complete value-axis/series units".to_string(),
                    );
                }
            }
            _ => {}
        }
    }
}

fn cell_is_numeric(cell: &CellValue) -> bool {
    if cell.as_number().is_some() {
        return true;
    }
    let text = cell.display_text();
    let normalized: String = text
        .trim()
        .trim_end_matches(['%', '％'])
        .chars()
        .filter(|ch| !matches!(ch, ',' | '，'))
        .collect();
    !normalized.is_empty() && normalized.parse::<f64>().is_ok()
}

fn chart_has_numeric_data(option: &Value) -> bool {
    chart_series(option)
        .iter()
        .any(|series| series.get("data").is_some_and(json_has_number))
        || entries(option.get("dataset"))
            .iter()
            .any(|dataset| dataset.get("source").is_some_and(json_has_number))
}

fn json_has_number(value: &Value) -> bool {
    match value {
        Value::Number(_) => true,
        Value::Array(items) => items.iter().any(json_has_number),
        Value::Object(object) => object.values().any(json_has_number),
        _ => false,
    }
}

fn chart_units_complete(option: &Value) -> bool {
    let mut value_axes = Vec::new();
    for key in ["xAxis", "yAxis"] {
        value_axes.extend(
            entries(option.get(key))
                .into_iter()
                .filter(|axis| axis.get("type").and_then(Value::as_str) == Some("value")),
        );
    }
    if !value_axes.is_empty() {
        return value_axes.iter().all(|axis| {
            axis.get("name")
                .and_then(Value::as_str)
                .is_some_and(label_has_unit)
        });
    }

    let numeric_series: Vec<_> = chart_series(option)
        .into_iter()
        .filter(|series| series.get("data").is_some_and(json_has_number))
        .collect();
    !numeric_series.is_empty()
        && numeric_series.iter().all(|series| {
            series
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(label_has_unit)
        })
}

fn label_has_unit(label: &str) -> bool {
    let Some((open_index, open)) = label
        .char_indices()
        .find(|(_, ch)| matches!(ch, '(' | '（'))
    else {
        return false;
    };
    let close = if open == '(' { ')' } else { '）' };
    let Some(rest) = label.get(open_index + open.len_utf8()..) else {
        return false;
    };
    let Some(close_index) = rest.find(close) else {
        return false;
    };
    let content = rest.get(..close_index).unwrap_or_default();
    [
        "项", "篇", "所", "名", "人", "元", "万", "亿", "%", "％", "‰", "倍", "分", "位", "个",
        "家", "册", "次", "年", "items", "people", "yuan", "million", "billion",
    ]
    .iter()
    .any(|unit| content.to_ascii_lowercase().contains(unit))
}

fn report_visual_note(
    engine: &mut Engine<'_>,
    block: &quire_protocol::Block,
    index: usize,
    detail: String,
) {
    engine.report(
        RuleId::VisualMissingUnitOrScopeNote,
        format!(
            "{} `{}` has numeric data but no usable note and incomplete unit labels ({detail})",
            block.block_type(),
            block.id
        ),
        "Add `note` with `单位：…；统计口径/时间范围：…`, or label every numeric table column/value axis with units such as （项）, （万元） or （%）.",
        Some(block.id.clone()),
        Some(format!("blocks[{index}].note")),
    );
}

fn check_policy_and_document_format(document: &ReportDocument, engine: &mut Engine<'_>) {
    for (index, block) in document.blocks.iter().enumerate() {
        let mut fields: Vec<(&str, String)> = Vec::new();
        match &block.body {
            BlockKind::Heading { text, .. } | BlockKind::Paragraph { text } => {
                fields.push((text, format!("blocks[{index}].text")));
            }
            BlockKind::Table(table) => {
                for (text, path) in [
                    (table.title.as_deref(), "title"),
                    (table.note.as_deref(), "note"),
                    (block.source_note.as_deref(), "source_note"),
                ] {
                    if let Some(text) = text {
                        fields.push((text, format!("blocks[{index}].{path}")));
                    }
                }
            }
            BlockKind::Echarts(chart) => {
                for (text, path) in [
                    (chart.title.as_deref(), "title"),
                    (chart.note.as_deref(), "note"),
                    (block.source_note.as_deref(), "source_note"),
                ] {
                    if let Some(text) = text {
                        fields.push((text, format!("blocks[{index}].{path}")));
                    }
                }
            }
            _ => {}
        }
        for (text, path) in fields {
            let mut issues = BTreeSet::new();
            for term in POLICY_TERMS_REQUIRING_QUOTES {
                if text.match_indices(term).any(|(position, _)| {
                    !inside_chinese_quotes(text, position) && !inside_inline_code(text, position)
                }) {
                    issues.insert(format!("固定提法应写为“{term}”"));
                }
            }
            if let Some(range) = nonstandard_year_range(text) {
                issues.insert(format!("年份区间 `{range}` 应使用一字线 `—`"));
            }
            if let Some(document_number) = nonstandard_document_number(text) {
                issues.insert(format!(
                    "文号 `{document_number}` 的年份应使用六角括号 `〔〕`"
                ));
            }
            if issues.is_empty() {
                continue;
            }
            engine.report(
                RuleId::HigherEdPolicyTermFormat,
                format!(
                    "block `{}` has higher-education terminology/format issues: {}",
                    block.id,
                    issues.into_iter().collect::<Vec<_>>().join("；")
                ),
                "Use Chinese quotation marks for fixed policy terms, `—` for year ranges, and `〔20XX〕X号` for official document numbers; preserve verified source wording in Quote blocks.",
                Some(block.id.clone()),
                Some(path),
            );
        }
    }
}

fn inside_chinese_quotes(text: &str, position: usize) -> bool {
    text.get(..position)
        .is_some_and(|prefix| prefix.matches('“').count() > prefix.matches('”').count())
}

fn inside_inline_code(text: &str, position: usize) -> bool {
    text.get(..position)
        .is_some_and(|prefix| prefix.matches('`').count() % 2 == 1)
}

fn nonstandard_year_range(text: &str) -> Option<String> {
    for (start, _) in text.match_indices(|ch: char| ch.is_ascii_digit()) {
        let Some(rest) = text.get(start..) else {
            continue;
        };
        let first_len = ascii_digits_len(rest);
        if first_len != 4 || !rest.starts_with("20") {
            continue;
        }
        let Some(after_first) = rest.get(first_len..) else {
            continue;
        };
        let Some(separator) = after_first.chars().next() else {
            continue;
        };
        if !matches!(separator, '-' | '~' | '～' | '–') {
            continue;
        }
        let after_separator = after_first.get(separator.len_utf8()..).unwrap_or_default();
        if ascii_digits_len(after_separator) != 4 || !after_separator.starts_with("20") {
            continue;
        }
        let end = start + first_len + separator.len_utf8() + 4;
        if let Some(range) = text.get(start..end) {
            return Some(range.to_string());
        }
    }
    None
}

fn nonstandard_document_number(text: &str) -> Option<String> {
    for (open_index, open) in text.match_indices(['(', '（']) {
        let close = if open == "(" { ')' } else { '）' };
        let content_start = open_index + open.len();
        let rest = text.get(content_start..)?;
        let Some(close_index) = rest.find(close) else {
            continue;
        };
        let year = rest.get(..close_index).unwrap_or_default();
        if year.len() != 4
            || !year.starts_with("20")
            || !year.bytes().all(|byte| byte.is_ascii_digit())
        {
            continue;
        }
        let after_close = rest
            .get(close_index + close.len_utf8()..)
            .unwrap_or_default();
        let number_len = ascii_digits_len(after_close);
        if number_len == 0
            || !after_close
                .get(number_len..)
                .is_some_and(|suffix| suffix.starts_with('号'))
        {
            continue;
        }
        let prefix = text.get(..open_index).unwrap_or_default();
        let cjk_prefix_len = prefix
            .chars()
            .rev()
            .take_while(|ch| ('\u{4e00}'..='\u{9fff}').contains(ch))
            .count();
        if cjk_prefix_len < 2 {
            continue;
        }
        let end = content_start + close_index + close.len_utf8() + number_len + '号'.len_utf8();
        if let Some(number) = text.get(open_index..end) {
            return Some(number.to_string());
        }
    }
    None
}

fn ascii_digits_len(text: &str) -> usize {
    let mut end = 0usize;
    for (index, ch) in text.char_indices() {
        if ch.is_ascii_digit() {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    end
}
