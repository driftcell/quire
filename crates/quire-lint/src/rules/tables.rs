//! Table layout risk checks: narrow CJK headers, empty/constant columns, A4 overflow and units.

use std::collections::{BTreeMap, BTreeSet};

use quire_protocol::{
    BlockKind, CellValue, ColumnDataType, LeafColumn, ReportDocument, TableBlock, TableShape,
    display_cell, validate_table,
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
        check_column_width(engine, block, table, &shape, index);
        check_empty_columns(engine, block, table, &shape, index);
        check_constant_columns(engine, block, table, &shape, index);
        check_a4_width(engine, block, table, &shape, index);
        check_numeric_units(engine, block, table, &shape, index);
        check_summary_row(engine, block, table, &shape, index);
    }
}

fn check_column_width(
    engine: &mut Engine<'_>,
    block: &quire_protocol::Block,
    table: &TableBlock,
    shape: &TableShape,
    index: usize,
) {
    if shape.leaves.is_empty() {
        return;
    }
    let widths = block.style.column_widths.as_ref();
    let mut widest_implicit: Option<(&str, &str, f64)> = None;
    let mut total_required = 0.0;
    let mut has_implicit = false;
    for leaf in &shape.leaves {
        let header_units = display_width(&leaf.label).max(1) as f64;
        let body_units = max_body_width_units(table, &leaf.key, leaf.data_type) as f64;
        let required_units = header_units.max(body_units);
        let required = required_units * PX_PER_WIDTH_UNIT + MIN_CELL_PADDING_PX;
        total_required += required;
        let explicit = widths
            .and_then(|widths| widths.get(&leaf.key))
            .and_then(|raw| parse_css_px(raw));
        let Some(available) = explicit else {
            has_implicit = true;
            if widest_implicit
                .as_ref()
                .is_none_or(|(_, _, width)| required > *width)
            {
                widest_implicit = Some((leaf.key.as_str(), leaf.label.as_str(), required));
            }
            continue;
        };
        let header_driven = has_cjk(&leaf.label) && header_units >= 6.0;
        let numeric_driven = leaf.data_type.is_numeric() && body_units > 0.0;
        if (!header_driven && !numeric_driven) || available + 0.5 >= required {
            continue;
        }
        engine.report(
            RuleId::TableColumnTooNarrow,
            format!(
                "table `{}` column `{}` is too narrow: explicit column width is {:.0}px but the header or widest row needs about {:.0}px",
                block.id, leaf.key, available, required
            ),
            "widen this column's explicit `width`/`column_widths` entry, shorten the header, or reduce the column count; numeric and summary cells must stay on one line.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].columns.{}", leaf.key)),
        );
    }

    if !has_implicit || total_required <= A4_CONTENT_WIDTH_PX + 0.5 {
        return;
    }
    let Some((key, label, required)) = widest_implicit else {
        return;
    };
    engine.report(
        RuleId::TableColumnTooNarrow,
        format!(
            "table `{}` needs about {:.0}px for {} columns, exceeding the 640px A4 content width; column `{}` (`{}`) needs about {:.0}px and will be squeezed",
            block.id,
            total_required,
            shape.leaves.len(),
            key,
            label,
            required
        ),
        "shorten long CJK headers, reduce the column count, or give the widest column an explicit width so Water-Filling can allocate enough space.",
        Some(block.id.clone()),
        Some(format!("blocks[{index}].columns.{key}")),
    );
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
        let row_count = table.rows.len() + table.summary_rows.len();
        let body_total: usize = table
            .rows
            .iter()
            .chain(table.summary_rows.iter())
            .filter_map(|row| row.get(&leaf.key))
            .map(|cell| display_width(&display_cell(cell, leaf.data_type)))
            .sum();
        let body_average = if row_count == 0 {
            0.0
        } else {
            body_total as f64 / row_count as f64
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

type TableRow = BTreeMap<String, CellValue>;

#[derive(Debug, Clone, Copy)]
enum SummaryRowLocation {
    Body(usize),
    Summary(usize),
}

struct SummaryTarget<'a> {
    row: &'a TableRow,
    location: SummaryRowLocation,
}

impl SummaryTarget<'_> {
    fn path(&self, block_index: usize, key: &str) -> String {
        match self.location {
            SummaryRowLocation::Body(row) => {
                format!("blocks[{block_index}].rows[{row}].{key}")
            }
            SummaryRowLocation::Summary(row) => {
                format!("blocks[{block_index}].summary_rows[{row}].{key}")
            }
        }
    }
}

fn check_summary_row(
    engine: &mut Engine<'_>,
    block: &quire_protocol::Block,
    table: &TableBlock,
    shape: &TableShape,
    index: usize,
) {
    let Some(first_leaf) = shape.leaves.first() else {
        return;
    };
    if table
        .note
        .as_deref()
        .is_some_and(|note| note.contains("四舍五入"))
    {
        return;
    }

    let body_summary_index = table.rows.last().and_then(|row| {
        is_total_summary_row(row, &first_leaf.key).then(|| table.rows.len().saturating_sub(1))
    });
    let mut targets = Vec::new();
    for (row_index, row) in table.summary_rows.iter().enumerate() {
        if is_total_summary_row(row, &first_leaf.key) {
            targets.push(SummaryTarget {
                row,
                location: SummaryRowLocation::Summary(row_index),
            });
        }
    }
    if targets.is_empty()
        && let Some(row_index) = body_summary_index
        && let Some(row) = table.rows.last()
    {
        targets.push(SummaryTarget {
            row,
            location: SummaryRowLocation::Body(row_index),
        });
    }
    if targets.is_empty() {
        return;
    }

    let detail_len = body_summary_index.unwrap_or(table.rows.len());
    let Some(detail_rows) = table.rows.get(..detail_len) else {
        return;
    };
    if detail_rows.len() < 2 {
        return;
    }

    for target in &targets {
        for leaf in shape.leaves.iter().skip(1) {
            if !is_additive_column(leaf) {
                continue;
            }
            let finding = single_decimal_mismatch(detail_rows, target.row, &leaf.key)
                .or_else(|| composite_integer_mismatch(detail_rows, target.row, &leaf.key));
            let Some((actual, expected)) = finding else {
                continue;
            };
            engine.report(
                RuleId::TableSummaryRowMismatch,
                format!(
                    "table `{}` summary row column `{}` is `{actual}`, but rows sum to `{expected}`",
                    block.id, leaf.key
                ),
                format!(
                    "In this Table via apply_operations (UpdateCell), update summary row column `{}` to `{expected}` (or verify the individual row figures above it).",
                    leaf.key
                ),
                Some(block.id.clone()),
                Some(target.path(index, &leaf.key)),
            );
        }
    }
}

fn is_total_summary_row(row: &TableRow, first_key: &str) -> bool {
    row.get(first_key).is_some_and(|cell| {
        let text = cell.display_text();
        let text = text.trim();
        (text.contains("合计") || text.contains("总计"))
            && !["平均", "均值", "占比", "比例", "率", "排名", "位次"]
                .iter()
                .any(|marker| text.contains(marker))
    })
}

fn is_additive_column(leaf: &LeafColumn) -> bool {
    leaf.data_type != ColumnDataType::Percent
        && ![
            "占比",
            "比例",
            "增长率",
            "率",
            "排名",
            "位次",
            "均值",
            "平均",
        ]
        .iter()
        .any(|marker| leaf.label.contains(marker))
}

#[derive(Debug, Clone, Copy)]
struct DecimalValue {
    units: i128,
    scale: u32,
}

fn single_decimal_mismatch(
    detail_rows: &[TableRow],
    summary_row: &TableRow,
    key: &str,
) -> Option<(String, String)> {
    let actual = parse_decimal_cell(&summary_row.get(key)?.display_text(), false)?;
    let mut values = Vec::with_capacity(detail_rows.len());
    for row in detail_rows {
        values.push(parse_decimal_cell(&row.get(key)?.display_text(), true)?);
    }
    let scale = values.iter().map(|value| value.scale).max().unwrap_or(0);
    let mut expected_units = 0i128;
    for value in values {
        expected_units = expected_units.checked_add(rescale_decimal(value, scale)?)?;
    }
    let expected = DecimalValue {
        units: expected_units,
        scale,
    };
    if decimal_values_match(actual, expected)? {
        None
    } else {
        Some((format_decimal(actual), format_decimal(expected)))
    }
}

fn parse_decimal_cell(text: &str, allow_placeholder: bool) -> Option<DecimalValue> {
    let trimmed = text.trim();
    if allow_placeholder && matches!(trimmed, "" | "-" | "—") {
        return Some(DecimalValue { units: 0, scale: 0 });
    }
    if trimmed.is_empty() {
        return None;
    }
    let normalized: String = trimmed
        .chars()
        .filter(|ch| !matches!(ch, ',' | '，'))
        .collect();
    let has_decimal_point = normalized.contains('.');
    let (whole, fraction) = normalized
        .split_once('.')
        .map_or((normalized.as_str(), ""), |(whole, fraction)| {
            (whole, fraction)
        });
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || (has_decimal_point && fraction.is_empty())
    {
        return None;
    }
    let mut digits = String::with_capacity(whole.len() + fraction.len());
    digits.push_str(whole);
    digits.push_str(fraction);
    Some(DecimalValue {
        units: digits.parse::<i128>().ok()?,
        scale: u32::try_from(fraction.len()).ok()?,
    })
}

fn rescale_decimal(value: DecimalValue, target_scale: u32) -> Option<i128> {
    let exponent = target_scale.checked_sub(value.scale)?;
    let factor = 10i128.checked_pow(exponent)?;
    value.units.checked_mul(factor)
}

fn decimal_values_match(actual: DecimalValue, expected: DecimalValue) -> Option<bool> {
    let common_scale = actual.scale.max(expected.scale);
    let actual_units = rescale_decimal(actual, common_scale)?;
    let expected_units = rescale_decimal(expected, common_scale)?;
    let difference = actual_units.abs_diff(expected_units);
    if difference == 0 {
        return Some(true);
    }

    // Allow half a unit at the precision displayed by the summary cell. Comparing twice the
    // difference avoids representing a fractional tolerance in the common fixed-point scale.
    let exponent = common_scale.checked_sub(actual.scale)?;
    let twice_tolerance = 10u128.checked_pow(exponent)?;
    Some(difference.checked_mul(2)? <= twice_tolerance)
}

fn format_decimal(value: DecimalValue) -> String {
    if value.scale == 0 {
        return value.units.to_string();
    }
    let digits = value.units.to_string();
    let scale = value.scale as usize;
    let split = digits.len().saturating_sub(scale);
    if split == 0 {
        return format!("0.{}{}", "0".repeat(scale - digits.len()), digits);
    }
    match (digits.get(..split), digits.get(split..)) {
        (Some(whole), Some(fraction)) => format!("{whole}.{fraction}"),
        _ => digits,
    }
}

fn composite_integer_mismatch(
    detail_rows: &[TableRow],
    summary_row: &TableRow,
    key: &str,
) -> Option<(String, String)> {
    ['/', '／', '·']
        .into_iter()
        .find_map(|separator| composite_with_separator(detail_rows, summary_row, key, separator))
}

fn composite_with_separator(
    detail_rows: &[TableRow],
    summary_row: &TableRow,
    key: &str,
    separator: char,
) -> Option<(String, String)> {
    let actual = parse_integer_parts(&summary_row.get(key)?.display_text(), separator)?;
    let width = actual.len();
    if !(2..=4).contains(&width) {
        return None;
    }
    let mut expected = vec![0i128; width];
    for row in detail_rows {
        let parts = parse_integer_parts(&row.get(key)?.display_text(), separator)?;
        if parts.len() != width {
            return None;
        }
        for (sum, part) in expected.iter_mut().zip(parts) {
            *sum = (*sum).checked_add(part)?;
        }
    }
    (expected != actual).then(|| {
        (
            join_integer_parts(&actual, separator),
            join_integer_parts(&expected, separator),
        )
    })
}

fn parse_integer_parts(text: &str, separator: char) -> Option<Vec<i128>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parts: Vec<&str> = trimmed.split(separator).map(str::trim).collect();
    if !(2..=4).contains(&parts.len()) {
        return None;
    }
    let mut values = Vec::with_capacity(parts.len());
    for part in parts {
        values.push(parse_integer_cell(part, false)?);
    }
    Some(values)
}

fn join_integer_parts(parts: &[i128], separator: char) -> String {
    let glue = if separator == '/' {
        format!(" {separator} ")
    } else {
        separator.to_string()
    };
    parts
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(&glue)
}

fn parse_integer_cell(text: &str, allow_placeholder: bool) -> Option<i128> {
    let trimmed = text.trim();
    if allow_placeholder && matches!(trimmed, "" | "-" | "—") {
        return Some(0);
    }
    if trimmed.is_empty() {
        return None;
    }
    let normalized: String = trimmed
        .chars()
        .filter(|ch| !matches!(ch, ',' | '，'))
        .collect();
    let value = normalized.parse::<i128>().ok()?;
    (value >= 0).then_some(value)
}

fn max_body_width_units(table: &TableBlock, key: &str, data_type: ColumnDataType) -> usize {
    table
        .rows
        .iter()
        .chain(table.summary_rows.iter())
        .filter_map(|row| row.get(key))
        .map(|cell| display_width(&display_cell(cell, data_type)))
        .max()
        .unwrap_or(0)
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
