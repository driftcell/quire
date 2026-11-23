//! Table layout risk checks: narrow CJK headers, empty/constant columns, A4 overflow and units.

use std::collections::BTreeMap;

use quire_protocol::{
    BlockKind, CellValue, ColumnDataType, LeafColumn, ReportDocument, TableBlock, TableShape,
    display_cell, validate_table,
};

use crate::{engine::Engine, registry::RuleId};

const A4_CONTENT_WIDTH_PX: f64 = 640.0;
const MIN_CELL_PADDING_PX: f64 = 24.0;
const COMPACT_CELL_PADDING_PX: f64 = 16.0;
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
    let cell_padding = if shape.leaves.len() >= 6 {
        COMPACT_CELL_PADDING_PX
    } else {
        MIN_CELL_PADDING_PX
    };
    for leaf in &shape.leaves {
        let header_units = display_width(&leaf.label).max(1) as f64;
        // Headers with <= 4 CJK chars (<= 8 width units) stay on one line in TableBlockView;
        // longer headers wrap across two balanced lines.
        let effective_header_units = if header_units <= 8.0 {
            header_units
        } else {
            (header_units * 0.6).ceil().max(6.0)
        };
        let body_units = max_body_width_units(table, &leaf.key, leaf.data_type) as f64;
        let body_has_text = table
            .rows
            .iter()
            .chain(table.summary_rows.iter())
            .filter_map(|row| row.get(&leaf.key))
            .any(|cell| !cell.display_text().trim().is_empty());
        let max_cjk_chars = table
            .rows
            .iter()
            .chain(table.summary_rows.iter())
            .filter_map(|row| row.get(&leaf.key))
            .map(|cell| cjk_char_count(&cell.display_text()))
            .max()
            .unwrap_or(0);
        let body_required = if leaf.data_type.is_numeric() {
            body_units * PX_PER_WIDTH_UNIT + cell_padding
        } else if max_cjk_chars >= 3 {
            // CJK entity names (3+ CJK characters) need 3–4 characters before they become a
            // one-character vertical strip, regardless of an explicit width.
            76.0
        } else if body_has_text {
            (body_units * PX_PER_WIDTH_UNIT + cell_padding).min(64.0)
        } else {
            0.0
        };
        let required =
            (effective_header_units * PX_PER_WIDTH_UNIT + cell_padding).max(body_required);
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
        if available + 0.5 >= required {
            continue;
        }
        engine.report(
            RuleId::TableColumnTooNarrow,
            format!(
                "table `{}` column `{}` is too narrow: explicit column width is {:.0}px but the single-line header or readable body needs about {:.0}px",
                block.id, leaf.key, available, required
            ),
            "删除全 `—` 空列、将列数精简至 8 列以内（最多不超过 9 列），或拆分为两张专题表；缩短长表头，不要用显式 column_widths 掩盖超宽。",
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
        "删除全 `—` 空列、将列数精简至 8 列以内（最多不超过 9 列），或拆分为两张专题表；缩短长表头，不要用显式 column_widths 掩盖超宽。",
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
    let col_count = shape.leaves.len();
    let cell_padding = if (6..=7).contains(&col_count) {
        COMPACT_CELL_PADDING_PX
    } else {
        MIN_CELL_PADDING_PX
    };
    let mut natural_width = 0.0;
    for leaf in &shape.leaves {
        let header = display_width(&leaf.label) as f64;
        let effective_header = if col_count <= 7 && header > 8.0 {
            (header * 0.65).ceil().max(6.0)
        } else {
            header
        };
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
        let effective_body = if col_count <= 5 && matches!(leaf.data_type, ColumnDataType::Text) {
            // In 2-to-5 column qualitative comparison tables, text columns wrap
            // naturally across 2–3 lines in A4 portrait CSS, whereas headers and
            // numeric columns stay on 1–2 lines.
            (body_average * 0.55).min(20.0)
        } else if col_count <= 7 && matches!(leaf.data_type, ColumnDataType::Text) {
            // In 6-to-7 column benchmark matrices, parenthetical annotations such as
            // `B+档（第14）` or `轻工B+、生工B+` wrap cleanly across 2 lines.
            (body_average * 0.65).min(16.0)
        } else {
            body_average
        };
        let widest = effective_header.max(effective_body);
        natural_width += widest * PX_PER_WIDTH_UNIT + cell_padding;
    }
    if col_count <= 9 && natural_width <= A4_CONTENT_WIDTH_PX * 1.18 {
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
        "删除全 `—` 空列、将列数精简至 8 列以内（最多不超过 9 列），或拆分为两张专题表；A4 竖版不得依赖横向滚动显示完整表格。",
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
    let first_key = shape.leaves.first().map(|leaf| leaf.key.as_str());
    for leaf in &shape.leaves {
        let mut values = Vec::new();
        for row in &table.rows {
            let Some(cell) = row.get(&leaf.key) else {
                continue;
            };
            let value = display_cell(cell, leaf.data_type);
            if is_placeholder_empty(&value) {
                continue;
            }
            // In row-oriented indicator tables, explicitly labelled percentage,
            // ratio, average or score rows (e.g. `领军人才数量省占比（%）`,
            // `国家自然科学基金年均立项（项）`, `产业契合度（分）`) legitimately coexist
            // with integer count rows in the same value columns.
            let explicit_ratio_or_rate_row = first_key.is_some_and(|first_key| {
                first_key != leaf.key
                    && row.get(first_key).is_some_and(|label| {
                        let label = label.display_text();
                        let has_ratio_label = [
                            "占比",
                            "比例",
                            "百分",
                            "增长率",
                            "费率",
                            "命中率",
                            "年均",
                            "均值",
                            "平均",
                            "折合",
                            "契合度",
                            "贡献度",
                            "集中度",
                            "覆盖率",
                            "转化率",
                            "就业率",
                            "深造率",
                            "升学率",
                            "留存率",
                            "比率",
                            "比值",
                            "系数",
                            "指数",
                            "得分",
                            "评分",
                            "分值",
                            "权重",
                            "强度",
                            "密度",
                        ]
                        .iter()
                        .any(|marker| label.contains(marker));
                        has_ratio_label
                            || (label.ends_with("率") && !label.ends_with("效率"))
                            || label.contains("率（")
                            || label.contains("率(")
                            || label.contains("比（")
                            || label.contains("比(")
                            || label.contains("（%）")
                            || label.contains("(%)")
                    })
            });
            if explicit_ratio_or_rate_row {
                continue;
            }
            values.push(value);
        }
        if values.len() < 3 {
            continue;
        }
        let Some((dominant, outlier)) = unit_inconsistency(&leaf.label, leaf.data_type, &values)
        else {
            continue;
        };
        engine.report(
            RuleId::TableNumericUnitInconsistent,
            format!(
                "table `{}` column `{}` (`{}`) mixes `{dominant}` cells with `{outlier}`",
                block.id, leaf.key, leaf.label
            ),
            "use one unit per column and put the unit in the column header or table note.",
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
        if ["平均", "均值", "占比", "比例", "率", "排名", "位次"]
            .iter()
            .any(|marker| text.contains(marker))
        {
            return false;
        }
        if text.starts_with("合计") || text.starts_with("总计") {
            return true;
        }
        // Permit short entity prefixes such as `三校合计`, but do not confuse
        // a metric's own time-window total with a whole-table summary row.
        ["合计", "总计"].iter().any(|marker| {
            text.strip_suffix(marker).is_some_and(|prefix| {
                let prefix = prefix.trim();
                !prefix.is_empty()
                    && prefix.chars().count() <= 6
                    && !prefix.contains(['(', '（', '[', '【'])
            })
        })
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
        .map(|cell| {
            let formatted = display_cell(cell, data_type);
            let trimmed = formatted.trim();
            if data_type.is_numeric() || is_short_unbreakable_token(trimmed) {
                display_width(trimmed)
            } else {
                display_width(trimmed).min(8)
            }
        })
        .max()
        .unwrap_or(0)
}

fn is_short_unbreakable_token(text: &str) -> bool {
    !text.is_empty()
        && text.chars().count() <= 10
        && text.chars().all(|ch| {
            ch.is_ascii_digit() || matches!(ch, '+' | '-' | '.' | ',' | '%' | '/' | '／' | '·')
        })
}

fn display_width(text: &str) -> usize {
    text.chars()
        .map(|ch| if is_wide_char(ch) { 2 } else { 1 })
        .sum()
}

fn cjk_char_count(text: &str) -> usize {
    text.chars().filter(|ch| is_wide_char(*ch)).count()
}

fn is_wide_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{1100}'..='\u{115f}'
            | '\u{2e80}'..='\u{a4cf}'
            | '\u{ac00}'..='\u{d7a3}'
            | '\u{f900}'..='\u{faff}'
            | '\u{fe10}'..='\u{fe6f}'
            | '\u{ff00}'..='\u{ff60}'
            | '\u{ffe0}'..='\u{ffe6}'
    )
}

fn parse_css_px(raw: &str) -> Option<f64> {
    let trimmed = raw.trim().to_ascii_lowercase();
    if let Some(px) = trimmed.strip_suffix("px") {
        let value: f64 = px.trim().parse().ok()?;
        return (value.is_finite() && value > 0.0).then_some(value);
    }
    if let Some(percent) = trimmed.strip_suffix('%') {
        let value: f64 = percent.trim().parse().ok()?;
        return (value.is_finite() && value > 0.0).then_some(A4_CONTENT_WIDTH_PX * (value / 100.0));
    }
    None
}

fn is_placeholder_empty(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.is_empty()
        || matches!(
            trimmed,
            "-" | "—" | "——" | "–" | "/" | "N/A" | "n/a" | "NA" | "无" | "暂无"
        )
}

fn parse_plain_number(raw: &str) -> Option<f64> {
    let trimmed = raw.trim().trim_end_matches('%').replace(',', "");
    let value: f64 = trimmed.parse().ok()?;
    value.is_finite().then_some(value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum UnitKind {
    Percent,
    Currency,
    Wan,
    Yi,
    PlainNumber,
}

impl UnitKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Percent => "%",
            Self::Currency => "currency",
            Self::Wan => "万",
            Self::Yi => "亿",
            Self::PlainNumber => "plain number",
        }
    }
}

fn unit_inconsistency(
    label: &str,
    data_type: ColumnDataType,
    values: &[String],
) -> Option<(&'static str, &'static str)> {
    let mut counts: BTreeMap<UnitKind, usize> = BTreeMap::new();
    for value in values {
        let Some(kind) = classify_unit(value) else {
            continue;
        };
        *counts.entry(kind).or_default() += 1;
    }
    if counts.len() <= 1 {
        return None;
    }
    let total: usize = counts.values().sum();
    let (&dominant_kind, &dominant_count) = counts.iter().max_by_key(|(_, count)| *count)?;
    if dominant_count * 2 <= total {
        return None;
    }
    let (&outlier_kind, _) = counts.iter().find(|(kind, _)| **kind != dominant_kind)?;
    let header_declares_percent =
        matches!(data_type, ColumnDataType::Percent) || label.contains('%') || label.contains("率");
    if dominant_kind == UnitKind::Percent
        && outlier_kind == UnitKind::PlainNumber
        && !header_declares_percent
    {
        return Some((dominant_kind.label(), outlier_kind.label()));
    }
    if dominant_kind != UnitKind::PlainNumber || header_declares_percent {
        return Some((dominant_kind.label(), outlier_kind.label()));
    }
    if matches!(
        outlier_kind,
        UnitKind::Percent | UnitKind::Currency | UnitKind::Wan | UnitKind::Yi
    ) {
        return Some((dominant_kind.label(), outlier_kind.label()));
    }
    None
}

fn classify_unit(raw: &str) -> Option<UnitKind> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.ends_with('%') && parse_plain_number(trimmed).is_some() {
        return Some(UnitKind::Percent);
    }
    if (trimmed.starts_with('$') || trimmed.starts_with('¥') || trimmed.starts_with('￥'))
        && parse_plain_number(&trimmed[trimmed.chars().next()?.len_utf8()..]).is_some()
    {
        return Some(UnitKind::Currency);
    }
    for (suffix, kind) in [
        ("万元", UnitKind::Wan),
        ("万", UnitKind::Wan),
        ("亿元", UnitKind::Yi),
        ("亿", UnitKind::Yi),
    ] {
        if let Some(number) = trimmed.strip_suffix(suffix)
            && parse_plain_number(number).is_some()
        {
            return Some(kind);
        }
    }
    if parse_plain_number(trimmed).is_some() {
        return Some(UnitKind::PlainNumber);
    }
    None
}
