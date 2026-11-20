//! Table layout risk checks: narrow CJK headers, empty/constant columns, A4 overflow and units.

use std::collections::BTreeSet;

use quire_protocol::{
    BlockKind, ReportDocument, TableBlock, TableShape, display_cell, validate_table,
};

use crate::{engine::Engine, registry::RuleId};

const A4_CONTENT_WIDTH_PX: f64 = 640.0;
const MIN_CELL_PADDING_PX: f64 = 24.0;
const PX_PER_WIDTH_UNIT: f64 = 8.0;

pub(crate) fn check(document: &ReportDocument, engine: &mut Engine<'_>) {
    for (index, block) in document.blocks.iter().enumerate() {
        let BlockKind::Table(table) = &block.body else {
            continue;
        };
        let Ok(shape) = validate_table(table) else {
            continue;
        };
        check_column_width(engine, block, &shape, index);
        check_empty_columns(engine, block, table, &shape, index);
        check_constant_columns(engine, block, table, &shape, index);
        check_a4_width(engine, block, table, &shape, index);
        check_numeric_units(engine, block, table, &shape, index);
    }
}

fn check_column_width(
    engine: &mut Engine<'_>,
    block: &quire_protocol::Block,
    shape: &TableShape,
    index: usize,
) {
    let column_count = shape.leaves.len();
    if column_count == 0 {
        return;
    }
    let implicit_width = A4_CONTENT_WIDTH_PX / column_count as f64;
    for leaf in &shape.leaves {
        if !has_cjk(&leaf.label) {
            continue;
        }
        let header_units = display_width(&leaf.label);
        if header_units < 6 {
            continue;
        }
        let required = header_units as f64 * PX_PER_WIDTH_UNIT + MIN_CELL_PADDING_PX;
        let explicit = block
            .style
            .column_widths
            .as_ref()
            .and_then(|widths| widths.get(&leaf.key))
            .and_then(|raw| parse_css_px(raw));
        let available = explicit.unwrap_or(implicit_width);
        if available + 0.5 >= required {
            continue;
        }
        let reason = if explicit.is_some() {
            format!(
                "explicit column width is {:.0}px but header `{}` needs about {:.0}px",
                available, leaf.label, required
            )
        } else {
            format!(
                "{column_count} columns leave about {:.0}px per column, but header `{}` needs about {:.0}px",
                available, leaf.label, required
            )
        };
        engine.report(
            RuleId::TableColumnTooNarrow,
            format!("table `{}` column `{}` is too narrow: {reason}", block.id, leaf.key),
            "shorten the header, reduce the column count, or reserve a wider explicit width such as `120px`/`20%`; CJK headers wrap vertically when squeezed.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].columns.{}", leaf.key)),
        );
    }
}

fn check_empty_columns(
    engine: &mut Engine<'_>,
    block: &quire_protocol::Block,
    table: &TableBlock,
    shape: &TableShape,
    index: usize,
) {
    if table.rows.is_empty() {
        return;
    }
    for leaf in &shape.leaves {
        let all_empty = table.rows.iter().all(|row| {
            row.get(&leaf.key)
                .is_none_or(|cell| is_placeholder_empty(&cell.display_text()))
        });
        if !all_empty {
            continue;
        }
        engine.report(
            RuleId::TableEmptyColumn,
            format!(
                "table `{}` column `{}` (`{}`) is empty in every row",
                block.id, leaf.key, leaf.label
            ),
            "remove the column, fill it with real values, or ignore this rule only when the empty column is intentional.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].rows.{}", leaf.key)),
        );
    }
}

fn check_constant_columns(
    engine: &mut Engine<'_>,
    block: &quire_protocol::Block,
    table: &TableBlock,
    shape: &TableShape,
    index: usize,
) {
    if table.rows.len() < 3 {
        return;
    }
    for leaf in &shape.leaves {
        let values: Vec<String> = table
            .rows
            .iter()
            .filter_map(|row| row.get(&leaf.key))
            .map(|cell| display_cell(cell, leaf.data_type))
            .collect();
        if values.len() < 3 || values.iter().all(|value| is_placeholder_empty(value)) {
            continue;
        }
        let Some(first) = values.first() else {
            continue;
        };
        if values.iter().all(|value| value == first) {
            engine.report(
                RuleId::TableConstantColumn,
                format!(
                    "table `{}` column `{}` (`{}`) repeats `{}` in every row",
                    block.id, leaf.key, leaf.label, first
                ),
                "drop constant columns or move the shared value into the caption/note; a zero-entropy column wastes horizontal width.",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].rows.{}", leaf.key)),
            );
        }
    }
}

fn check_a4_width(
    engine: &mut Engine<'_>,
    block: &quire_protocol::Block,
    table: &TableBlock,
    shape: &TableShape,
    index: usize,
) {
    if shape.leaves.len() < 6 {
        return;
    }
    let mut natural_width = 0.0;
    for leaf in &shape.leaves {
        let header = display_width(&leaf.label) as f64;
        let body_total: usize = table
            .rows
            .iter()
            .filter_map(|row| row.get(&leaf.key))
            .map(|cell| display_width(&display_cell(cell, leaf.data_type)))
            .sum();
        let body_average = if table.rows.is_empty() {
            0.0
        } else {
            body_total as f64 / table.rows.len() as f64
        };
        let widest = header.max(body_average);
        natural_width += widest * PX_PER_WIDTH_UNIT + MIN_CELL_PADDING_PX;
    }
    if natural_width <= A4_CONTENT_WIDTH_PX * 1.5 {
        return;
    }
    engine.report(
        RuleId::TableTooWideForA4,
        format!(
            "table `{}` has {} columns and about {:.0}px of natural width, exceeding A4 portrait comfort",
            block.id,
            shape.leaves.len(),
            natural_width
        ),
        "split the table, move low-priority columns into notes, or shorten cell text so the table fits a 640px A4 content width.",
        Some(block.id.clone()),
        Some(format!("blocks[{index}].columns")),
    );
}

fn check_numeric_units(
    engine: &mut Engine<'_>,
    block: &quire_protocol::Block,
    table: &TableBlock,
    shape: &TableShape,
    index: usize,
) {
    for leaf in &shape.leaves {
        let values: Vec<String> = table
            .rows
            .iter()
            .filter_map(|row| row.get(&leaf.key))
            .map(|cell| display_cell(cell, leaf.data_type))
            .filter(|value| !is_placeholder_empty(value))
            .collect();
        if values.len() < 2 {
            continue;
        }
        let mut has_percent = false;
        let mut has_plain_number = false;
        let mut currencies = BTreeSet::new();
        let mut decimals = BTreeSet::new();
        for value in &values {
            let trimmed = value.trim();
            if trimmed.contains('%') {
                has_percent = true;
            }
            for symbol in ['¥', '$', '€', '£', '￥'] {
                if trimmed.contains(symbol) {
                    currencies.insert(symbol);
                }
            }
            let normalized = trimmed.trim_matches(|ch: char| {
                ch.is_whitespace() || matches!(ch, '%' | '¥' | '$' | '€' | '£' | '￥' | ',' | '，')
            });
            if normalized.parse::<f64>().is_ok() {
                if !trimmed.contains('%') {
                    has_plain_number = true;
                }
                let decimals_for_value = trimmed
                    .split_once('.')
                    .map(|(_, fraction)| fraction.chars().take_while(char::is_ascii_digit).count())
                    .unwrap_or(0);
                decimals.insert(decimals_for_value);
            }
        }
        let mixed_percent = has_percent && has_plain_number;
        let mixed_currency = currencies.len() > 1;
        let spread_decimals = decimals
            .iter()
            .next_back()
            .zip(decimals.iter().next())
            .is_some_and(|(max, min)| max.saturating_sub(*min) >= 2);
        if !(mixed_percent || mixed_currency || spread_decimals) {
            continue;
        }
        let mut reasons = Vec::new();
        if mixed_percent {
            reasons.push("mixes percentages with plain numbers".to_string());
        }
        if mixed_currency {
            reasons.push(format!(
                "mixes currency symbols {}",
                currencies
                    .into_iter()
                    .map(|symbol| symbol.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }
        if spread_decimals {
            reasons.push("mixes very different decimal precision".to_string());
        }
        engine.report(
            RuleId::TableNumericUnitInconsistent,
            format!(
                "table `{}` column `{}` (`{}`) {}",
                block.id,
                leaf.key,
                leaf.label,
                reasons.join("; ")
            ),
            "normalize the unit and decimal precision inside one numeric column, or split mixed units into separate columns.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].rows.{}", leaf.key)),
        );
    }
}

fn display_width(text: &str) -> usize {
    text.chars().map(|ch| if is_wide(ch) { 2 } else { 1 }).sum()
}

fn has_cjk(text: &str) -> bool {
    text.chars().any(is_wide)
}

fn is_wide(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
    )
}

fn is_placeholder_empty(value: &str) -> bool {
    let value = value.trim();
    value.is_empty()
        || matches!(
            value.to_ascii_lowercase().as_str(),
            "-" | "--" | "—" | "–" | "/" | "n/a" | "na"
        )
        || matches!(value, "暂无" | "无")
}

fn parse_css_px(raw: &str) -> Option<f64> {
    let value = raw.trim().to_ascii_lowercase();
    if value.is_empty() || value.len() > 32 {
        return None;
    }
    if let Some(number) = value.strip_suffix('%') {
        let pct = number.trim().parse::<f64>().ok()?;
        return pct.is_finite().then_some(A4_CONTENT_WIDTH_PX * pct / 100.0);
    }
    let unit_start = value
        .find(|ch: char| ch.is_ascii_alphabetic())
        .unwrap_or(value.len());
    let number = value.get(..unit_start)?;
    let unit = value.get(unit_start..)?;
    let number = number.parse::<f64>().ok()?;
    if !number.is_finite() || number < 0.0 {
        return None;
    }
    let px = match unit {
        "" => number,
        "px" => number,
        "pt" => number * 96.0 / 72.0,
        "pc" => number * 16.0,
        "in" => number * 96.0,
        "cm" => number * 96.0 / 2.54,
        "mm" => number * 96.0 / 25.4,
        "em" | "rem" => number * 16.0,
        "ch" => number * 8.0,
        "vw" => number * A4_CONTENT_WIDTH_PX / 100.0,
        _ => return None,
    };
    Some(px)
}
