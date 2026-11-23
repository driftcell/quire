use super::*;

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

pub(super) fn check_headings(document: &ReportDocument, engine: &mut Engine) {
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
    engine: &mut Engine,
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
