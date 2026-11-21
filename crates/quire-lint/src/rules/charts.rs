//! ECharts contract and safety checks.

use std::collections::BTreeMap;

use quire_protocol::{
    BlockId, BlockKind, EchartsBlock, ReportDocument, axis_count, chart_series,
    configured_axis_count, effective_chart_option, entries, find_js_function_string,
    has_data_source, inner_title_duplicates, is_center_inner_title,
    is_positioned_or_light_inner_title, is_valid_series_type,
};
use serde_json::Value;

use crate::{engine::Engine, registry::RuleId};

pub(crate) fn check(document: &ReportDocument, engine: &mut Engine<'_>) {
    for (index, block) in document.blocks.iter().enumerate() {
        let BlockKind::Echarts(chart) = &block.body else {
            continue;
        };
        check_inner_title(engine, block, chart, index);
        check_chart(engine, block, chart, index);
    }
    check_cross_chart_colors(document, engine);
}

fn check_inner_title(
    engine: &mut Engine<'_>,
    block: &quire_protocol::Block,
    chart: &EchartsBlock,
    index: usize,
) {
    let Some(outer_title) = chart
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
    else {
        return;
    };
    let option = effective_chart_option(chart);
    let Some(title) = option.get("title") else {
        return;
    };
    let is_array = title.is_array();
    let titles = title
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_else(|| std::slice::from_ref(title));
    let duplicate = titles
        .iter()
        .any(|entry| inner_title_duplicates(entry, outer_title));
    let allowed = is_array
        || titles
            .iter()
            .all(|entry| is_center_inner_title(entry) || is_positioned_or_light_inner_title(entry));
    if !duplicate && allowed {
        return;
    }
    let detail = if duplicate {
        "repeats its outer title inside `option.title`"
    } else {
        "has a top-aligned inner ECharts title that is not a positioned subplot label"
    };
    engine.report(
        RuleId::EchartsDuplicateInnerTitle,
        format!("chart `{}` {detail}", block.id),
        "Keep the main title in outer `title` only. For multi-panel charts, use an `option.title` array or light positioned subtitles (left/right/bottom or fontSize <= 13) that do not repeat the outer title.",
        Some(block.id.clone()),
        Some(format!("blocks[{index}].option.title")),
    );
}

fn check_chart(
    engine: &mut Engine<'_>,
    block: &quire_protocol::Block,
    chart: &EchartsBlock,
    index: usize,
) {
    if chart.fallback_summary.trim().is_empty() {
        engine.report(
            RuleId::EchartsFallbackEmpty,
            format!("chart `{}` has an empty fallback_summary", block.id),
            "provide a one-sentence numeric summary for non-JS agents and accessibility",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].fallback_summary")),
        );
    }
    if !(120..=2000).contains(&chart.height) {
        engine.report(
            RuleId::EchartsHeightRange,
            format!(
                "chart `{}` height {} is outside the recommended 120..=2000",
                block.id, chart.height
            ),
            "use a height between 120 and 2000 pixels",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].height")),
        );
    }
    if chart.option.is_null() {
        engine.report(
            RuleId::EchartsOptionMissing,
            format!("chart `{}` has no native ECharts `option`", block.id),
            "Provide the complete ECharts 6.0 option object; presets and mode switches are not supported.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].option")),
        );
    }
    for (field, value) in [
        ("option", Some(&chart.option)),
        ("option_patch", chart.option_patch.as_ref()),
    ] {
        if let Some(value) = value
            && !value.is_null()
            && !value.is_object()
        {
            engine.report(
                RuleId::EchartsOptionNotObject,
                format!("chart `{}` `{field}` must be a JSON object", block.id),
                "send an ECharts option object, for example {\"series\":[{\"data\":[1,2]}]}",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].{field}")),
            );
        }
    }
    for (field, value) in [
        ("option", Some(&chart.option)),
        ("option_patch", chart.option_patch.as_ref()),
    ] {
        let Some(value) = value else {
            continue;
        };
        if let Some(path) = find_js_function_string(value, field) {
            engine.report(
                RuleId::EchartsJsFunction,
                format!(
                    "chart `{}` contains a JavaScript callback string at `{path}`",
                    block.id
                ),
                "Remove JS callbacks. Use ECharts string templates instead, for example \"{b}: {c} ({d}%)\"; callbacks must live in frontend renderer code, not canonical JSON.",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].{path}")),
            );
        }
    }

    let option = effective_chart_option(chart);
    check_dense_labels(engine, block, &option, index);
    check_line_end_labels(engine, block, &option, index);
    check_integer_axis_interval(engine, block, &option, index);
    let has_matrix = option.get("matrix").is_some_and(|value| !value.is_null());
    for (axis_key, axis_path) in [("xAxis", "xAxis"), ("yAxis", "yAxis")] {
        let grid_count = i64::try_from(axis_count(&option, "grid")).unwrap_or(i64::MAX);
        for (axis_index, axis) in entries(option.get(axis_key)).iter().enumerate() {
            let Some(grid_index) = axis.get("gridIndex").and_then(Value::as_i64) else {
                continue;
            };
            if grid_index < 0 || grid_index >= grid_count {
                engine.report(
                    RuleId::EchartsIndexOutOfRange,
                    format!(
                        "chart `{}` {axis_key}[{axis_index}].gridIndex is {grid_index}, but only {grid_count} `grid` entr{} exist",
                        block.id,
                        if grid_count == 1 { "y" } else { "ies" }
                    ),
                    format!(
                        "Use a gridIndex in 0..={}. Bind each axis array entry to its matching `grid` and keep series xAxisIndex/yAxisIndex in range.",
                        grid_count.saturating_sub(1)
                    ),
                    Some(block.id.clone()),
                    Some(format!("blocks[{index}].{axis_path}[{axis_index}].gridIndex")),
                );
            }
        }
    }
    for (series_index, series) in chart_series(&option).into_iter().enumerate() {
        let kind = series.get("type").and_then(Value::as_str);
        match kind {
            Some(kind) if is_valid_series_type(kind) => {}
            Some(kind) => {
                engine.report(
                    RuleId::EchartsSeriesTypeInvalid,
                    format!(
                        "chart `{}` series[{series_index}] has unsupported type `{kind}`",
                        block.id
                    ),
                    "Use an ECharts 6.0 series type such as line, bar, pie, radar, sankey, chord, beeswarm or matrix, or a registered extension such as wordCloud/liquidFill/bar3D.",
                    Some(block.id.clone()),
                    Some(format!("blocks[{index}].series[{series_index}].type")),
                );
            }
            None => {
                engine.report(
                    RuleId::EchartsSeriesTypeInvalid,
                    format!(
                        "chart `{}` series[{series_index}] is missing a string `type`",
                        block.id
                    ),
                    "Every series needs an explicit ECharts 6.0 type, for example `\"type\":\"bar\"`.",
                    Some(block.id.clone()),
                    Some(format!("blocks[{index}].series[{series_index}].type")),
                );
            }
        }
        if series.get("coordinateSystem").and_then(Value::as_str) == Some("matrix") && !has_matrix {
            engine.report(
                RuleId::EchartsMatrixMissing,
                format!(
                    "chart `{}` series[{series_index}] uses the matrix coordinate system but has no top-level `matrix`",
                    block.id
                ),
                "Add the ECharts 6.0 `matrix` configuration (rows/columns/cells) or remove coordinateSystem=\"matrix\".",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].series[{series_index}].coordinateSystem")),
            );
        }
        for (field, axis_key) in [
            ("xAxisIndex", "xAxis"),
            ("yAxisIndex", "yAxis"),
            ("gridIndex", "grid"),
            ("datasetIndex", "dataset"),
        ] {
            let Some(raw_index) = series.get(field).and_then(Value::as_i64) else {
                continue;
            };
            let count = i64::try_from(axis_count(&option, axis_key)).unwrap_or(i64::MAX);
            if raw_index < 0 || raw_index >= count {
                engine.report(
                    RuleId::EchartsIndexOutOfRange,
                    format!(
                        "chart `{}` series[{series_index}].{field} is {raw_index}, but only {count} `{axis_key}` entr{} exist",
                        block.id,
                        if count == 1 { "y" } else { "ies" }
                    ),
                    format!(
                        "Use an index in 0..={}. Add another `{axis_key}` entry only when the chart truly needs it.",
                        count.saturating_sub(1)
                    ),
                    Some(block.id.clone()),
                    Some(format!("blocks[{index}].series[{series_index}].{field}")),
                );
            }
        }

        let coordinate_system = series.get("coordinateSystem").and_then(Value::as_str);
        let needs_radar = kind == Some("radar")
            || series.get("radarIndex").is_some()
            || coordinate_system == Some("radar");
        let needs_polar = series.get("polarIndex").is_some() || coordinate_system == Some("polar");
        for (component, field, needed) in [
            ("radar", "radarIndex", needs_radar),
            ("polar", "polarIndex", needs_polar),
        ] {
            if !needed {
                continue;
            }
            let Some(count) = configured_axis_count(&option, component) else {
                engine.report(
                    RuleId::EchartsCoordinateMissing,
                    format!(
                        "chart `{}` series[{series_index}] uses `{component}` coordinates but has no top-level `{component}`",
                        block.id
                    ),
                    format!(
                        "Add a `{component}` object/array to the option (for radar, include `indicator`), or remove the `{field}`/coordinateSystem usage."
                    ),
                    Some(block.id.clone()),
                    Some(format!("blocks[{index}].{component}")),
                );
                continue;
            };
            let raw_index = series.get(field).and_then(Value::as_i64).unwrap_or(0);
            let count = i64::try_from(count).unwrap_or(i64::MAX);
            if raw_index < 0 || raw_index >= count {
                engine.report(
                    RuleId::EchartsIndexOutOfRange,
                    format!(
                        "chart `{}` series[{series_index}].{field} is {raw_index}, but only {count} `{component}` entr{} exist",
                        block.id,
                        if count == 1 { "y" } else { "ies" }
                    ),
                    format!(
                        "Use an index in 0..={}, or add another `{component}` configuration.",
                        count.saturating_sub(1)
                    ),
                    Some(block.id.clone()),
                    Some(format!("blocks[{index}].series[{series_index}].{field}")),
                );
            }
        }
    }
    if !has_data_source(&option) {
        engine.report(
            RuleId::EchartsDataMissing,
            format!("chart `{}` has no non-empty data source", block.id),
            "Supply series[*].data, series[*].links/edges, or a non-empty dataset.source so reviewers can verify the chart.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].option")),
        );
    }
}

struct DenseLabelFinding {
    message: String,
    hint: &'static str,
    path: String,
}

fn check_dense_labels(
    engine: &mut Engine<'_>,
    block: &quire_protocol::Block,
    option: &Value,
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

fn dense_pie_labels(option: &Value, index: usize, block_id: &BlockId) -> Option<DenseLabelFinding> {
    let series_is_array = option.get("series").is_some_and(Value::is_array);
    for (series_index, series) in chart_series(option).into_iter().enumerate() {
        if series.get("type").and_then(Value::as_str) != Some("pie") {
            continue;
        }
        let Some(data) = series.get("data").and_then(Value::as_array) else {
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
    option: &Value,
    index: usize,
    block_id: &BlockId,
) -> Option<DenseLabelFinding> {
    let axis_is_array = option.get("xAxis").is_some_and(Value::is_array);
    for (axis_index, axis) in entries(option.get("xAxis")).into_iter().enumerate() {
        let Some(data) = axis.get("data").and_then(Value::as_array) else {
            continue;
        };
        if data.is_empty() {
            continue;
        }
        let category_axis = axis.get("type").and_then(Value::as_str) == Some("category")
            || axis.get("type").is_none();
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
        let axis_label = axis.get("axisLabel");
        let rotate_handled = axis_label
            .and_then(|label| label.get("rotate"))
            .is_some_and(axis_label_handled);
        let interval_handled = axis_label
            .and_then(|label| label.get("interval"))
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

fn check_line_end_labels(
    engine: &mut Engine<'_>,
    block: &quire_protocol::Block,
    option: &Value,
    index: usize,
) {
    let top_level_overlap_guard = option
        .get("labelLayout")
        .and_then(|layout| layout.get("hideOverlap"))
        .and_then(Value::as_bool)
        == Some(true);
    let mut enabled_series = 0usize;
    let mut missing_text_border = false;
    let mut series_overlap_guard = false;
    for series in chart_series(option) {
        if series.get("type").and_then(Value::as_str) != Some("line") {
            continue;
        }
        let Some(end_label) = series.get("endLabel") else {
            continue;
        };
        if end_label.get("show").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        enabled_series = enabled_series.saturating_add(1);
        if end_label.get("textBorderWidth").and_then(Value::as_f64) != Some(0.0) {
            missing_text_border = true;
        }
        if series
            .get("labelLayout")
            .and_then(|layout| layout.get("hideOverlap"))
            .and_then(Value::as_bool)
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

fn check_integer_axis_interval(
    engine: &mut Engine<'_>,
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

/// ECharts 6 default theme palette, used when a chart omits `option.color`.
const ECHARTS6_DEFAULT_PALETTE: [&str; 9] = [
    "#5070dd", "#b6d634", "#505372", "#ff994d", "#0ca8df", "#ffd10a", "#fb628b", "#785db0",
    "#3fbe95",
];

const IGNORED_ENTITY_NAMES: [&str; 18] = [
    "总计",
    "合计",
    "平均值",
    "均值",
    "占比",
    "其他",
    "小计",
    "总数",
    "总和",
    "平均",
    "total",
    "sum",
    "average",
    "avg",
    "mean",
    "share",
    "other",
    "others",
];

#[derive(Debug)]
struct ChartColorOccurrence {
    color: String,
    path: String,
}

enum SeriesColor {
    Missing,
    Invalid,
    Explicit(String, String),
}

fn check_cross_chart_colors(document: &ReportDocument, engine: &mut Engine<'_>) {
    let mut entity_baseline: BTreeMap<String, (String, BlockId)> = BTreeMap::new();
    let mut palette_baseline: Option<(Vec<String>, BlockId)> = None;

    for (block_index, block) in document.blocks.iter().enumerate() {
        let BlockKind::Echarts(chart) = &block.body else {
            continue;
        };
        let option = effective_chart_option(chart);
        let explicit = explicit_palette(&option);
        if let Some(palette) = &explicit {
            if let Some((baseline, baseline_block)) = &palette_baseline {
                if palette != baseline {
                    engine.report(
                        RuleId::EchartsPaletteInconsistent,
                        format!(
                            "chart `{}` explicitly configures a different `option.color` sequence than `{baseline_block}`",
                            block.id
                        ),
                        format!(
                            "chart `{}` uses a different `option.color` palette than `{baseline_block}`; unify the report-wide ECharts palette and apply it with `option_patch`",
                            block.id
                        ),
                        Some(block.id.clone()),
                        Some(format!("blocks[{block_index}].option.color")),
                    );
                }
            } else {
                palette_baseline = Some((palette.clone(), block.id.clone()));
            }
        }

        let palette = explicit.unwrap_or_else(default_palette);
        for (entity, occurrence) in chart_entity_colors(&option, &palette) {
            if is_ignored_entity(&entity) {
                continue;
            }
            if let Some((baseline_color, baseline_block)) = entity_baseline.get(&entity) {
                if baseline_color != &occurrence.color && baseline_block != &block.id {
                    engine.report(
                        RuleId::EchartsEntityColorInconsistent,
                        format!(
                            "chart `{}` colors entity `{entity}` as `{}`, but `{baseline_block}` first used `{baseline_color}`",
                            block.id, occurrence.color
                        ),
                        format!(
                            "entity `{entity}` uses `{baseline_color}` in `{baseline_block}` but `{}` in `{}`; align `itemStyle.color` or `option.color` via `option_patch`",
                            occurrence.color, block.id
                        ),
                        Some(block.id.clone()),
                        Some(format!("blocks[{block_index}]{}", occurrence.path)),
                    );
                }
            } else {
                entity_baseline.insert(entity, (occurrence.color, block.id.clone()));
            }
        }
    }
}

fn explicit_palette(option: &Value) -> Option<Vec<String>> {
    let colors = option.get("color")?.as_array()?;
    let palette = colors
        .iter()
        .filter_map(normalize_color)
        .collect::<Vec<_>>();
    (!palette.is_empty()).then_some(palette)
}

fn default_palette() -> Vec<String> {
    ECHARTS6_DEFAULT_PALETTE
        .iter()
        .map(|color| (*color).to_string())
        .collect()
}

fn chart_entity_colors(
    option: &Value,
    palette: &[String],
) -> BTreeMap<String, ChartColorOccurrence> {
    let mut entities = BTreeMap::new();
    for (series_index, series) in chart_series(option).into_iter().enumerate() {
        let kind = series
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let item_series = matches!(kind, "pie" | "funnel");
        let series_color = series_color(series, series_index);

        if !item_series && let Some(name) = entity_name(series) {
            match series_color {
                SeriesColor::Explicit(color, path) => {
                    entities.insert(name, ChartColorOccurrence { color, path });
                }
                SeriesColor::Missing => {
                    if let Some(color) = palette_color(palette, series_index) {
                        entities.insert(
                            name,
                            ChartColorOccurrence {
                                color,
                                path: format!(".series[{series_index}].itemStyle.color"),
                            },
                        );
                    }
                }
                SeriesColor::Invalid => {}
            }
        }

        let Some(data) = series.get("data").and_then(Value::as_array) else {
            continue;
        };
        for (item_index, item) in data.iter().enumerate() {
            let Some(name) = entity_name(item) else {
                continue;
            };
            let item_color = item.get("itemStyle").and_then(|style| style.get("color"));
            let normalized_item_color = item_color.and_then(normalize_color);
            let (color, path) = if let Some(color) = normalized_item_color {
                (
                    color,
                    format!(".series[{series_index}].data[{item_index}].itemStyle.color"),
                )
            } else if item_color.is_some() {
                // Gradient objects and other non-string colors are intentionally unsupported.
                continue;
            } else if item_series {
                if let Some(series_item_color) =
                    series.get("itemStyle").and_then(|style| style.get("color"))
                {
                    let Some(color) = normalize_color(series_item_color) else {
                        continue;
                    };
                    (color, format!(".series[{series_index}].itemStyle.color"))
                } else if let Some(color) = palette_color(palette, item_index) {
                    (color, format!(".series[{series_index}].data[{item_index}]"))
                } else {
                    continue;
                }
            } else {
                // Non-pie item colors are only meaningful when explicitly authored.
                continue;
            };
            entities.insert(name, ChartColorOccurrence { color, path });
        }
    }
    entities
}

fn series_color(series: &Value, series_index: usize) -> SeriesColor {
    for (key, path) in [
        ("itemStyle", "itemStyle.color"),
        ("lineStyle", "lineStyle.color"),
    ] {
        let Some(raw) = series.get(key).and_then(|style| style.get("color")) else {
            continue;
        };
        return match normalize_color(raw) {
            Some(color) => SeriesColor::Explicit(color, format!(".series[{series_index}].{path}")),
            None => SeriesColor::Invalid,
        };
    }
    SeriesColor::Missing
}

fn entity_name(value: &Value) -> Option<String> {
    value
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn palette_color(palette: &[String], index: usize) -> Option<String> {
    if palette.is_empty() {
        return None;
    }
    palette.get(index % palette.len()).cloned()
}

fn normalize_color(value: &Value) -> Option<String> {
    value.as_str().and_then(normalize_color_text)
}

fn normalize_color_text(raw: &str) -> Option<String> {
    let text = raw.trim().to_ascii_lowercase();
    if text.is_empty() {
        return None;
    }
    if let Some(hex) = text.strip_prefix('#')
        && hex.chars().all(|ch| ch.is_ascii_hexdigit())
        && matches!(hex.len(), 3 | 4)
    {
        let mut expanded = String::with_capacity(1 + hex.len() * 2);
        expanded.push('#');
        for ch in hex.chars() {
            expanded.push(ch);
            expanded.push(ch);
        }
        return Some(expanded);
    }
    Some(text)
}

fn is_ignored_entity(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lowered = trimmed.to_ascii_lowercase();
    if IGNORED_ENTITY_NAMES.contains(&lowered.as_str()) {
        return true;
    }
    let numeric = trimmed.strip_suffix('年').unwrap_or(trimmed).trim();
    !numeric.is_empty()
        && numeric
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | ',' | '-' | '+' | '%' | '％' | ' '))
}
