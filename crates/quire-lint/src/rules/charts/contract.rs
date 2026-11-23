use super::{
    cartesian::{check_dense_labels, check_integer_axis_interval, check_line_end_labels},
    *,
};

pub(super) fn check_inner_title(
    engine: &mut Engine,
    block: &quire_protocol::Block,
    chart: &EchartsBlock,
    option: &Value,
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

pub(super) fn check_chart(
    engine: &mut Engine,
    block: &quire_protocol::Block,
    chart: &EchartsBlock,
    option: &Value,
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

    check_dense_labels(engine, block, option, index);
    check_line_end_labels(engine, block, option, index);
    check_integer_axis_interval(engine, block, option, index);
    let has_matrix = option.get("matrix").is_some_and(|value| !value.is_null());
    for (axis_key, axis_path) in [("xAxis", "xAxis"), ("yAxis", "yAxis")] {
        let grid_count = i64::try_from(axis_count(option, "grid")).unwrap_or(i64::MAX);
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
    for (series_index, series) in chart_series(option).into_iter().enumerate() {
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
            let count = i64::try_from(axis_count(option, axis_key)).unwrap_or(i64::MAX);
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
            let Some(count) = configured_axis_count(option, component) else {
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
    if !has_data_source(option) {
        engine.report(
            RuleId::EchartsDataMissing,
            format!("chart `{}` has no non-empty data source", block.id),
            "Supply series[*].data, series[*].links/edges, or a non-empty dataset.source so reviewers can verify the chart.",
            Some(block.id.clone()),
            Some(format!("blocks[{index}].option")),
        );
    }
}
