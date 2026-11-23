use super::*;

pub fn compute_table_layout(
    block_id: &BlockId,
    table: &TableBlock,
    style: &crate::block::BlockStyle,
) -> Result<TableLayout, TableError> {
    let shape = validate_table(table)?;
    let header_depth = shape.max_depth;
    let mut header_rows = vec![Vec::new(); header_depth];
    for column in &table.columns {
        fill_header_rows(column, 0, header_depth, &[], &mut header_rows);
    }
    let widths = compute_widths(table, &shape, style);
    let leaf_columns: Vec<LayoutLeafColumn> = shape
        .leaves
        .into_iter()
        .map(|leaf| {
            let width = widths.get(leaf.index).copied().unwrap_or(0.0);
            LayoutLeafColumn {
                key: leaf.key,
                label: leaf.label,
                path: leaf.path,
                data_type: leaf.data_type,
                sizing: leaf.sizing,
                weight: leaf.weight,
                align: leaf.align,
                header_align: leaf.header_align,
                index: leaf.index,
                width_pct: width,
                width_css: format!("{}%", trim_number(width)),
            }
        })
        .collect();
    let total_width_pct = round2(leaf_columns.iter().map(|leaf| leaf.width_pct).sum());
    Ok(TableLayout {
        table_block_id: block_id.clone(),
        header_rows,
        leaf_columns,
        header_depth,
        total_width_pct,
    })
}

fn fill_header_rows(
    node: &ColumnNode,
    depth: usize,
    header_depth: usize,
    ancestors: &[String],
    rows: &mut [Vec<LayoutHeaderCell>],
) {
    let Some(row) = rows.get_mut(depth) else {
        return;
    };
    if node.is_leaf() {
        row.push(LayoutHeaderCell {
            key: node.key.clone().unwrap_or_default(),
            label: node.label.clone(),
            colspan: 1,
            rowspan: header_depth.saturating_sub(depth).max(1),
            data_type: node.effective_data_type(),
            align: node.effective_header_align(),
            is_group: false,
        });
        return;
    }
    let mut path = ancestors.to_vec();
    path.push(node.label.clone());
    row.push(LayoutHeaderCell {
        key: node
            .key
            .clone()
            .filter(|key| !key.trim().is_empty())
            .unwrap_or_else(|| format!("group-{}", slugify(&node.label))),
        label: node.label.clone(),
        colspan: node.leaf_count(),
        rowspan: 1,
        data_type: node.effective_data_type(),
        align: node.effective_header_align(),
        is_group: true,
    });
    for child in &node.children {
        fill_header_rows(child, depth + 1, header_depth, &path, rows);
    }
}

fn compute_widths(
    table: &TableBlock,
    shape: &crate::table::TableShape,
    style: &crate::block::BlockStyle,
) -> Vec<f64> {
    let mut explicit: Vec<Option<f64>> = vec![None; shape.leaves.len()];
    for leaf in &shape.leaves {
        let Some(slot) = explicit.get_mut(leaf.index) else {
            continue;
        };
        if let Some(raw) = style
            .column_widths
            .as_ref()
            .and_then(|widths| widths.get(&leaf.key))
            && let Some(percent) = parse_width_percent(raw)
        {
            *slot = Some(percent.clamp(0.0, 100.0));
        }
    }
    let explicit_total: f64 = explicit.iter().flatten().sum();
    let computed_indices: Vec<usize> = (0..shape.leaves.len())
        .filter(|index| explicit.get(*index).is_none_or(Option::is_none))
        .collect();
    let mut desired = vec![0.0; shape.leaves.len()];
    for leaf in &shape.leaves {
        let Some(slot) = desired.get_mut(leaf.index) else {
            continue;
        };
        if let Some(percent) = explicit.get(leaf.index).copied().flatten() {
            *slot = percent;
            continue;
        }
        let mut widths = vec![visual_width_multiline(&leaf.label)];
        for row in table.rows.iter().chain(table.summary_rows.iter()) {
            if let Some(value) = row.get(&leaf.key) {
                let mut text = display_cell(value, leaf.data_type);
                if leaf.data_type == ColumnDataType::Percent
                    && value.as_number().is_some()
                    && !text.ends_with('%')
                {
                    text.push('%');
                }
                widths.push(visual_width_multiline(&text));
            }
        }
        let p90 = percentile_90(&mut widths).clamp(0.0, 80.0);
        let weight = if leaf.weight.is_finite() {
            f64::from(leaf.weight).clamp(0.05, 100.0)
        } else {
            1.0
        };
        *slot = match leaf.sizing {
            ColumnSizing::Fit => (p90 + 2.0).max(4.0) * weight,
            ColumnSizing::Grow => 12.0 * weight,
        };
    }

    let mut explicit_total = explicit_total;
    if !computed_indices.is_empty() && explicit_total > 95.0 {
        let scale = 95.0 / explicit_total;
        for (index, value) in explicit.iter_mut().enumerate() {
            if let Some(percent) = value {
                *percent *= scale;
                if let Some(slot) = desired.get_mut(index) {
                    *slot = *percent;
                }
            }
        }
        explicit_total = 95.0;
    }
    if computed_indices.is_empty() {
        let total: f64 = desired.iter().sum();
        if total <= 0.0 {
            return vec![round2(100.0 / shape.leaves.len() as f64); shape.leaves.len()];
        }
        let mut widths: Vec<f64> = desired
            .iter()
            .map(|value| round2(value / total * 100.0))
            .collect();
        fix_rounding(&mut widths);
        return widths;
    }

    let remaining = (100.0 - explicit_total).max(0.0);
    let computed_total: f64 = computed_indices
        .iter()
        .map(|index| desired.get(*index).copied().unwrap_or_default().max(0.0001))
        .sum();
    let mut widths = desired
        .iter()
        .enumerate()
        .map(|(index, value)| {
            if explicit.get(index).copied().flatten().is_some() {
                round2(*value)
            } else if computed_total > 0.0 {
                round2(remaining * value.max(0.0001) / computed_total)
            } else {
                round2(remaining / computed_indices.len() as f64)
            }
        })
        .collect::<Vec<_>>();
    fix_rounding(&mut widths);
    widths
}

fn fix_rounding(widths: &mut [f64]) {
    let sum: f64 = widths.iter().sum();
    let residual = round2(100.0 - sum);
    if residual.abs() < 0.000_001 {
        return;
    }
    if let Some((index, _)) = widths
        .iter()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(right.1))
        && let Some(value) = widths.get_mut(index)
    {
        *value = round2(*value + residual).max(0.0);
    }
}

fn percentile_90(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|left, right| left.total_cmp(right));
    let rank = ((values.len() as f64) * 0.9).ceil() as usize;
    let index = rank.saturating_sub(1).min(values.len().saturating_sub(1));
    values.get(index).copied().unwrap_or_default()
}

fn parse_width_percent(raw: &str) -> Option<f64> {
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.is_empty() || trimmed.len() > 32 {
        return None;
    }
    if let Some(number) = trimmed.strip_suffix('%') {
        return number
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite());
    }
    let (number, unit) = split_number_unit(&trimmed)?;
    let value: f64 = number.parse().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let nominal_px = 960.0;
    let px = match unit {
        "" | "%" => return Some(value),
        "px" => value,
        "pt" => value * (96.0 / 72.0),
        "pc" => value * 16.0,
        "in" => value * 96.0,
        "cm" => value * (96.0 / 2.54),
        "mm" => value * (96.0 / 25.4),
        "em" | "rem" => value * 16.0,
        "ch" => value * 8.0,
        "vw" => value * 9.6,
        "vh" => value * 5.4,
        _ => return None,
    };
    Some(px / nominal_px * 100.0)
}

fn split_number_unit(input: &str) -> Option<(&str, &str)> {
    let unit_start = input
        .find(|ch: char| ch.is_ascii_alphabetic())
        .unwrap_or(input.len());
    let (number, unit) = input.split_at(unit_start);
    if number.is_empty()
        || !number.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
        || number.chars().filter(|ch| *ch == '.').count() > 1
    {
        return None;
    }
    Some((number, unit))
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn trim_number(value: f64) -> String {
    let mut rendered = format!("{value:.2}");
    while rendered.ends_with('0') {
        rendered.pop();
    }
    if rendered.ends_with('.') {
        rendered.pop();
    }
    if rendered.is_empty() || rendered == "-0" {
        "0".to_string()
    } else {
        rendered
    }
}

/// Return the set of table leaf keys for external validation helpers.
pub fn table_leaf_keys(layout: &TableLayout) -> BTreeSet<String> {
    layout
        .leaf_columns
        .iter()
        .map(|leaf| leaf.key.clone())
        .collect()
}

/// Convenience map for quick block lookups in frontend code.
pub fn layouts_by_key(document: &ReportDocument) -> BTreeMap<BlockId, TableLayout> {
    compute_document_meta(document)
        .table_layouts
        .into_iter()
        .collect()
}
