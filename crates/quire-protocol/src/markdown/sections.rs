use super::*;

pub(crate) fn find_section_heading_start(
    document: &ReportDocument,
    query: &str,
) -> Result<Option<usize>, MarkdownError> {
    // Strip trailing ellipsis (`…` or `...`) when an agent passes a truncated 30-char heading
    // `preview` copied verbatim from `get_document_digest`.
    let trimmed = query.trim().trim_end_matches(['…', '.']).trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let query_lower = trimmed.to_lowercase();

    // 1. Exact Heading block_id match (case-insensitive), e.g. `ch1`, `c1-1`, `intro`.
    if let Some(pos) = document.blocks.iter().position(|block| {
        matches!(&block.body, BlockKind::Heading { .. })
            && block.id.as_str().eq_ignore_ascii_case(trimmed)
    }) {
        return Ok(Some(pos));
    }

    // 2. Direct heading plain-text substring match (case-insensitive).
    if let Some(pos) = unique_heading_match(document, &query_lower)? {
        return Ok(Some(pos));
    }

    // 3. Ordinal chapter prefix / label resolution (`第一章`, `第1章`, `第三章：师资与人才`, `Chapter 1`, `ch1`).
    let (ordinal, remainder) = parse_chapter_query(trimmed);
    if let Some(remainder) = remainder {
        let remainder_lower = remainder.to_lowercase();
        if let Some(pos) = unique_heading_match(document, &remainder_lower)? {
            return Ok(Some(pos));
        }
    }
    if let Some(ordinal) = ordinal {
        let h1_positions: Vec<usize> = document
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(idx, block)| match &block.body {
                BlockKind::Heading { level: 1, .. } => Some(idx),
                _ => None,
            })
            .collect();
        if let Some(pos) = h1_positions.get(ordinal.saturating_sub(1)).copied() {
            return Ok(Some(pos));
        }
    }
    Ok(None)
}

fn unique_heading_match(
    document: &ReportDocument,
    text: &str,
) -> Result<Option<usize>, MarkdownError> {
    let mut matches = document.blocks.iter().enumerate().filter(|(_, block)| {
        matches!(&block.body, BlockKind::Heading { text: heading, .. } if crate::inline::inline_plain_text(heading).to_lowercase().contains(text))
    });
    let Some((index, first)) = matches.next() else {
        return Ok(None);
    };
    let Some((_, second)) = matches.next() else {
        return Ok(Some(index));
    };
    let ids = [first.id.as_str(), second.id.as_str()]
        .into_iter()
        .chain(matches.map(|(_, block)| block.id.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    Err(MarkdownError::AmbiguousSection {
        query: text.to_string(),
        ids,
    })
}

fn parse_chapter_query(query: &str) -> (Option<usize>, Option<&str>) {
    let trimmed = query.trim();
    if let Some(after_di) = trimmed.strip_prefix('第') {
        let mut numeral_bytes = 0usize;
        for ch in after_di.chars() {
            if ch.is_ascii_digit()
                || matches!(
                    ch,
                    '一' | '二' | '两' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十'
                )
            {
                numeral_bytes += ch.len_utf8();
            } else {
                break;
            }
        }
        if numeral_bytes > 0
            && let Some(numeral_str) = after_di.get(..numeral_bytes)
            && let Some(rest) = after_di.get(numeral_bytes..)
        {
            let after_marker = rest
                .strip_prefix("部分")
                .or_else(|| rest.strip_prefix('章'))
                .or_else(|| rest.strip_prefix('节'))
                .or_else(|| rest.strip_prefix('篇'));
            if let Some(tail) = after_marker {
                let ordinal = parse_chinese_or_arabic_ordinal(numeral_str);
                let tail = tail
                    .trim_start_matches(|ch: char| {
                        ch.is_whitespace() || matches!(ch, ':' | '：' | '、' | '-' | '—')
                    })
                    .trim();
                let remainder = (!tail.is_empty()).then_some(tail);
                return (ordinal, remainder);
            }
        }
    }
    let lowered = trimmed.to_ascii_lowercase();
    for prefix in ["chapter", "ch"] {
        if let Some(rest) = lowered.strip_prefix(prefix) {
            let rest_trimmed = rest.trim_start_matches([' ', '_', '-']);
            let digit_len: usize = rest_trimmed
                .chars()
                .take_while(char::is_ascii_digit)
                .map(char::len_utf8)
                .sum();
            if digit_len > 0
                && let Some(digits) = rest_trimmed.get(..digit_len)
                && let Ok(ordinal) = digits.parse::<usize>()
                && ordinal >= 1
            {
                let after = rest_trimmed.get(digit_len..).unwrap_or("");
                if after.is_empty() || after.starts_with([':', '：', ' ', '-', '—']) {
                    return (Some(ordinal), None);
                }
            }
        }
    }
    (None, None)
}

fn parse_chinese_or_arabic_ordinal(input: &str) -> Option<usize> {
    if let Ok(value) = input.parse::<usize>() {
        return (value >= 1).then_some(value);
    }
    match input {
        "一" => Some(1),
        "二" | "两" => Some(2),
        "三" => Some(3),
        "四" => Some(4),
        "五" => Some(5),
        "六" => Some(6),
        "七" => Some(7),
        "八" => Some(8),
        "九" => Some(9),
        "十" => Some(10),
        "十一" => Some(11),
        "十二" => Some(12),
        "十三" => Some(13),
        "十四" => Some(14),
        "十五" => Some(15),
        "十六" => Some(16),
        "十七" => Some(17),
        "十八" => Some(18),
        "十九" => Some(19),
        "二十" => Some(20),
        _ => None,
    }
}

pub(crate) fn heading_list(document: &ReportDocument) -> String {
    let headings: Vec<String> = document
        .blocks
        .iter()
        .filter_map(|block| match &block.body {
            BlockKind::Heading { level, text } => Some(format!(
                "h{level} {} ({})",
                collapse_whitespace(&crate::inline::inline_plain_text(text)),
                block.id
            )),
            _ => None,
        })
        .collect();
    if headings.is_empty() {
        "(none)".to_string()
    } else {
        headings.join("; ")
    }
}
