use super::*;

struct DenseLabelFinding {
    message: String,
    hint: &'static str,
    path: String,
}

pub(super) fn check_dense_labels(
    engine: &mut Engine,
    block: &quire_protocol::Block,
    option: &EchartsOptionView,
    index: usize,
) {
    let Some(finding) = dense_pie_labels(option, index, &block.id)
        .or_else(|| dense_x_axis_labels(option, index, &block.id))
    else {
        return;
    };
    engine.report(
        RuleId::EchartsDenseLabelsOverlap,
        finding.message,
        finding.hint,
        Some(block.id.clone()),
        Some(finding.path),
    );
}

fn dense_pie_labels(
    option: &EchartsOptionView,
    index: usize,
    block_id: &BlockId,
) -> Option<DenseLabelFinding> {
    let series_is_array = option.series.is_array;
    for (series_index, series) in option.series.iter().enumerate() {
        if series.kind.as_deref() != Some("pie") {
            continue;
        }
        let Some(data) = series.data.as_ref().and_then(Value::as_array) else {
            continue;
        };
        if data.len() <= 7 {
            continue;
        }
        let path = if series_is_array {
            format!("blocks[{index}].option.series[{series_index}].data")
        } else {
            format!("blocks[{index}].option.series.data")
        };
        return Some(DenseLabelFinding {
            message: format!(
                "chart `{block_id}` pie series has {} slices (> 7), which causes label crowding",
                data.len()
            ),
            hint: "In `series.data`, merge the smallest tail slices into '其他' (Other) so the pie has <= 7 slices, or switch to a horizontal bar chart; keep every retained category visible and do not drop data merely to satisfy the check.",
            path,
        });
    }
    None
}

fn dense_x_axis_labels(
    option: &EchartsOptionView,
    index: usize,
    block_id: &BlockId,
) -> Option<DenseLabelFinding> {
    let axis_is_array = option.x_axis.is_array;
    for (axis_index, axis) in option.x_axis.iter().enumerate() {
        let Some(data) = &axis.data else {
            continue;
        };
        if data.is_empty() {
            continue;
        }
        let category_axis = axis.kind.as_deref() == Some("category") || axis.kind.is_none();
        if !category_axis {
            continue;
        }
        let label_count = data.len();
        let total_chars = data
            .iter()
            .filter_map(axis_label_text)
            .map(|label| label.chars().count())
            .sum::<usize>();
        let average_chars = total_chars as f64 / label_count as f64;
        let dense = label_count >= 10 || (label_count >= 8 && average_chars >= 6.0);
        if !dense {
            continue;
        }
        let rotate_handled = axis
            .axis_label
            .as_ref()
            .and_then(|label| label.rotate.as_ref())
            .is_some_and(axis_label_handled);
        let interval_handled = axis
            .axis_label
            .as_ref()
            .and_then(|label| label.interval.as_ref())
            .is_some_and(axis_label_handled);
        if rotate_handled || interval_handled {
            continue;
        }
        let path = if axis_is_array {
            format!("blocks[{index}].option.xAxis[{axis_index}]")
        } else {
            format!("blocks[{index}].option.xAxis")
        };
        return Some(DenseLabelFinding {
            message: format!(
                "chart `{block_id}` xAxis has {label_count} category labels without rotation or interval"
            ),
            hint: "On `xAxis.axisLabel`, set `rotate: 30` (or `45`) or a non-zero `interval`, or move categories to `yAxis` as a horizontal bar; keep every category readable and do not hide labels merely to satisfy the check.",
            path,
        });
    }
    None
}

fn axis_label_text(value: &Value) -> Option<&str> {
    value
        .as_str()
        .or_else(|| value.get("value").and_then(Value::as_str))
}

pub(super) fn check_line_end_labels(
    engine: &mut Engine,
    block: &quire_protocol::Block,
    option: &EchartsOptionView,
    index: usize,
) {
    let top_level_overlap_guard = option
        .label_layout
        .as_ref()
        .and_then(|layout| layout.hide_overlap)
        == Some(true);
    let mut enabled_series = 0usize;
    let mut missing_text_border = false;
    let mut series_overlap_guard = false;
    for series in option.series.iter() {
        if series.kind.as_deref() != Some("line") {
            continue;
        }
        let Some(end_label) = &series.end_label else {
            continue;
        };
        if end_label.show != Some(true) {
            continue;
        }
        enabled_series = enabled_series.saturating_add(1);
        if end_label.text_border_width != Some(0.0) {
            missing_text_border = true;
        }
        if series
            .label_layout
            .as_ref()
            .and_then(|layout| layout.hide_overlap)
            == Some(true)
        {
            series_overlap_guard = true;
        }
    }
    let multiple_without_guard =
        enabled_series >= 2 && !top_level_overlap_guard && !series_overlap_guard;
    if enabled_series == 0 || (!missing_text_border && !multiple_without_guard) {
        return;
    }
    engine.report(
        RuleId::EchartsLineEndLabelStyle,
        format!(
            "chart `{}` enables line `endLabel` without `textBorderWidth: 0` or overlap guard",
            block.id
        ),
        "On each `line` series with `endLabel.show: true`, set `endLabel.textBorderWidth: 0` to prevent text ghosting on white backgrounds, and set `labelLayout: { hideOverlap: true }` (or rely on legend) when multiple series end closely.",
        Some(block.id.clone()),
        Some(format!("blocks[{index}].option.series")),
    );
}

#[derive(Debug, Clone, Copy)]
enum CartesianValueAxis {
    X,
    Y,
}

impl CartesianValueAxis {
    fn name(self) -> &'static str {
        match self {
            Self::X => "xAxis",
            Self::Y => "yAxis",
        }
    }

    fn series_index_field(self) -> &'static str {
        match self {
            Self::X => "xAxisIndex",
            Self::Y => "yAxisIndex",
        }
    }

    fn category_index_field(self) -> &'static str {
        match self {
            Self::X => "yAxisIndex",
            Self::Y => "xAxisIndex",
        }
    }
}

struct IntegerAxisFinding {
    orientation: CartesianValueAxis,
    axis_index: usize,
    max_value: f64,
}

pub(super) fn check_integer_axis_interval(
    engine: &mut Engine,
    block: &quire_protocol::Block,
    option: &Value,
    index: usize,
) {
    for finding in integer_axis_findings(option) {
        let axis_name = finding.orientation.name();
        let axis_path = if option.get(axis_name).is_some_and(Value::is_array) {
            format!("{axis_name}[{}]", finding.axis_index)
        } else {
            axis_name.to_string()
        };
        engine.report(
            RuleId::EchartsIntegerAxisMinInterval,
            format!(
                "chart `{}` plots small integer counts (max {}) without `minInterval: 1` on `{axis_path}`",
                block.id, finding.max_value
            ),
            format!(
                "On `{axis_path}`, set `\"minInterval\": 1` so ECharts does not render fractional axis ticks (like 0.5 or 2.4) for integer count data."
            ),
            Some(block.id.clone()),
            Some(format!("blocks[{index}].option.{axis_path}")),
        );
    }
}

fn integer_axis_findings(option: &Value) -> Vec<IntegerAxisFinding> {
    let series = chart_series(option);
    if series.is_empty()
        || option.get("polar").is_some_and(|value| !value.is_null())
        || option.get("radar").is_some_and(|value| !value.is_null())
        || series.iter().any(|entry| {
            matches!(
                entry.get("coordinateSystem").and_then(Value::as_str),
                Some("polar" | "radar")
            )
        })
    {
        return Vec::new();
    }

    let x_axes = entries(option.get("xAxis"));
    let y_axes = entries(option.get("yAxis"));
    let mut findings = Vec::new();
    findings.extend(find_integer_value_axes(
        &series,
        &y_axes,
        &x_axes,
        CartesianValueAxis::Y,
    ));
    findings.extend(find_integer_value_axes(
        &series,
        &x_axes,
        &y_axes,
        CartesianValueAxis::X,
    ));
    findings
}

fn find_integer_value_axes(
    series: &[&Value],
    value_axes: &[&Value],
    category_axes: &[&Value],
    orientation: CartesianValueAxis,
) -> Vec<IntegerAxisFinding> {
    let mut findings = Vec::new();
    for (axis_index, axis) in value_axes.iter().enumerate() {
        if is_category_axis(axis) || is_percentage_axis(axis) || axis_interval_is_handled(axis) {
            continue;
        }
        let Some(numbers) =
            integer_numbers_for_axis(series, axis_index, category_axes, orientation)
        else {
            continue;
        };
        if numbers.len() < 2
            || numbers
                .iter()
                .any(|value| *value < 0.0 || value.fract() != 0.0)
        {
            continue;
        }
        let max_value = numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if (1.0..=15.0).contains(&max_value) {
            findings.push(IntegerAxisFinding {
                orientation,
                axis_index,
                max_value,
            });
        }
    }
    findings
}

fn integer_numbers_for_axis(
    series: &[&Value],
    value_axis_index: usize,
    category_axes: &[&Value],
    orientation: CartesianValueAxis,
) -> Option<Vec<f64>> {
    let mut numbers = Vec::new();
    let mut matched = false;
    for entry in series.iter().filter(|entry| {
        matches!(
            entry.get("type").and_then(Value::as_str),
            Some("bar" | "line")
        )
    }) {
        if series_component_index(entry, orientation.series_index_field())? != value_axis_index {
            continue;
        }
        let category_index = series_component_index(entry, orientation.category_index_field())?;
        if !category_axes
            .get(category_index)
            .is_some_and(|axis| is_category_axis(axis))
        {
            return None;
        }
        matched = true;
        numbers.extend(simple_cartesian_series_numbers(entry)?);
    }
    matched.then_some(numbers)
}

fn series_component_index(series: &Value, field: &str) -> Option<usize> {
    match series.get(field) {
        None => Some(0),
        Some(value) => value.as_u64().and_then(|index| usize::try_from(index).ok()),
    }
}

fn simple_cartesian_series_numbers(series: &Value) -> Option<Vec<f64>> {
    let data = series.get("data")?.as_array()?;
    let mut numbers = Vec::new();
    for item in data {
        match item {
            Value::Number(number) => numbers.push(number.as_f64()?),
            Value::Object(object) => match object.get("value") {
                Some(Value::Number(number)) => numbers.push(number.as_f64()?),
                Some(Value::Null) => {}
                _ => return None,
            },
            Value::Null => {}
            _ => return None,
        }
    }
    Some(numbers)
}

fn is_percentage_axis(axis: &Value) -> bool {
    axis.get("name")
        .and_then(Value::as_str)
        .is_some_and(|name| {
            name.contains('%')
                || name.contains('％')
                || name.contains("占比")
                || name.contains('率')
        })
        || axis
            .get("axisLabel")
            .and_then(|label| label.get("formatter"))
            .and_then(Value::as_str)
            .is_some_and(|formatter| formatter.contains('%') || formatter.contains('％'))
}

fn is_category_axis(axis: &Value) -> bool {
    axis.get("type").and_then(Value::as_str) == Some("category")
        || axis.get("data").is_some_and(Value::is_array)
}

fn axis_interval_is_handled(axis: &Value) -> bool {
    axis.get("minInterval")
        .and_then(Value::as_f64)
        .is_some_and(is_positive_integer)
        || axis
            .get("interval")
            .and_then(Value::as_f64)
            .is_some_and(is_positive_integer)
}

fn is_positive_integer(value: f64) -> bool {
    value >= 1.0 && value.fract() == 0.0
}

fn axis_label_handled(value: &Value) -> bool {
    if let Some(number) = value.as_f64() {
        return number != 0.0;
    }
    if let Some(text) = value.as_str() {
        return text.eq_ignore_ascii_case("auto")
            || text.trim().parse::<f64>().is_ok_and(|number| number != 0.0);
    }
    false
}
