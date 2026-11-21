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
    check_visual_titles(document, engine);
    check_visual_references(document, engine);
    check_inline_syntax(document, engine);
    check_consecutive_visuals(document, engine);
    check_boring_headings(document, engine);
    check_duplicate_note_prefix(document, engine);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum HeadingNumberingFamily {
    Chapter(usize),
    ChineseTop,
    ParenthesizedChinese,
    Arabic,
    ParenthesizedArabic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HeadingNumbering {
    family: HeadingNumberingFamily,
    components: Vec<usize>,
}

impl HeadingNumbering {
    fn is_root(&self) -> bool {
        match self.family {
            HeadingNumberingFamily::Chapter(1) | HeadingNumberingFamily::ChineseTop => true,
            HeadingNumberingFamily::Arabic => self.components.len() == 1,
            _ => false,
        }
    }

    fn system(&self) -> NumberingSystem {
        if self.family == HeadingNumberingFamily::Arabic {
            NumberingSystem::Arabic
        } else {
            NumberingSystem::Chinese
        }
    }

    fn rank(&self, system: NumberingSystem) -> usize {
        match self.family {
            HeadingNumberingFamily::Chapter(rank) => rank,
            HeadingNumberingFamily::ChineseTop => 1,
            HeadingNumberingFamily::ParenthesizedChinese => 2,
            HeadingNumberingFamily::Arabic if self.components.len() > 1 => self.components.len(),
            HeadingNumberingFamily::Arabic => match system {
                NumberingSystem::Chinese => 3,
                NumberingSystem::Arabic => 1,
            },
            HeadingNumberingFamily::ParenthesizedArabic => match system {
                NumberingSystem::Chinese => 4,
                NumberingSystem::Arabic => 2,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum NumberingSystem {
    Chinese,
    Arabic,
}

type HeadingSequences =
    BTreeMap<(Option<quire_protocol::BlockId>, u8, HeadingNumberingFamily), Vec<usize>>;

fn heading_numbering_base(document: &ReportDocument) -> (u8, NumberingSystem) {
    let mut fallback = None;
    for block in &document.blocks {
        let BlockKind::Heading { level, text } = &block.body else {
            continue;
        };
        let Some(numbering) = parse_heading_numbering(text) else {
            continue;
        };
        if fallback.is_none() {
            fallback = Some(numbering.system());
        }
        if numbering.is_root() {
            return (*level, numbering.system());
        }
    }
    (1, fallback.unwrap_or(NumberingSystem::Chinese))
}

fn check_headings(document: &ReportDocument, engine: &mut Engine<'_>) {
    let (numbering_base, numbering_system) = heading_numbering_base(document);
    let mut previous_heading_level: Option<u8> = None;
    let mut heading_stack: Vec<(u8, quire_protocol::BlockId)> = Vec::new();
    let mut sequences = HeadingSequences::new();
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

            while heading_stack
                .last()
                .is_some_and(|(parent_level, _)| *parent_level >= *level)
            {
                heading_stack.pop();
            }
            let parent_id = heading_stack.last().map(|(_, block_id)| block_id.clone());
            if let Some(numbering) = parse_heading_numbering(text) {
                let rank = numbering.rank(numbering_system);
                let expected_level = usize::from(numbering_base) + rank.saturating_sub(1);
                if expected_level != usize::from(*level) {
                    engine.report(
                        RuleId::HeadingNumberingMismatch,
                        format!(
                            "heading `{}` uses relative level-{rank} numbering `{}` at h{level}; the document's numbering root is h{numbering_base}",
                            block.id,
                            text.trim()
                        ),
                        "match the numbering to the document's relative heading hierarchy, or remove the manual number and let the TOC number it.",
                        Some(block.id.clone()),
                        Some(format!("blocks[{index}].text")),
                    );
                }
                check_heading_sequence(
                    engine,
                    block,
                    index,
                    &numbering,
                    parent_id,
                    *level,
                    &mut sequences,
                );
            }
            heading_stack.push((*level, block.id.clone()));
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

fn check_heading_sequence(
    engine: &mut Engine<'_>,
    block: &quire_protocol::Block,
    index: usize,
    numbering: &HeadingNumbering,
    parent_id: Option<quire_protocol::BlockId>,
    level: u8,
    sequences: &mut HeadingSequences,
) {
    let Some(current) = numbering.components.last().copied() else {
        return;
    };
    let previous = sequences
        .entry((parent_id, level, numbering.family))
        .or_default();
    let previous_label = previous
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(".");
    let current_label = numbering
        .components
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(".");
    let out_of_sequence = if previous.is_empty() {
        current != 1
    } else {
        let previous_prefix = previous.get(..previous.len().saturating_sub(1));
        let current_prefix = numbering
            .components
            .get(..numbering.components.len().saturating_sub(1));
        previous_prefix != current_prefix
            || previous.last().and_then(|value| value.checked_add(1)) != Some(current)
    };
    if out_of_sequence {
        let message = if previous_label.is_empty() {
            format!(
                "heading `{}` numbering `{current_label}` starts at {current} instead of 1",
                block.id
            )
        } else {
            format!(
                "heading `{}` numbering `{current_label}` does not follow `{previous_label}` within the same parent section",
                block.id
            )
        };
        engine.report(
            RuleId::HeadingNumberingMismatch,
            message,
            "renumber sibling headings continuously from 1 within each parent section; keep one numbering style per sibling group.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].text")),
        );
    }
    *previous = numbering.components.clone();
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
        let mut found: Option<(String, String)> = None;
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

fn check_visual_titles(document: &ReportDocument, engine: &mut Engine<'_>) {
    for (index, block) in document.blocks.iter().enumerate() {
        let title = match &block.body {
            BlockKind::Table(table) => table.title.as_deref(),
            BlockKind::Echarts(chart) => chart.title.as_deref(),
            BlockKind::CustomHtml(html) => html.title.as_deref(),
            BlockKind::Heading { .. } | BlockKind::Paragraph { .. } | BlockKind::Quote { .. } => {
                continue;
            }
        };
        check_visual_title(engine, block, title, index);
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
        for reference in visual_references(text) {
            let available = if reference.kind == "表" {
                table_count
            } else {
                figure_count
            };
            let unsupported_format = reference.compound || reference.appendix;
            if (!unsupported_format && reference.number > 0 && reference.number <= available)
                || !reported.insert((reference.kind, reference.raw.clone()))
            {
                continue;
            }
            let (message, hint) = if unsupported_format {
                (
                    format!(
                        "block `{}` uses unsupported visual reference `{}`",
                        block.id, reference.raw
                    ),
                    "use the renderer's global continuous labels (表 1, 表 2 / 图 1, 图 2); do not use chapter-scoped, decimal, hyphenated or appendix-only numbers".to_string(),
                )
            } else {
                (
                    format!(
                        "block `{}` references {}, but the document has only {available}",
                        block.id, reference.raw
                    ),
                    "fix the reference number or add the missing visual block; automatic labels start at 1 within each kind".to_string(),
                )
            };
            engine.report(
                RuleId::DanglingVisualRef,
                message,
                hint,
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
            "Rewrite this Heading via apply_operations to name a specific subject, metric or takeaway. For a standard official-document section, keep its column label and add a concrete judgment after a colon (e.g. '对策建议：依托省重点实验室集中引育领军人才'); fix this together with QR412/QR413 in one apply_operations batch when they hit the same block.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].text")),
        );
    }
}

fn check_duplicate_note_prefix(document: &ReportDocument, engine: &mut Engine<'_>) {
    for (index, block) in document.blocks.iter().enumerate() {
        let note = match &block.body {
            BlockKind::Table(table) => table.note.as_deref(),
            BlockKind::Echarts(chart) => chart.note.as_deref(),
            _ => None,
        };
        let Some(note) = note else {
            continue;
        };
        if !has_redundant_note_prefix(note) {
            continue;
        }
        engine.report(
            RuleId::DuplicateNotePrefix,
            format!(
                "{} `{}` note starts with a redundant `注：` prefix",
                block.block_type(),
                block.id
            ),
            "In `note` via apply_operations (UpdateBlock) or preview_echarts, strip the leading `注：` / `图注：` / `表注：` prefix because the report renderer prepends `注：` automatically.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].note")),
        );
    }
}

fn has_redundant_note_prefix(note: &str) -> bool {
    let trimmed = note.trim_start();
    trimmed.starts_with("注：")
        || trimmed.starts_with("注:")
        || trimmed.starts_with("表注：")
        || trimmed.starts_with("表注:")
        || trimmed.starts_with("图注：")
        || trimmed.starts_with("图注:")
        || trimmed
            .get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("Note:"))
}

pub(super) fn dense_prose_section_exempt(text: &str) -> bool {
    let label = section_label(text);
    let lowered = label.to_ascii_lowercase();
    heading_list_contains(OPENING_SECTION_HEADINGS, label, &lowered)
        || heading_list_contains(CLOSING_SECTION_HEADINGS, label, &lowered)
        || heading_list_contains(DENSE_PROSE_EXEMPT_SECTION_HEADINGS, label, &lowered)
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

fn first_placeholder(text: &str) -> Option<String> {
    let lowered = text.to_ascii_lowercase();
    for token in ["tbd", "todo", "fixme", "tba"] {
        if contains_ascii_word(&lowered, token) {
            return Some(token.to_string());
        }
    }
    for token in [
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
    ] {
        if lowered.contains(token) {
            return Some(token.to_string());
        }
    }
    if text.contains("××") {
        return Some("××".to_string());
    }
    for token in [
        "xx高校", "xx大学", "xx学院", "xx学科", "xx项", "xx名", "xx所",
    ] {
        if lowered.contains(token) {
            return Some(token.to_string());
        }
    }
    for (open, close) in [('【', '】'), ('（', '）'), ('(', ')')] {
        if let Some(token) = wrapped_pending_placeholder(text, open, close) {
            return Some(token);
        }
    }
    for token in ["?%", "??"] {
        if text.contains(token) {
            return Some(token.to_string());
        }
    }
    independent_n_quantifier(text)
}

fn wrapped_pending_placeholder(text: &str, open: char, close: char) -> Option<String> {
    let mut search_start = 0usize;
    while let Some(found) = text.get(search_start..).and_then(|rest| rest.find(open)) {
        let start = search_start + found;
        let content_start = start + open.len_utf8();
        let rest = text.get(content_start..)?;
        if rest.starts_with('待')
            && let Some(close_index) = rest.find(close)
        {
            return text
                .get(start..content_start + close_index + close.len_utf8())
                .map(str::to_string);
        }
        search_start = content_start;
    }
    None
}

fn independent_n_quantifier(text: &str) -> Option<String> {
    for (index, _) in text.match_indices('N') {
        let previous = text
            .get(..index)
            .and_then(|prefix| prefix.chars().next_back());
        if previous.is_some_and(|ch| ch.is_ascii_alphanumeric()) {
            continue;
        }
        let after = text.get(index + 1..)?;
        let trimmed = after.trim_start();
        let unit = trimmed.chars().next()?;
        if !matches!(unit, '项' | '名' | '篇' | '所' | '个' | '位') {
            continue;
        }
        let whitespace = after.len().saturating_sub(trimmed.len());
        return text
            .get(index..index + 1 + whitespace + unit.len_utf8())
            .map(str::to_string);
    }
    None
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

fn parse_heading_numbering(text: &str) -> Option<HeadingNumbering> {
    let trimmed = text.trim_start();
    if let Some(rest) = trimmed.strip_prefix('第') {
        let numeral_len = chapter_numeral_prefix_len(rest);
        if numeral_len > 0 {
            let value = parse_numeral_token(rest.get(..numeral_len)?)?;
            let after = rest.get(numeral_len..)?;
            for (marker, rank) in [("部分", 1), ("章", 1), ("节", 2), ("篇", 1)] {
                if after.starts_with(marker) {
                    return Some(HeadingNumbering {
                        family: HeadingNumberingFamily::Chapter(rank),
                        components: vec![value],
                    });
                }
            }
        }
    }

    if let Some(first) = trimmed.chars().next()
        && matches!(first, '（' | '(')
    {
        let close = if first == '（' { '）' } else { ')' };
        let rest = trimmed.get(first.len_utf8()..)?;
        let close_index = rest.find(close)?;
        let inner = rest.get(..close_index)?;
        if !inner.is_empty() && chapter_numeral_prefix_len(inner) == inner.len() {
            let value = parse_numeral_token(inner)?;
            let family = if inner.bytes().all(|byte| byte.is_ascii_digit()) {
                HeadingNumberingFamily::ParenthesizedArabic
            } else {
                HeadingNumberingFamily::ParenthesizedChinese
            };
            return Some(HeadingNumbering {
                family,
                components: vec![value],
            });
        }
    }

    if trimmed.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        return parse_arabic_heading_numbering(trimmed);
    }

    let numeral_len = chapter_numeral_prefix_len(trimmed);
    if numeral_len > 0
        && trimmed
            .get(numeral_len..)
            .is_some_and(|rest| rest.starts_with('、'))
    {
        return Some(HeadingNumbering {
            family: HeadingNumberingFamily::ChineseTop,
            components: vec![parse_numeral_token(trimmed.get(..numeral_len)?)?],
        });
    }
    None
}

fn parse_arabic_heading_numbering(text: &str) -> Option<HeadingNumbering> {
    let mut components = Vec::new();
    let mut cursor = 0usize;
    let mut saw_dot = false;
    loop {
        let digit_len = ascii_digit_prefix_len(text.get(cursor..)?);
        if digit_len == 0 {
            return None;
        }
        let value = text
            .get(cursor..cursor + digit_len)?
            .parse::<usize>()
            .ok()?;
        components.push(value);
        cursor += digit_len;
        let after = text.get(cursor..)?;
        let Some(separator) = after.chars().next() else {
            break;
        };
        if !matches!(separator, '.' | '．') {
            break;
        }
        saw_dot = true;
        cursor += separator.len_utf8();
        if ascii_digit_prefix_len(text.get(cursor..)?) == 0 {
            break;
        }
    }
    let after = text.get(cursor..)?;
    if after.starts_with('%') || after.starts_with('％') || after.starts_with('‰') {
        return None;
    }
    let valid =
        saw_dot || after.starts_with('、') || after.starts_with(')') || after.starts_with('）');
    valid.then_some(HeadingNumbering {
        family: HeadingNumberingFamily::Arabic,
        components,
    })
}

fn parse_numeral_token(text: &str) -> Option<usize> {
    if !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()) {
        return text.parse::<usize>().ok();
    }
    parse_chinese_numeral(text)
}

fn parse_chinese_numeral(text: &str) -> Option<usize> {
    let mut total = 0usize;
    let mut current = 0usize;
    let mut saw_numeral = false;
    for ch in text.chars() {
        let digit = match ch {
            '零' => Some(0),
            '一' => Some(1),
            '二' | '两' => Some(2),
            '三' => Some(3),
            '四' => Some(4),
            '五' => Some(5),
            '六' => Some(6),
            '七' => Some(7),
            '八' => Some(8),
            '九' => Some(9),
            _ => None,
        };
        if let Some(digit) = digit {
            current = digit;
            saw_numeral = true;
            continue;
        }
        let unit = match ch {
            '十' => 10usize,
            '百' => 100usize,
            _ => return None,
        };
        let value = if current == 0 { 1 } else { current };
        total = total.checked_add(value.checked_mul(unit)?)?;
        current = 0;
        saw_numeral = true;
    }
    if saw_numeral {
        total.checked_add(current)
    } else {
        None
    }
}

fn is_chinese_numeral(ch: char) -> bool {
    matches!(
        ch,
        '零' | '一' | '二' | '两' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十' | '百'
    )
}

struct VisualReference {
    kind: &'static str,
    raw: String,
    number: usize,
    compound: bool,
    appendix: bool,
}

fn visual_references(text: &str) -> Vec<VisualReference> {
    let mut refs = Vec::new();
    for (kind, marker) in [("表", "表"), ("图", "图")] {
        collect_visual_references(text, kind, marker, &mut refs);
    }
    let lowered = text.to_ascii_lowercase();
    for (kind, marker) in [("表", "table"), ("图", "figure")] {
        collect_visual_references(&lowered, kind, marker, &mut refs);
    }
    refs
}

fn collect_visual_references(
    text: &str,
    kind: &'static str,
    marker: &str,
    refs: &mut Vec<VisualReference>,
) {
    for (position, _) in text.match_indices(marker) {
        let after_marker = text.get(position + marker.len()..).unwrap_or_default();
        let rest = after_marker.trim_start();
        let digit_len = ascii_digit_prefix_len(rest);
        if digit_len == 0 {
            continue;
        }
        let Ok(number) = rest.get(..digit_len).unwrap_or_default().parse::<usize>() else {
            continue;
        };

        let mut end = digit_len;
        let mut compound = false;
        loop {
            let after_number = rest.get(end..).unwrap_or_default();
            let Some(separator) = after_number.chars().next() else {
                break;
            };
            if !matches!(separator, '-' | '.' | '—' | '–' | '－' | '．') {
                break;
            }
            let suffix = after_number.get(separator.len_utf8()..).unwrap_or_default();
            let suffix_digits = ascii_digit_prefix_len(suffix);
            if suffix_digits == 0 {
                break;
            }
            compound = true;
            end += separator.len_utf8() + suffix_digits;
        }

        let appendix = text
            .get(..position)
            .and_then(|prefix| prefix.chars().next_back())
            == Some('附');
        let raw_start = if appendix {
            position.saturating_sub('附'.len_utf8())
        } else {
            position
        };
        let leading_space = after_marker.len().saturating_sub(rest.len());
        let raw_end = position + marker.len() + leading_space + end;
        let raw = text.get(raw_start..raw_end).unwrap_or_default().to_string();
        refs.push(VisualReference {
            kind,
            raw,
            number,
            compound,
            appendix,
        });
    }
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
