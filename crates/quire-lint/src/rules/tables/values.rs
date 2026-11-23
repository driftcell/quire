use super::{layout::is_placeholder_empty, *};

pub(super) fn check_numeric_units(
    engine: &mut Engine,
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

pub(super) fn check_summary_row(
    engine: &mut Engine,
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
