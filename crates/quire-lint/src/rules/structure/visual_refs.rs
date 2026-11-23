use super::*;

pub(super) fn check_visual_titles(document: &ReportDocument, engine: &mut Engine) {
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
    engine: &mut Engine,
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

pub(super) fn check_visual_references(document: &ReportDocument, engine: &mut Engine) {
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

pub(super) fn check_consecutive_visuals(document: &ReportDocument, engine: &mut Engine) {
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
