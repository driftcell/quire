//! Structure and text-quality rules: placeholders, heading flow, visual captions and references.

use std::collections::{BTreeMap, BTreeSet};

use quire_protocol::{BlockKind, NumberingKind, ReportDocument};

use crate::{engine::Engine, registry::RuleId};

pub(crate) fn check(document: &ReportDocument, engine: &mut Engine<'_>) {
    if document.blocks.is_empty() {
        engine.report(
            RuleId::EmptyDocument,
            "document has no blocks",
            "use apply_operations with Append or InsertAfter to add content",
            None,
            Some("blocks".into()),
        );
    }
    check_headings(document, engine);
    check_empty_text(document, engine);
    check_placeholders(document, engine);
    check_visual_sequence(document, engine);
    check_visual_references(document, engine);
    check_inline_syntax(document, engine);
    check_consecutive_visuals(document, engine);
    check_boring_headings(document, engine);
}

fn check_headings(document: &ReportDocument, engine: &mut Engine<'_>) {
    let mut previous_heading_level: Option<u8> = None;
    for (index, block) in document.blocks.iter().enumerate() {
        let BlockKind::Heading { level, text } = &block.body else {
            continue;
        };
        if text.trim().is_empty() {
            engine.report(
                RuleId::EmptyHeading,
                format!("heading `{}` has no text", block.id),
                "add section text or remove the heading",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].text")),
            );
        }
        if (1..=6).contains(level) {
            if let Some(previous) = previous_heading_level
                && *level > previous + 1
            {
                engine.report(
                    RuleId::HeadingLevelJump,
                    format!("heading `{}` jumps from h{previous} to h{level}", block.id),
                    "insert an intermediate heading or reduce the level by one",
                    Some(block.id.clone()),
                    Some(format!("blocks[{index}].level")),
                );
            }
            previous_heading_level = Some(*level);
            if let Some(depth) = manual_heading_depth(text)
                && depth != usize::from(*level)
            {
                engine.report(
                    RuleId::HeadingNumberingMismatch,
                    format!(
                        "heading `{}` uses level-{depth} numbering `{}` at h{level}",
                        block.id,
                        text.trim()
                    ),
                    "match the manual number depth to the heading level, or remove the manual number and let the TOC number it.",
                    Some(block.id.clone()),
                    Some(format!("blocks[{index}].text")),
                );
            }
        }
        let next_is_boundary = document.blocks.get(index + 1).is_none_or(|next| {
            matches!(
                &next.body,
                BlockKind::Heading { level: next_level, .. } if *next_level <= *level
            )
        });
        if next_is_boundary && !text.trim().is_empty() {
            engine.report(
                RuleId::HeadingEmptySection,
                format!("heading `{}` has no content before the next section", block.id),
                "add at least one paragraph, table, chart or sub-heading, or remove the empty section.",
                Some(block.id.clone()),
                Some(format!("blocks[{index}]")),
            );
        }
    }
}

fn check_empty_text(document: &ReportDocument, engine: &mut Engine<'_>) {
    for (index, block) in document.blocks.iter().enumerate() {
        let text = match &block.body {
            BlockKind::Paragraph { text } | BlockKind::Quote { text } => text,
            _ => continue,
        };
        if text.trim().is_empty() {
            engine.report(
                RuleId::EmptyTextBlock,
                format!("{} `{}` has no text", block.block_type(), block.id),
                "supply text or delete the block",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].text")),
            );
        }
    }
}

fn check_placeholders(document: &ReportDocument, engine: &mut Engine<'_>) {
    for (index, block) in document.blocks.iter().enumerate() {
        let mut found: Option<(&'static str, String)> = None;
        let mut scan = |text: &str, path: String| {
            if found.is_none()
                && let Some(token) = first_placeholder(text)
            {
                found = Some((token, path));
            }
        };
        match &block.body {
            BlockKind::Heading { text, .. } => scan(text, format!("blocks[{index}].text")),
            BlockKind::Paragraph { text } | BlockKind::Quote { text } => {
                scan(text, format!("blocks[{index}].text"));
            }
            BlockKind::Table(table) => {
                if let Some(title) = table.title.as_deref() {
                    scan(title, format!("blocks[{index}].title"));
                }
                if let Some(note) = table.note.as_deref() {
                    scan(note, format!("blocks[{index}].note"));
                }
                for row in table.rows.iter().chain(table.summary_rows.iter()) {
                    for (key, cell) in row {
                        scan(&cell.display_text(), format!("blocks[{index}].rows.{key}"));
                    }
                }
            }
            BlockKind::Echarts(chart) => {
                if let Some(title) = chart.title.as_deref() {
                    scan(title, format!("blocks[{index}].title"));
                }
                if let Some(note) = chart.note.as_deref() {
                    scan(note, format!("blocks[{index}].note"));
                }
                scan(
                    &chart.fallback_summary,
                    format!("blocks[{index}].fallback_summary"),
                );
            }
            BlockKind::CustomHtml(html) => {
                if let Some(title) = html.title.as_deref() {
                    scan(title, format!("blocks[{index}].title"));
                }
                if let Some(note) = html.note.as_deref() {
                    scan(note, format!("blocks[{index}].note"));
                }
                scan(
                    &html.fallback_text,
                    format!("blocks[{index}].fallback_text"),
                );
            }
        }
        if let Some((token, path)) = found {
            engine.report(
                RuleId::PlaceholderTextLeak,
                format!("block `{}` contains unfinished placeholder `{token}`", block.id),
                "replace the placeholder with verified content or remove the sentence before publishing; placeholders must never reach readers.",
                Some(block.id.clone()),
                Some(path),
            );
        }
    }
}

fn check_visual_sequence(document: &ReportDocument, engine: &mut Engine<'_>) {
    let mut expected: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut seen: BTreeMap<&'static str, BTreeSet<usize>> = BTreeMap::new();
    for (index, block) in document.blocks.iter().enumerate() {
        let BlockKind::Table(table) = &block.body else {
            let BlockKind::Echarts(chart) = &block.body else {
                if let BlockKind::CustomHtml(html) = &block.body {
                    check_visual_title(engine, block, html.title.as_deref(), index);
                    check_manual_caption(
                        engine,
                        block,
                        html.title.as_deref(),
                        index,
                        &mut expected,
                        &mut seen,
                    );
                }
                continue;
            };
            check_visual_title(engine, block, chart.title.as_deref(), index);
            check_manual_caption(
                engine,
                block,
                chart.title.as_deref(),
                index,
                &mut expected,
                &mut seen,
            );
            continue;
        };
        check_visual_title(engine, block, table.title.as_deref(), index);
        check_manual_caption(
            engine,
            block,
            table.title.as_deref(),
            index,
            &mut expected,
            &mut seen,
        );
    }
}

fn check_visual_title(
    engine: &mut Engine<'_>,
    block: &quire_protocol::Block,
    title: Option<&str>,
    index: usize,
) {
    let Some(title) = title.map(str::trim).filter(|title| !title.is_empty()) else {
        return;
    };
    if !has_manual_number_prefix(title) {
        return;
    }
    engine.report(
        RuleId::ManualTitleNumbering,
        format!(
            "block `{}` title starts with hardcoded numbering: `{title}`",
            block.id
        ),
        "Put the bare title in outer `title` ONLY; numbering is generated automatically (表 N / 图 N).",
        Some(block.id.clone()),
        Some(format!("blocks[{index}].title")),
    );
}

fn check_manual_caption(
    engine: &mut Engine<'_>,
    block: &quire_protocol::Block,
    title: Option<&str>,
    index: usize,
    expected: &mut BTreeMap<&'static str, usize>,
    seen: &mut BTreeMap<&'static str, BTreeSet<usize>>,
) {
    let Some(title) = title else {
        return;
    };
    let Some((kind, number)) = manual_caption(title) else {
        return;
    };
    let next = expected.entry(kind).or_insert(1);
    let duplicates = seen.entry(kind).or_default();
    if !duplicates.insert(number) {
        engine.report(
            RuleId::VisualCaptionSequence,
            format!("block `{}` repeats {kind} {number}", block.id),
            "remove the hardcoded label and let automatic numbering produce a unique sequence.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].title")),
        );
        return;
    }
    if number != *next {
        engine.report(
            RuleId::VisualCaptionSequence,
            format!(
                "block `{}` uses {kind} {number} where the next label is {kind} {next}",
                block.id
            ),
            "remove the hardcoded label; automatic numbering keeps tables and figures contiguous.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].title")),
        );
    }
    *next = number.saturating_add(1);
}

fn check_visual_references(document: &ReportDocument, engine: &mut Engine<'_>) {
    let table_count = document
        .blocks
        .iter()
        .filter(|block| {
            matches!(&block.body, BlockKind::Table(table) if table.numbering_kind() == NumberingKind::Table)
        })
        .count();
    let figure_count = document
        .blocks
        .iter()
        .filter(|block| match &block.body {
            BlockKind::Echarts(chart) => chart.numbering_kind() == NumberingKind::Figure,
            BlockKind::CustomHtml(html) => html.numbering_kind() == NumberingKind::Figure,
            _ => false,
        })
        .count();
    for (index, block) in document.blocks.iter().enumerate() {
        let text = match &block.body {
            BlockKind::Paragraph { text } | BlockKind::Quote { text } => text,
            _ => continue,
        };
        let mut reported = BTreeSet::new();
        for (kind, number) in visual_references(text) {
            let available = if kind == "表" {
                table_count
            } else {
                figure_count
            };
            if number <= available || !reported.insert((kind, number)) {
                continue;
            }
            engine.report(
                RuleId::DanglingVisualRef,
                format!(
                    "block `{}` references {kind} {number}, but the document has only {available}"
                , block.id),
                "fix the reference number or add the missing visual block; automatic labels start at 1 within each kind.",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].text")),
            );
        }
    }
}

fn check_inline_syntax(document: &ReportDocument, engine: &mut Engine<'_>) {
    for (index, block) in document.blocks.iter().enumerate() {
        let text = match &block.body {
            BlockKind::Heading { text, .. }
            | BlockKind::Paragraph { text }
            | BlockKind::Quote { text } => text,
            _ => continue,
        };
        let mut issues = Vec::new();
        if !text.matches("**").count().is_multiple_of(2) {
            issues.push("unclosed `**` bold marker");
        }
        if has_unpaired_math_delimiter(text) {
            issues.push("unpaired `$` math delimiter");
        }
        if has_unclosed_link(text) {
            issues.push("unclosed `[text](` link target");
        }
        if issues.is_empty() {
            continue;
        }
        engine.report(
            RuleId::UnclosedInlineSyntax,
            format!("block `{}` has {}", block.id, issues.join(", ")),
            "close the inline marker or remove it so Markdown projection and rendering stay deterministic.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].text")),
        );
    }
}

fn check_consecutive_visuals(document: &ReportDocument, engine: &mut Engine<'_>) {
    let mut visual_streak = 0usize;
    for (index, block) in document.blocks.iter().enumerate() {
        match &block.body {
            BlockKind::Table(_) | BlockKind::Echarts(_) | BlockKind::CustomHtml(_) => {
                visual_streak = visual_streak.saturating_add(1);
                if visual_streak == 3 {
                    engine.report(
                        RuleId::ConsecutiveVisualsWithoutProse,
                        format!(
                            "block `{}` is the 3rd consecutive visual block without intervening analytical prose",
                            block.id
                        ),
                        "Between these visual blocks via apply_operations (InsertAfter), insert a concise analytical Paragraph or Quote that states the takeaway, comparison, or anomaly; fix this together with QR412/QR413 in one apply_operations batch when they hit the same block.",
                        Some(block.id.clone()),
                        Some(format!("blocks[{index}]")),
                    );
                }
            }
            BlockKind::Paragraph { text } | BlockKind::Quote { text } => {
                if !text.trim().is_empty() {
                    visual_streak = 0;
                }
            }
            BlockKind::Heading { .. } => {}
        }
    }
}

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
];

fn check_boring_headings(document: &ReportDocument, engine: &mut Engine<'_>) {
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
        let lowered = normalized.to_ascii_lowercase();
        let is_generic = GENERIC_HEADINGS_ZH.contains(&normalized)
            || GENERIC_HEADINGS_EN.contains(&lowered.as_str());
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
            "Rewrite this Heading via apply_operations as a conclusion-first title naming the specific subject, metric, or takeaway (e.g. '算力与政策双轮驱动 Q3 增速达 24%'); fix this together with QR412/QR413 in one apply_operations batch when they hit the same block.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].text")),
        );
    }
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
    let first = text.chars().next()?;
    if !is_chinese_numeral(first) {
        return None;
    }
    let rest = text.get(first.len_utf8()..)?;
    rest.strip_prefix('、')
}

fn ascii_digit_prefix_len(text: &str) -> usize {
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

fn chapter_numeral_prefix_len(text: &str) -> usize {
    let mut end = 0usize;
    for (index, ch) in text.char_indices() {
        if ch.is_ascii_digit() || is_chinese_numeral(ch) {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    end
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
    for adjective in ["核心", "主要", "关键"] {
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

fn first_placeholder(text: &str) -> Option<&'static str> {
    let lowered = text.to_ascii_lowercase();
    for token in ["tbd", "todo", "fixme", "tba"] {
        if contains_ascii_word(&lowered, token) {
            return Some(token);
        }
    }
    [
        "[待补充]",
        "[待核实]",
        "[待完善]",
        "[待确认]",
        "[xx]",
        "[xxx]",
        "待补充",
        "待核实",
        "待完善",
        "待确认",
    ]
    .into_iter()
    .find(|token| lowered.contains(*token))
}

fn contains_ascii_word(haystack: &str, needle: &str) -> bool {
    let mut start = 0;
    while let Some(found) = haystack.get(start..).and_then(|rest| rest.find(needle)) {
        let absolute = start + found;
        let before = haystack
            .get(..absolute)
            .and_then(|prefix| prefix.chars().next_back());
        let after = haystack
            .get(absolute + needle.len()..)
            .and_then(|suffix| suffix.chars().next());
        let before_ok = before.is_none_or(|ch| !ch.is_ascii_alphabetic());
        let after_ok = after.is_none_or(|ch| !ch.is_ascii_alphabetic());
        if before_ok && after_ok {
            return true;
        }
        start = absolute + needle.len();
    }
    false
}

fn manual_heading_depth(text: &str) -> Option<usize> {
    let trimmed = text.trim_start();
    let first = trimmed.chars().next()?;
    if first.is_ascii_digit() {
        let token: String = trimmed
            .chars()
            .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
            .collect();
        let has_dot = token.contains('.');
        let rest = trimmed.get(token.len()..)?.trim_start();
        let chinese_separator = rest.starts_with('、')
            || rest.starts_with(')')
            || rest.starts_with('）')
            || rest.starts_with('．');
        if !has_dot && !chinese_separator {
            return None;
        }
        let depth = token
            .split('.')
            .filter(|segment| !segment.is_empty())
            .count();
        return (depth > 0).then_some(depth);
    }
    if matches!(first, '第') {
        return Some(if trimmed.contains('节') { 2 } else { 1 });
    }
    let rest = trimmed.get(first.len_utf8()..)?;
    if rest.starts_with('、') && is_chinese_numeral(first) {
        return Some(1);
    }
    None
}

fn is_chinese_numeral(ch: char) -> bool {
    matches!(
        ch,
        '一' | '二' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十' | '百'
    )
}

fn manual_caption(title: &str) -> Option<(&'static str, usize)> {
    let trimmed = title.trim_start();
    let kind = if trimmed.starts_with('表') {
        "表"
    } else if trimmed.starts_with('图') {
        "图"
    } else {
        return None;
    };
    let rest = trimmed.get(kind.len()..)?.trim_start();
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let number = digits.parse::<usize>().ok()?;
    (number > 0).then_some((kind, number))
}

fn visual_references(text: &str) -> Vec<(&'static str, usize)> {
    let mut refs = Vec::new();
    for (kind, marker) in [("表", '表'), ("图", '图')] {
        for (position, _) in text.match_indices(marker) {
            let rest = text.get(position + marker.len_utf8()..).unwrap_or_default();
            let rest = rest.trim_start();
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(number) = digits.parse::<usize>()
                && number > 0
            {
                refs.push((kind, number));
            }
        }
    }
    for (kind, lower_marker) in [("表", "table"), ("图", "figure")] {
        let lowered = text.to_ascii_lowercase();
        for (position, _) in lowered.match_indices(lower_marker) {
            let rest = lowered
                .get(position + lower_marker.len()..)
                .unwrap_or_default()
                .trim_start();
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(number) = digits.parse::<usize>()
                && number > 0
            {
                refs.push((kind, number));
            }
        }
    }
    refs
}

fn has_unclosed_link(text: &str) -> bool {
    let mut start = 0;
    while let Some(found) = text.get(start..).and_then(|rest| rest.find("](")) {
        let absolute = start + found;
        let after = absolute + 2;
        let Some(rest) = text.get(after..) else {
            return true;
        };
        if !rest.contains(')') {
            return true;
        }
        start = after;
    }
    false
}

/// Count only math delimiters. Currency forms such as `$500`, `$ 500` and `$12.5M`
/// are not formula delimiters, and escaped `\$` is ignored.
fn has_unpaired_math_delimiter(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let mut delimiters = 0usize;
    for (index, ch) in chars.iter().enumerate() {
        if *ch != '$' {
            continue;
        }
        if index
            .checked_sub(1)
            .and_then(|previous| chars.get(previous))
            == Some(&'\\')
        {
            continue;
        }
        let mut next = index + 1;
        while let Some(candidate) = chars.get(next) {
            if candidate.is_whitespace() {
                next += 1;
                continue;
            }
            break;
        }
        if chars
            .get(next)
            .is_some_and(|candidate| candidate.is_ascii_digit())
        {
            continue;
        }
        delimiters += 1;
    }
    !delimiters.is_multiple_of(2)
}

fn has_manual_number_prefix(title: &str) -> bool {
    let lower = title.trim().to_ascii_lowercase();
    let mut chars = lower.chars().peekable();
    let Some(first) = chars.next() else {
        return false;
    };
    if matches!(first, '图' | '表') {
        let mut saw_digit = false;
        for ch in chars.by_ref() {
            if ch.is_ascii_digit() {
                saw_digit = true;
                break;
            }
            if ch == ' ' || ch == '\u{3000}' {
                continue;
            }
            break;
        }
        return saw_digit;
    }
    let rest = lower.as_str();
    for prefix in ["figure", "fig.", "table", "tab."] {
        if let Some(mut remainder) = rest.strip_prefix(prefix) {
            remainder = remainder.trim_start();
            if remainder
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_digit())
            {
                return true;
            }
        }
    }
    false
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
