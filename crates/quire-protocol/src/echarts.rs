//! Shared ECharts 6.0 option inspection helpers.
//!
//! These helpers are intentionally renderer-agnostic: they power the linter, Markdown
//! projection and frontend dependency pre-computation from the same merged option view.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    block::{EchartsBlock, format_number},
    json::merge_optional_json,
    text::truncate_chars,
};

/// Series types supported by ECharts 6.0 plus the common extension series.
pub(crate) const SERIES_TYPES: &[&str] = &[
    "line",
    "bar",
    "pie",
    "scatter",
    "effectScatter",
    "radar",
    "tree",
    "treemap",
    "sunburst",
    "boxplot",
    "candlestick",
    "heatmap",
    "map",
    "parallel",
    "lines",
    "graph",
    "sankey",
    "funnel",
    "gauge",
    "pictorialBar",
    "themeRiver",
    "custom",
    "chord",
    "beeswarm",
    "wordCloud",
    "liquidFill",
    "bar3D",
    "line3D",
    "lines3D",
    "scatter3D",
    "surface",
    "map3D",
    "geo3D",
    "globe",
    "globe3D",
    "graphGL",
    "linesGL",
    "scatterGL",
    "flowGL",
];

/// Merge the base option with `option_patch`.
pub(crate) fn merged_option(chart: &EchartsBlock) -> Value {
    let mut value = chart.option.clone();
    if let Some(patch) = &chart.option_patch {
        value = merge_optional_json(Some(&value), patch);
    }
    value
}

/// Resolve the effective native ECharts option for a chart block.
///
/// The returned value is the complete `option` deep-merged with `option_patch`. Frontends and
/// static validators share this projection so previews never diverge from the committed chart.
pub fn effective_chart_option(chart: &EchartsBlock) -> Value {
    merged_option(chart)
}

/// Deep-merge an incremental ECharts `option_patch` onto an optional base patch.
///
/// Arrays replace arrays and objects merge recursively, matching the canonical protocol merge.
pub fn merge_chart_option_patch(base: Option<&Value>, patch: &Value) -> Value {
    merge_optional_json(base, patch)
}

/// Return series entries whether `series` is an array or a single object.
pub(crate) fn series(option: &Value) -> Vec<&Value> {
    match option.get("series") {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(value @ Value::Object(_)) => vec![value],
        _ => Vec::new(),
    }
}

/// Treat an object/array JSON value as a list of entries.
pub(crate) fn entries(value: Option<&Value>) -> Vec<&Value> {
    match value {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(value @ Value::Object(_)) => vec![value],
        _ => Vec::new(),
    }
}

/// Number of selectable coordinate-system instances for an option key.
///
/// Cartesian axes and grids have an implicit default instance, so they may be omitted.
/// Radar/polar have no implicit component and report zero when absent; callers can use
/// [`configured_axis_count`] to distinguish "absent" from "one configured instance".
pub(crate) fn axis_count(option: &Value, key: &str) -> usize {
    configured_axis_count(option, key).unwrap_or({
        if matches!(key, "xAxis" | "yAxis" | "grid") {
            1
        } else {
            0
        }
    })
}

/// Number of explicitly configured coordinate-system instances, or `None` when absent.
pub(crate) fn configured_axis_count(option: &Value, key: &str) -> Option<usize> {
    match option.get(key) {
        Some(Value::Array(items)) if !items.is_empty() => Some(items.len()),
        Some(Value::Object(_)) => Some(1),
        _ => None,
    }
}

/// Whether a series type is valid ECharts 6.0 or a documented extension series.
pub(crate) fn is_valid_series_type(kind: &str) -> bool {
    SERIES_TYPES.contains(&kind)
}

/// Whether a series type needs an extra ECharts extension registration.
pub(crate) fn is_extension_series_type(kind: &str) -> bool {
    let lower = kind.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "wordcloud" | "liquidfill" | "surface" | "globe"
    ) || lower.ends_with("3d")
        || lower.ends_with("gl")
        || lower.ends_with("-gl")
}

/// Whether the merged option contains any non-empty data source.
pub(crate) fn has_data_source(option: &Value) -> bool {
    if series(option).iter().any(|entry| {
        ["data", "links", "edges"]
            .iter()
            .any(|key| non_empty_container(entry.get(key)))
    }) {
        return true;
    }
    entries(option.get("dataset"))
        .iter()
        .any(|dataset| non_empty_container(dataset.get("source")))
}

fn non_empty_container(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
        Some(Value::String(text)) => !text.trim().is_empty(),
        Some(Value::Null) | None => false,
        Some(_) => true,
    }
}

/// Map names required by `geo` and `series[type=map]`.
pub(crate) fn required_maps(option: &Value) -> BTreeSet<String> {
    let mut maps = BTreeSet::new();
    for geo in entries(option.get("geo")) {
        if let Some(name) = geo
            .get("map")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            maps.insert(name.to_string());
        }
    }
    for series in series(option) {
        if !matches!(
            series.get("type").and_then(Value::as_str),
            Some("map" | "map3D")
        ) {
            continue;
        }
        if let Some(name) = series
            .get("map")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            maps.insert(name.to_string());
        }
    }
    maps
}

/// Extension series types that must be registered before rendering.
pub(crate) fn required_extensions(option: &Value) -> BTreeSet<String> {
    series(option)
        .into_iter()
        .filter_map(|series| series.get("type").and_then(Value::as_str))
        .filter(|kind| is_extension_series_type(kind))
        .map(str::to_string)
        .collect()
}

/// Recursively find the first JavaScript callback string. Returns its JSON-ish path.
///
/// Data payloads (dataset sources, series data/links/edges/nodes, axis categories, names and
/// display text) are skipped: they are legitimate business strings and never execute as code.
pub(crate) fn find_js_function_string(value: &Value, path: &str) -> Option<String> {
    match value {
        Value::String(text) if looks_like_js_function(text) => Some(path.to_string()),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .find_map(|(index, item)| find_js_function_string(item, &format!("{path}[{index}]"))),
        Value::Object(map) => map.iter().find_map(|(key, value)| {
            if is_data_payload_key(key) {
                return None;
            }
            let child = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            find_js_function_string(value, &child)
        }),
        _ => None,
    }
}

fn is_data_payload_key(key: &str) -> bool {
    matches!(
        key,
        "data"
            | "source"
            | "links"
            | "edges"
            | "nodes"
            | "name"
            | "text"
            | "subtext"
            | "dimensions"
            | "encode"
            | "id"
            | "type"
            | "map"
    )
}

/// Detect inline JavaScript callbacks while allowing ECharts string templates and prose.
pub(crate) fn looks_like_js_function(text: &str) -> bool {
    let compact: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
    if compact.contains("function(") {
        return true;
    }
    contains_arrow_function(text)
}

fn contains_arrow_function(text: &str) -> bool {
    let mut remaining = text;
    while let Some((left, right)) = remaining.split_once("=>") {
        if arrow_left_is_function_params(left, right) {
            return true;
        }
        remaining = right;
    }
    false
}

fn arrow_left_is_function_params(left: &str, right: &str) -> bool {
    let left = left.trim_end();
    if left.ends_with(')') {
        return true;
    }
    let Some(token) = left
        .rsplit(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$')))
        .next()
    else {
        return false;
    };
    if token.is_empty() {
        return false;
    }
    let head = left.strip_suffix(token).unwrap_or_default().trim_end();
    let boundary_ok = head.is_empty()
        || head.ends_with([
            '(', '[', '{', ',', ';', ':', '=', '?', '!', '&', '|', '+', '-', '*', '/', '%', '<',
            '>',
        ]);
    if !boundary_ok {
        return false;
    }
    // Single-character tokens are ambiguous. Treat lowercase identifiers (`x => x`) as JS and
    // the canonical uppercase prose case (`A => B`) as business text unless the body is JS-like.
    if token.chars().count() > 1 || right_looks_like_js_expression(right) {
        return true;
    }
    token
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || matches!(ch, '_' | '$'))
}

fn right_looks_like_js_expression(right: &str) -> bool {
    let right = right.trim_start();
    !right.is_empty()
        && (right.contains([
            '(', '[', '{', '.', ';', '+', '-', '*', '/', '%', '!', '?', ':', '=', '&', '|', '<',
            '>',
        ]) || right.contains("return"))
}

/// Maximum rows emitted for one chart data table.
pub(crate) const MAX_CHART_ROWS: usize = 200;

/// Renderer-agnostic tabular projection of an ECharts option.
///
/// The same tables power Markdown review and the frontend `computed.chart_tables` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChartDataTable {
    pub title: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Extract all structured data tables from a merged ECharts option.
pub fn chart_data_tables(option: &Value) -> Vec<ChartDataTable> {
    let series_entries = series(option);
    let mut tables = dataset_tables(option);
    tables.extend(relation_tables(&series_entries));
    tables.extend(radar_tables(option, &series_entries));
    tables.extend(hierarchy_tables(&series_entries));
    tables.extend(cartesian_tables(option, &series_entries));
    tables
}

fn dataset_tables(option: &Value) -> Vec<ChartDataTable> {
    let mut tables = Vec::new();
    for (index, dataset) in entries(option.get("dataset")).iter().enumerate() {
        let Some(source) = dataset.get("source") else {
            continue;
        };
        let rows: Vec<&Value> = match source {
            Value::Array(items) => items.iter().collect(),
            Value::Object(_) => vec![source],
            _ => continue,
        };
        let Some((headers, mut body)) = dataset_shape(&rows) else {
            continue;
        };
        if headers.is_empty() || body.is_empty() {
            continue;
        }
        body.truncate(MAX_CHART_ROWS);
        let label = dataset
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("Dataset {}", index + 1));
        tables.push(ChartDataTable {
            title: format!("{label} (dataset)"),
            headers,
            rows: body,
        });
    }
    tables
}

fn dataset_shape(rows: &[&Value]) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    let object_rows: Vec<&serde_json::Map<String, Value>> =
        rows.iter().filter_map(|row| row.as_object()).collect();
    if object_rows.len() == rows.len() && !object_rows.is_empty() {
        let mut header: Vec<String> = Vec::new();
        for row in &object_rows {
            for key in row.keys() {
                if !header.contains(key) {
                    header.push(key.clone());
                }
            }
        }
        let body: Vec<Vec<String>> = object_rows
            .iter()
            .map(|row| {
                header
                    .iter()
                    .map(|key| row.get(key).map(json_scalar_label).unwrap_or_default())
                    .collect()
            })
            .collect();
        return Some((header, body));
    }
    let array_rows: Vec<&Vec<Value>> = rows.iter().filter_map(|row| row.as_array()).collect();
    if array_rows.len() != rows.len() || array_rows.is_empty() {
        return None;
    }
    let has_header = rows.len() > 1
        && rows.first().is_some_and(|row| {
            row.as_array()
                .is_some_and(|cells| !cells.is_empty() && cells.iter().all(Value::is_string))
        });
    let width = array_rows.iter().map(|row| row.len()).max().unwrap_or(0);
    if width == 0 {
        return None;
    }
    let header: Vec<String> = if has_header {
        rows.first()
            .and_then(|row| row.as_array())
            .map(|cells| cells.iter().map(json_scalar_label).collect())
            .unwrap_or_default()
    } else {
        (1..=width)
            .map(|column| format!("Column {column}"))
            .collect()
    };
    let body: Vec<Vec<String>> = rows
        .iter()
        .skip(usize::from(has_header))
        .filter_map(|row| row.as_array())
        .map(|cells| {
            (0..header.len())
                .map(|index| cells.get(index).map(json_scalar_label).unwrap_or_default())
                .collect()
        })
        .collect();
    Some((header, body))
}

fn relation_tables(series: &[&Value]) -> Vec<ChartDataTable> {
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

fn radar_tables(option: &Value, series: &[&Value]) -> Vec<ChartDataTable> {
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

fn hierarchy_tables(series: &[&Value]) -> Vec<ChartDataTable> {
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

fn cartesian_tables(option: &Value, series: &[&Value]) -> Vec<ChartDataTable> {
    let mut simple = Vec::new();
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
        } else {
            simple.push((index, *series));
        }
    }
    let mut tables = Vec::new();
    if simple.len() > 1 {
        if let Some(table) = combined_series_table(option, &simple) {
            tables.push(table);
        }
    } else if let Some((index, series)) = simple.first()
        && let Some(table) = single_series_table(option, series, *index)
    {
        tables.push(table);
    }
    for (index, series) in multidimensional {
        if let Some(table) = multidimensional_table(option, series, index) {
            tables.push(table);
        }
    }
    tables
}

fn single_series_table(option: &Value, series: &Value, index: usize) -> Option<ChartDataTable> {
    let data = series.get("data").and_then(Value::as_array)?;
    let name = series_name(series, index);
    let kind = series_type(series).unwrap_or("chart");
    let categories = chart_categories(option);
    let mut rows = Vec::new();
    for (data_index, item) in data.iter().enumerate().take(MAX_CHART_ROWS) {
        if let Some((category, value)) = chart_point(item, categories.get(data_index), data_index) {
            rows.push(vec![category, value]);
        }
    }
    if rows.is_empty() {
        return None;
    }
    Some(ChartDataTable {
        title: format!("{name} ({kind})"),
        headers: vec!["Category".into(), "Value".into()],
        rows,
    })
}

fn combined_series_table(option: &Value, series: &[(usize, &Value)]) -> Option<ChartDataTable> {
    let mut categories = chart_categories(option);
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
    Some(ChartDataTable {
        title: format!("{} ({})", names.join(", "), kinds.join(", ")),
        headers,
        rows,
    })
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
    let categories = chart_categories(option);
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

fn chart_categories(option: &Value) -> Vec<String> {
    for key in ["xAxis", "yAxis"] {
        let Some(axis) = option.get(key) else {
            continue;
        };
        let axes = axis
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_else(|| std::slice::from_ref(axis));
        for axis in axes {
            let Some(data) = axis.get("data").and_then(Value::as_array) else {
                continue;
            };
            if !data.is_empty() {
                return data.iter().map(json_scalar_label).collect();
            }
        }
    }
    Vec::new()
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

fn json_scalar_label(value: &Value) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::EchartsBlock;

    fn chart(option: Value) -> EchartsBlock {
        EchartsBlock {
            title: None,
            height: 360,
            option,
            option_patch: None,
            fallback_summary: "Summary".into(),
            numbering: None,
            note: None,
        }
    }

    #[test]
    fn effective_option_merges_the_incremental_patch() {
        let base = chart(serde_json::json!({
            "series": [{"type": "bar", "data": [1]}],
            "title": {"text": "Base"}
        }));
        let merged = merge_chart_option_patch(
            Some(&base.option),
            &serde_json::json!({
                "series": [{"type": "bar", "data": [2, 3]}],
                "title": {"subtext": "Patched"}
            }),
        );
        let mut patched = base;
        patched.option_patch = Some(merged);

        let effective = effective_chart_option(&patched);
        assert_eq!(effective["title"]["text"], "Base");
        assert_eq!(effective["title"]["subtext"], "Patched");
        assert_eq!(effective["series"][0]["data"], serde_json::json!([2, 3]));
    }
}
