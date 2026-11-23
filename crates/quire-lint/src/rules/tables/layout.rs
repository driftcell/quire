use super::*;

const A4_CONTENT_WIDTH_PX: f64 = 640.0;
const MIN_CELL_PADDING_PX: f64 = 24.0;
const COMPACT_CELL_PADDING_PX: f64 = 16.0;
const PX_PER_WIDTH_UNIT: f64 = 8.0;

pub(super) fn check_column_width(
    engine: &mut Engine,
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

pub(super) fn check_empty_columns(
    engine: &mut Engine,
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

pub(super) fn check_constant_columns(
    engine: &mut Engine,
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

pub(super) fn check_a4_width(
    engine: &mut Engine,
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

pub(super) fn is_placeholder_empty(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.is_empty()
        || matches!(
            trimmed,
            "-" | "—" | "——" | "–" | "/" | "N/A" | "n/a" | "NA" | "无" | "暂无"
        )
}
