use super::*;

pub(super) fn check_inline_syntax(document: &ReportDocument, engine: &mut Engine) {
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

pub(super) fn check_duplicate_note_prefix(document: &ReportDocument, engine: &mut Engine) {
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

pub(super) fn check_duplicate_source_prefix(document: &ReportDocument, engine: &mut Engine) {
    for (index, block) in document.blocks.iter().enumerate() {
        if !matches!(
            &block.body,
            BlockKind::Table(_) | BlockKind::Echarts(_) | BlockKind::CustomHtml(_)
        ) {
            continue;
        }
        let Some(source_note) = block.source_note.as_deref() else {
            continue;
        };
        let Some(prefix) = redundant_source_prefix(source_note) else {
            continue;
        };
        engine.report(
            RuleId::DuplicateSourcePrefix,
            format!(
                "{} `{}` source_note starts with a redundant `{prefix}：` prefix",
                block.block_type(),
                block.id
            ),
            "In `source_note` via apply_operations (UpdateBlock) or preview_echarts, strip the leading `资料来源：` / `数据来源：` / `来源：` prefix because the report renderer prepends `资料来源：` automatically; keep only the institution or document name.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].source_note")),
        );
    }
}

/// Detect a leading source label followed by a colon. The label set is
/// deliberately open-ended: any short Chinese label ending in `来源` or
/// `出处` (including chains such as `资料来源：来源：`) is redundant because
/// the renderer always prepends `资料来源：`.
fn redundant_source_prefix(source_note: &str) -> Option<&str> {
    let trimmed = source_note.trim_start();
    let colon = trimmed.find([':', '：'])?;
    let label = trimmed.get(..colon)?.trim();
    if label.is_empty() || label.chars().count() > 16 || label.contains('\n') {
        return None;
    }
    let compact: String = label
        .chars()
        .filter(|value| !value.is_whitespace())
        .collect();
    let base = compact
        .split(['（', '('])
        .next()
        .unwrap_or(compact.as_str());
    if base.ends_with("来源") || base.ends_with("出处") {
        return Some(label);
    }
    let lowered = label.to_ascii_lowercase();
    matches!(
        lowered.as_str(),
        "source" | "sources" | "data source" | "data sources" | "reference" | "references"
    )
    .then_some(label)
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
