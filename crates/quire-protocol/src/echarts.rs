//! Shared ECharts 6.0 option inspection helpers.
//!
//! These helpers are intentionally renderer-agnostic: they power the linter, Markdown
//! projection and frontend dependency pre-computation from the same merged option view.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::{block::EchartsBlock, json::merge_optional_json};

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
    let mut value = chart
        .option
        .clone()
        .unwrap_or_else(|| Value::Object(Default::default()));
    if let Some(patch) = &chart.option_patch {
        value = merge_optional_json(Some(&value), patch);
    }
    value
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
pub(crate) fn axis_count(option: &Value, key: &str) -> usize {
    match option.get(key) {
        Some(Value::Array(items)) => items.len().max(1),
        Some(Value::Object(_)) => 1,
        _ => 1,
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
pub(crate) fn find_js_function_string(value: &Value, path: &str) -> Option<String> {
    match value {
        Value::String(text) if looks_like_js_function(text) => Some(path.to_string()),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .find_map(|(index, item)| find_js_function_string(item, &format!("{path}[{index}]"))),
        Value::Object(map) => map.iter().find_map(|(key, value)| {
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

/// Detect inline JavaScript callbacks while allowing ECharts string templates.
pub(crate) fn looks_like_js_function(text: &str) -> bool {
    if text.contains("=>") || text.contains("function(") {
        return true;
    }
    let compact: String = text.chars().filter(|ch| !ch.is_whitespace()).collect();
    compact.contains("function(") || compact.contains("=>")
}
