use super::*;

pub(super) fn relation_tables(series: &[&Value]) -> Vec<ChartDataTable> {
    let mut tables = Vec::new();
    for (index, series) in series.iter().enumerate() {
        let Some(kind) = series_type(series) else {
            continue;
        };
        if !matches!(kind, "sankey" | "graph" | "chord") {
            continue;
        }
        let links = series
            .get("links")
            .or_else(|| series.get("edges"))
            .and_then(Value::as_array);
        let Some(links) = links else {
            continue;
        };
        let rows: Vec<Vec<String>> = links
            .iter()
            .filter_map(relation_row)
            .take(MAX_CHART_ROWS)
            .collect();
        if rows.is_empty() {
            continue;
        }
        let name = series_name(series, index);
        tables.push(ChartDataTable {
            title: format!("{name} ({kind} links)"),
            headers: vec!["Source".into(), "Target".into(), "Value".into()],
            rows,
        });
    }
    tables
}

fn relation_row(link: &Value) -> Option<Vec<String>> {
    if let Some(object) = link.as_object() {
        let source = object
            .get("source")
            .or_else(|| object.get("from"))
            .map(json_scalar_label)?;
        let target = object
            .get("target")
            .or_else(|| object.get("to"))
            .map(json_scalar_label)?;
        let value = object
            .get("value")
            .map(json_scalar_label)
            .unwrap_or_default();
        return Some(vec![source, target, value]);
    }
    let values = link.as_array()?;
    let source = values.first().map(json_scalar_label)?;
    let target = values.get(1).map(json_scalar_label)?;
    let value = values.get(2).map(json_scalar_label).unwrap_or_default();
    Some(vec![source, target, value])
}

pub(super) fn radar_tables(option: &Value, series: &[&Value]) -> Vec<ChartDataTable> {
    let radars = entries(option.get("radar"));
    let mut tables = Vec::new();
    for (index, series) in series.iter().enumerate() {
        if series_type(series) != Some("radar") {
            continue;
        }
        let Some(data) = series
            .get("data")
            .and_then(Value::as_array)
            .filter(|data| !data.is_empty())
        else {
            continue;
        };
        let radar_index = series
            .get("radarIndex")
            .and_then(Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(0);
        let radar = radars
            .get(radar_index)
            .copied()
            .or_else(|| radars.first().copied());
        let indicators: Vec<String> = radar
            .and_then(|radar| radar.get("indicator"))
            .and_then(Value::as_array)
            .map(|indicators| {
                indicators
                    .iter()
                    .enumerate()
                    .map(|(index, indicator)| {
                        indicator
                            .get("name")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("Indicator {}", index + 1))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let name = series_name(series, index);
        let (headers, rows) = if indicators.is_empty() {
            let mut body = Vec::new();
            for item in data.iter().take(MAX_CHART_ROWS) {
                let label = item
                    .get("name")
                    .map(json_scalar_label)
                    .unwrap_or_else(|| name.clone());
                let value = item.get("value").map(json_scalar_label).unwrap_or_default();
                body.push(vec![label, value]);
            }
            (vec!["Series".into(), "Value".into()], body)
        } else {
            let mut header = vec!["Series".to_string()];
            header.extend(indicators.iter().cloned());
            let mut body = Vec::new();
            for item in data.iter().take(MAX_CHART_ROWS) {
                let label = item
                    .get("name")
                    .map(json_scalar_label)
                    .unwrap_or_else(|| name.clone());
                let values = item
                    .get("value")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let mut row = vec![label];
                for index in 0..indicators.len() {
                    row.push(values.get(index).map(json_scalar_label).unwrap_or_default());
                }
                body.push(row);
            }
            (header, body)
        };
        if rows.is_empty() {
            continue;
        }
        tables.push(ChartDataTable {
            title: format!("{name} (radar)"),
            headers,
            rows,
        });
    }
    tables
}

pub(super) fn hierarchy_tables(series: &[&Value]) -> Vec<ChartDataTable> {
    let mut tables = Vec::new();
    for (index, series) in series.iter().enumerate() {
        let Some(kind) = series_type(series) else {
            continue;
        };
        if !matches!(kind, "treemap" | "sunburst" | "tree") {
            continue;
        }
        let nodes: Vec<&Value> = match series.get("data") {
            Some(Value::Array(items)) => items.iter().collect(),
            Some(value @ Value::Object(_)) => vec![value],
            _ => continue,
        };
        let mut rows = Vec::new();
        flatten_hierarchy(&nodes, "", 0, &mut rows);
        if rows.is_empty() {
            continue;
        }
        let name = series_name(series, index);
        let rows: Vec<Vec<String>> = rows
            .into_iter()
            .take(MAX_CHART_ROWS)
            .map(|(path, value)| vec![path, value])
            .collect();
        tables.push(ChartDataTable {
            title: format!("{name} ({kind})"),
            headers: vec!["Path".into(), "Value".into()],
            rows,
        });
    }
    tables
}

fn flatten_hierarchy(
    nodes: &[&Value],
    parent: &str,
    depth: usize,
    rows: &mut Vec<(String, String)>,
) {
    if depth > 32 {
        return;
    }
    for node in nodes {
        let name = node.get("name").and_then(Value::as_str).unwrap_or("item");
        let path = if parent.is_empty() {
            name.to_string()
        } else {
            format!("{parent} / {name}")
        };
        if let Some(value) = node.get("value").filter(|value| !value.is_null()) {
            rows.push((path.clone(), json_scalar_label(value)));
        }
        if let Some(children) = node.get("children").and_then(Value::as_array) {
            let child_refs: Vec<&Value> = children.iter().collect();
            flatten_hierarchy(&child_refs, &path, depth + 1, rows);
        }
    }
}

pub(super) fn cartesian_tables(option: &Value, series: &[&Value]) -> Vec<ChartDataTable> {
    let mut cartesian: BTreeMap<i64, Vec<(usize, &Value)>> = BTreeMap::new();
    let mut standalone = Vec::new();
    let mut multidimensional = Vec::new();
    for (index, series) in series.iter().enumerate() {
        let Some(kind) = series_type(series) else {
            continue;
        };
        if matches!(
            kind,
            "sankey" | "graph" | "chord" | "radar" | "treemap" | "sunburst" | "tree"
        ) {
            continue;
        }
        let Some(data) = series
            .get("data")
            .and_then(Value::as_array)
            .filter(|data| !data.is_empty())
        else {
            continue;
        };
        if is_multidimensional(kind, data) {
            multidimensional.push((index, *series));
        } else if is_simple_cartesian(kind, series) {
            let x_axis_index = series_axis_index(series, "xAxisIndex");
            cartesian
                .entry(x_axis_index)
                .or_default()
                .push((index, *series));
        } else {
            standalone.push((index, *series));
        }
    }
    let multi_panel =
        entries(option.get("xAxis")).len() > 1 || entries(option.get("grid")).len() > 1;
    let mut tables = Vec::new();
    for (x_axis_index, group) in cartesian {
        let y_axis_index = group
            .first()
            .map(|(_, series)| series_axis_index(series, "yAxisIndex"))
            .unwrap_or(0);
        let categories = chart_categories_for(option, x_axis_index, y_axis_index);
        if group.len() > 1 {
            if let Some(table) =
                combined_series_table(&categories, &group, multi_panel, x_axis_index)
            {
                tables.push(table);
            }
        } else if let Some((index, series)) = group.first()
            && let Some(table) = single_series_table(
                series,
                *index,
                &categories,
                multi_panel.then_some(x_axis_index),
            )
        {
            tables.push(table);
        }
    }
    for (index, series) in standalone {
        if let Some(table) = single_series_table(series, index, &[], None) {
            tables.push(table);
        }
    }
    for (index, series) in multidimensional {
        if let Some(table) = multidimensional_table(option, series, index) {
            tables.push(table);
        }
    }
    tables
}

fn single_series_table(
    series: &Value,
    index: usize,
    categories: &[String],
    panel_index: Option<i64>,
) -> Option<ChartDataTable> {
    let data = series.get("data").and_then(Value::as_array)?;
    let name = series_name(series, index);
    let kind = series_type(series).unwrap_or("chart");
    let mut rows = Vec::new();
    for (data_index, item) in data.iter().enumerate().take(MAX_CHART_ROWS) {
        if let Some((category, value)) = chart_point(item, categories.get(data_index), data_index) {
            rows.push(vec![category, value]);
        }
    }
    if rows.is_empty() {
        return None;
    }
    let mut title = format!("{name} ({kind})");
    if let Some(panel_index) = panel_index {
        title.push_str(&format!(" · panel {}", panel_index + 1));
    }
    Some(ChartDataTable {
        title,
        headers: vec!["Category".into(), "Value".into()],
        rows,
    })
}

fn combined_series_table(
    categories: &[String],
    series: &[(usize, &Value)],
    multi_panel: bool,
    x_axis_index: i64,
) -> Option<ChartDataTable> {
    let mut categories = categories.to_vec();
    if categories.is_empty()
        && let Some((_, first)) = series.first()
        && let Some(data) = first.get("data").and_then(Value::as_array)
    {
        categories = data
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                chart_point(item, None, index).map(|(category, _)| category)
            })
            .take(MAX_CHART_ROWS)
            .collect();
    }
    if categories.is_empty() {
        return None;
    }
    let names: Vec<String> = series
        .iter()
        .map(|(index, entry)| series_name(entry, *index))
        .collect();
    let kinds: Vec<&str> = series
        .iter()
        .map(|(_, entry)| series_type(entry).unwrap_or("chart"))
        .collect();
    let mut headers = vec!["Category".to_string()];
    headers.extend(names.iter().cloned());
    let mut rows = Vec::new();
    for (row_index, category) in categories.iter().enumerate().take(MAX_CHART_ROWS) {
        let mut row = vec![category.clone()];
        for (_, entry) in series {
            let value = entry
                .get("data")
                .and_then(Value::as_array)
                .and_then(|data| data.get(row_index))
                .and_then(|item| chart_point(item, None, row_index))
                .map(|(_, value)| value)
                .unwrap_or_default();
            row.push(value);
        }
        rows.push(row);
    }
    let mut title = format!("{} ({})", names.join(", "), kinds.join(", "));
    if multi_panel {
        title.push_str(&format!(" · panel {}", x_axis_index + 1));
    }
    Some(ChartDataTable {
        title,
        headers,
        rows,
    })
}

fn multidim_categories(option: &Value, series: &Value) -> Vec<String> {
    chart_categories_for(
        option,
        series_axis_index(series, "xAxisIndex"),
        series_axis_index(series, "yAxisIndex"),
    )
}

fn multidimensional_table(option: &Value, series: &Value, index: usize) -> Option<ChartDataTable> {
    let data = series
        .get("data")
        .and_then(Value::as_array)
        .filter(|data| !data.is_empty())?;
    let name = series_name(series, index);
    let kind = series_type(series).unwrap_or("chart");
    let dimensions: Vec<String> = match kind {
        "candlestick" => ["Open", "Close", "Low", "High"]
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        "boxplot" => ["Min", "Q1", "Median", "Q3", "Max"]
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        "heatmap" => ["X", "Y", "Value"]
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        _ => {
            let width = data
                .iter()
                .filter_map(Value::as_array)
                .map(Vec::len)
                .max()
                .unwrap_or(2)
                .max(2);
            (0..width)
                .map(|index| match index {
                    0 => "X".to_string(),
                    1 => "Y".to_string(),
                    other => format!("Value {}", other + 1),
                })
                .collect()
        }
    };
    let uses_category = matches!(kind, "candlestick" | "boxplot");
    let categories = multidim_categories(option, series);
    let mut headers = Vec::new();
    if uses_category {
        headers.push("Category".to_string());
    }
    headers.extend(dimensions.iter().cloned());
    let mut rows = Vec::new();
    for (data_index, item) in data.iter().enumerate().take(MAX_CHART_ROWS) {
        let values: Vec<String> = match item {
            Value::Array(values) => values.iter().map(json_scalar_label).collect(),
            Value::Object(object) => object
                .get("value")
                .and_then(Value::as_array)
                .map(|values| values.iter().map(json_scalar_label).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        if values.is_empty() {
            continue;
        }
        let mut row = Vec::new();
        if uses_category {
            row.push(
                categories
                    .get(data_index)
                    .cloned()
                    .unwrap_or_else(|| format!("#{}", data_index + 1)),
            );
        }
        for dimension_index in 0..dimensions.len() {
            row.push(values.get(dimension_index).cloned().unwrap_or_default());
        }
        rows.push(row);
    }
    if rows.is_empty() {
        return None;
    }
    Some(ChartDataTable {
        title: format!("{name} ({kind})"),
        headers,
        rows,
    })
}

fn is_multidimensional(kind: &str, data: &[Value]) -> bool {
    matches!(
        kind,
        "scatter" | "effectScatter" | "candlestick" | "boxplot" | "heatmap"
    ) && data.iter().any(Value::is_array)
}

fn is_simple_cartesian(kind: &str, series: &Value) -> bool {
    match series.get("coordinateSystem").and_then(Value::as_str) {
        Some("cartesian2d" | "cartesian3D") => true,
        Some(_) => false,
        None => matches!(
            kind,
            "line" | "bar" | "scatter" | "effectScatter" | "pictorialBar" | "custom"
        ),
    }
}

fn series_axis_index(series: &Value, field: &str) -> i64 {
    series
        .get(field)
        .and_then(Value::as_i64)
        .filter(|index| *index >= 0)
        .unwrap_or(0)
}

fn series_type(series: &Value) -> Option<&str> {
    series.get("type").and_then(Value::as_str)
}

fn series_name(series: &Value, index: usize) -> String {
    series
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("Series {}", index + 1))
}

fn chart_categories_for(option: &Value, x_axis_index: i64, y_axis_index: i64) -> Vec<String> {
    let x = axis_categories(option, "xAxis", x_axis_index);
    if !x.is_empty() {
        return x;
    }
    axis_categories(option, "yAxis", y_axis_index)
}

fn axis_categories(option: &Value, key: &str, index: i64) -> Vec<String> {
    let axis_index = usize::try_from(index).unwrap_or(0);
    entries(option.get(key))
        .get(axis_index)
        .and_then(|axis| axis.get("data"))
        .and_then(Value::as_array)
        .map(|data| data.iter().map(json_scalar_label).collect())
        .unwrap_or_default()
}

fn chart_point(
    item: &Value,
    fallback_category: Option<&String>,
    index: usize,
) -> Option<(String, String)> {
    match item {
        Value::Object(map) => {
            let category = map
                .get("name")
                .map(json_scalar_label)
                .or_else(|| fallback_category.cloned())
                .unwrap_or_else(|| format!("#{}", index + 1));
            let value = map.get("value").map(json_scalar_label).unwrap_or_default();
            Some((category, value))
        }
        Value::Array(values) => {
            let category = values
                .first()
                .map(json_scalar_label)
                .or_else(|| fallback_category.cloned())
                .unwrap_or_else(|| format!("#{}", index + 1));
            let value = if values.len() == 2 {
                values.get(1).map(json_scalar_label).unwrap_or_default()
            } else {
                values
                    .iter()
                    .skip(1)
                    .map(json_scalar_label)
                    .collect::<Vec<_>>()
                    .join(" / ")
            };
            Some((category, value))
        }
        scalar => {
            let category = fallback_category
                .cloned()
                .unwrap_or_else(|| format!("#{}", index + 1));
            Some((category, json_scalar_label(scalar)))
        }
    }
}

pub(super) fn json_scalar_label(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => {
            if let Some(integer) = number.as_i64() {
                integer.to_string()
            } else if let Some(float) = number.as_f64() {
                format_number(float)
            } else {
                number.to_string()
            }
        }
        Value::Bool(value) => value.to_string(),
        Value::Null => String::new(),
        other => {
            let raw = serde_json::to_string(other).unwrap_or_default();
            truncate_chars(&raw, 80)
        }
    }
}
