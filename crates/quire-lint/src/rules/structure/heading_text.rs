use super::*;

const OPENING_SECTION_HEADINGS: &[&str] = &["导论", "引言", "前言", "introduction"];
const CLOSING_SECTION_HEADINGS: &[&str] = &["结语", "总结", "conclusion", "conclusions", "summary"];
const DENSE_PROSE_EXEMPT_SECTION_HEADINGS: &[&str] = &[
    "摘要",
    "总结与展望",
    "总结与建议",
    "结论与建议",
    "对策建议",
    "相关建议",
    "行动建议",
    "行动方案建议",
    "行动路径",
    "下一步工作",
    "存在问题与下一步工作",
    "保障措施",
    "落实要求",
    "工作举措",
    "具体举措",
];

const GENERIC_HEADINGS_ZH: &[&str] = &[
    "概述",
    "引言",
    "前言",
    "背景",
    "背景介绍",
    "综述",
    "总结",
    "小结",
    "结语",
    "总结与展望",
    "总结与建议",
    "结论与建议",
    "未来展望",
    "发展趋势",
    "核心结论",
    "主要发现",
    "数据分析",
    "现状分析",
    "问题分析",
    "对策建议",
    "相关建议",
    "行动建议",
    "行动方案建议",
    "附录说明",
];

const SCOPE_ONLY_HEADING_TOKENS: &[&str] = &[
    "时间窗口",
    "时间窗",
    "未纳入",
    "统计",
    "数据",
    "指标",
    "资料",
    "信息",
    "口径",
    "范围",
    "缺失",
    "说明",
    "释义",
    "备注",
    "与",
    "及",
    "和",
    "、",
    " ",
];

const GENERIC_HEADINGS_EN: &[&str] = &[
    "overview",
    "introduction",
    "background",
    "summary",
    "conclusion",
    "conclusions",
    "summary and outlook",
    "conclusion and recommendations",
    "future outlook",
    "key findings",
    "data analysis",
];

const COUNTING_NOUNS: &[&str] = &[
    "维度",
    "方面",
    "亮点",
    "趋势",
    "挑战",
    "机遇",
    "痛点",
    "优势",
    "劣势",
    "特征",
    "特点",
    "建议",
    "举措",
    "策略",
    "方向",
    "支柱",
    "驱动力",
    "要素",
    "判断",
    "结论",
    "发现",
    "主攻线",
    "主线",
    "路径",
    "对策",
    "问题",
    "短板",
    "瓶颈",
    "抓手",
    "重点",
    "任务",
    "措施",
    "原则",
    "机制",
    "行动",
    "工程",
];

fn is_scope_only_heading(normalized: &str) -> bool {
    if !["口径", "范围", "说明", "释义"]
        .iter()
        .any(|term| normalized.contains(term))
    {
        return false;
    }
    let remainder = SCOPE_ONLY_HEADING_TOKENS
        .iter()
        .fold(normalized.to_string(), |remaining, token| {
            remaining.replace(token, "")
        });
    remainder.trim().is_empty()
}

pub(super) fn check_boring_headings(document: &ReportDocument, engine: &mut Engine) {
    let first_h2 = document
        .blocks
        .iter()
        .position(|block| matches!(&block.body, BlockKind::Heading { level: 2, .. }));
    let last_h2 = document
        .blocks
        .iter()
        .rposition(|block| matches!(&block.body, BlockKind::Heading { level: 2, .. }));
    for (index, block) in document.blocks.iter().enumerate() {
        let BlockKind::Heading { level, text } = &block.body else {
            continue;
        };
        if *level < 2 {
            continue;
        }
        let normalized = normalize_heading_text(text);
        if normalized.is_empty() {
            continue;
        }
        let label = section_label(text);
        let lowered_label = label.to_ascii_lowercase();
        let positionally_exempt = *level == 2
            && ((Some(index) == first_h2
                && heading_list_contains(OPENING_SECTION_HEADINGS, label, &lowered_label))
                || (Some(index) == last_h2
                    && heading_list_contains(CLOSING_SECTION_HEADINGS, label, &lowered_label)));
        if positionally_exempt {
            continue;
        }
        let lowered = normalized.to_ascii_lowercase();
        let is_generic = GENERIC_HEADINGS_ZH.contains(&normalized)
            || GENERIC_HEADINGS_EN.contains(&lowered.as_str())
            || is_scope_only_heading(normalized);
        let is_counting = is_counting_heading(normalized);
        if !is_generic && !is_counting {
            continue;
        }
        engine.report(
            RuleId::BoringOrCountingHeading,
            format!(
                "heading `{}` uses generic or counting phrasing `{}` without stating a concrete conclusion or entity",
                block.id,
                text.trim()
            ),
            "Rewrite this Heading via apply_operations to name a specific subject, metric or takeaway. For a standard official-document section, keep its column label and add a concrete data-backed judgment after a colon (`<栏目或切面>：<基于本节证据得出的具体比较结论或结构性判断>`); fix this together with QR412/QR413 in one apply_operations batch when they hit the same block.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].text")),
        );
    }
}

pub(crate) fn dense_prose_section_exempt(text: &str) -> bool {
    let label = section_label(text);
    let lowered = label.to_ascii_lowercase();
    heading_list_contains(OPENING_SECTION_HEADINGS, label, &lowered)
        || action_or_closing_section_exempt(text)
}

pub(crate) fn action_or_closing_section_exempt(text: &str) -> bool {
    let label = section_label(text);
    let lowered = label.to_ascii_lowercase();
    heading_list_contains(CLOSING_SECTION_HEADINGS, label, &lowered)
        || heading_list_contains(DENSE_PROSE_EXEMPT_SECTION_HEADINGS, label, &lowered)
        || ["建议", "举措", "路径", "主攻线", "行动线"]
            .iter()
            .any(|marker| label.contains(marker))
}

fn section_label(text: &str) -> &str {
    let normalized = normalize_heading_text(text);
    if let Some((index, _)) = normalized
        .char_indices()
        .find(|(_, ch)| matches!(ch, ':' | '：'))
    {
        normalized.get(..index).unwrap_or(normalized).trim()
    } else {
        normalized
    }
}

fn heading_list_contains(headings: &[&str], label: &str, lowered: &str) -> bool {
    headings.contains(&label) || headings.contains(&lowered)
}

pub(super) fn normalize_heading_text(text: &str) -> &str {
    let stripped = strip_heading_number(text.trim());
    stripped
        .trim_matches(|ch: char| matches!(ch, ':' | '：' | '—' | '-' | '、' | ' ' | '\t'))
        .trim()
}

fn strip_heading_number(text: &str) -> &str {
    let text = text.trim_start();
    if let Some(rest) = strip_part_prefix(text) {
        return rest.trim_start();
    }
    if let Some(rest) = strip_chapter_prefix(text) {
        return rest.trim_start();
    }
    if let Some(rest) = strip_parenthesized_number(text) {
        return rest.trim_start();
    }
    if let Some(rest) = strip_dotted_number(text) {
        return rest.trim_start();
    }
    if let Some(rest) = strip_chinese_number(text) {
        return rest.trim_start();
    }
    text
}

fn strip_part_prefix(text: &str) -> Option<&str> {
    let head = text.get(..4)?;
    if !head.eq_ignore_ascii_case("part") {
        return None;
    }
    let rest = text.get(4..)?.trim_start();
    let digit_end = ascii_digit_prefix_len(rest);
    if digit_end == 0 {
        return None;
    }
    let after = rest.get(digit_end..)?.trim_start();
    let after = after
        .strip_prefix(':')
        .or_else(|| after.strip_prefix('：'))
        .or_else(|| after.strip_prefix('.'))
        .unwrap_or(after);
    Some(after)
}

fn strip_chapter_prefix(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('第')?;
    let numeral_end = chapter_numeral_prefix_len(rest);
    if numeral_end == 0 {
        return None;
    }
    let after = rest.get(numeral_end..)?;
    if let Some(rest) = after.strip_prefix("部分") {
        return Some(rest);
    }
    for marker in ['章', '节', '篇'] {
        if let Some(rest) = after.strip_prefix(marker) {
            return Some(rest);
        }
    }
    None
}

fn strip_parenthesized_number(text: &str) -> Option<&str> {
    let first = text.chars().next()?;
    let close = match first {
        '（' => '）',
        '(' => ')',
        _ => return None,
    };
    let rest = text.get(first.len_utf8()..)?;
    let close_index = rest.find(close)?;
    let inner = rest.get(..close_index)?;
    if inner.is_empty()
        || !inner
            .chars()
            .all(|ch| ch.is_ascii_digit() || is_chinese_numeral(ch))
    {
        return None;
    }
    rest.get(close_index + close.len_utf8()..)
}

fn strip_dotted_number(text: &str) -> Option<&str> {
    let mut end = 0usize;
    let mut has_dot = false;
    for (index, ch) in text.char_indices() {
        if ch.is_ascii_digit() {
            end = index + ch.len_utf8();
        } else if ch == '.' {
            has_dot = true;
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    if !has_dot || end == 0 {
        return None;
    }
    let suffix = text.get(end..)?;
    if suffix.is_empty() || suffix.starts_with('%') || suffix.starts_with('‰') {
        return None;
    }
    let last = text
        .get(..end)
        .and_then(|prefix| prefix.chars().next_back());
    if last != Some('.') {
        let next = suffix.chars().next()?;
        if !next.is_whitespace() && next.is_ascii() {
            return None;
        }
    }
    Some(suffix)
}

fn strip_chinese_number(text: &str) -> Option<&str> {
    let numeral_len = chapter_numeral_prefix_len(text);
    if numeral_len == 0 {
        return None;
    }
    text.get(numeral_len..)?.strip_prefix('、')
}

fn is_counting_heading(text: &str) -> bool {
    if !starts_with_counting_pattern(text) {
        return false;
    }
    !counting_heading_exempt(text)
}

fn starts_with_counting_pattern(text: &str) -> bool {
    let Some(rest) = strip_counting_numeral(text) else {
        return false;
    };
    let rest = rest.trim_start();
    let Some(quantifier) = rest.chars().next() else {
        return false;
    };
    if !matches!(quantifier, '大' | '个' | '项' | '类' | '重' | '条' | '点') {
        return false;
    }
    let mut rest = rest.get(quantifier.len_utf8()..).unwrap_or("").trim_start();
    for adjective in [
        "核心",
        "主要",
        "关键",
        "结构性",
        "系统性",
        "阶段性",
        "战略",
        "重要",
        "基本",
    ] {
        if let Some(after) = rest.strip_prefix(adjective) {
            rest = after.trim_start();
            break;
        }
    }
    COUNTING_NOUNS.iter().any(|noun| rest.starts_with(noun))
}

fn strip_counting_numeral(text: &str) -> Option<&str> {
    let first = text.chars().next()?;
    if first.is_ascii_digit() {
        let end = ascii_digit_prefix_len(text);
        return text.get(end..);
    }
    if matches!(
        first,
        '两' | '二' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十'
    ) {
        return text.get(first.len_utf8()..);
    }
    None
}

fn counting_heading_exempt(text: &str) -> bool {
    if let Some((colon_index, colon)) = text.char_indices().find(|(_, ch)| matches!(ch, ':' | '：'))
    {
        let after = text
            .get(colon_index + colon.len_utf8()..)
            .unwrap_or("")
            .trim();
        if after.chars().count() >= 6 {
            return true;
        }
    }
    text.chars().count() > 16 && has_concrete_heading_marker(text)
}

fn has_concrete_heading_marker(text: &str) -> bool {
    text.chars()
        .any(|ch| ch.is_ascii_digit() || matches!(ch, '%' | '％'))
        || text.contains('、')
        || text.contains('，')
        || text.contains(',')
        || text.contains('《')
        || text.contains('“')
        || text.contains('"')
}

#[cfg(test)]
mod tests {
    use super::strip_dotted_number;

    #[test]
    fn dotted_number_prefix_stops_before_percentages() {
        assert_eq!(strip_dotted_number("3.5% 增速"), None);
        assert_eq!(strip_dotted_number("3.5‰ 增速"), None);
        assert_eq!(strip_dotted_number("1.2 概述"), Some(" 概述"));
        assert_eq!(strip_dotted_number("1.2概述"), Some("概述"));
        assert_eq!(strip_dotted_number("1.2"), None);
    }
}
