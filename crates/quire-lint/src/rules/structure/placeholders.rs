use super::*;

pub(super) fn check_empty_text(document: &ReportDocument, engine: &mut Engine) {
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

pub(super) fn check_placeholders(document: &ReportDocument, engine: &mut Engine) {
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
    for (open, close) in [('【', '】'), ('（', '）'), ('(', ')'), ('[', ']')] {
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
