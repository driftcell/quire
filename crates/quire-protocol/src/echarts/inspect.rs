use super::*;

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

/// Return series entries whether `series` is an array or a single object.
pub fn series(option: &Value) -> Vec<&Value> {
    match option.get("series") {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(value @ Value::Object(_)) => vec![value],
        _ => Vec::new(),
    }
}

/// Treat an object/array JSON value as a list of entries.
pub fn entries(value: Option<&Value>) -> Vec<&Value> {
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
pub fn axis_count(option: &Value, key: &str) -> usize {
    configured_axis_count(option, key).unwrap_or({
        if matches!(key, "xAxis" | "yAxis" | "grid") {
            1
        } else {
            0
        }
    })
}

/// Number of explicitly configured coordinate-system instances, or `None` when absent.
pub fn configured_axis_count(option: &Value, key: &str) -> Option<usize> {
    match option.get(key) {
        Some(Value::Array(items)) if !items.is_empty() => Some(items.len()),
        Some(Value::Object(_)) => Some(1),
        _ => None,
    }
}

/// Whether a series type is valid ECharts 6.0 or a documented extension series.
pub fn is_valid_series_type(kind: &str) -> bool {
    SERIES_TYPES.contains(&kind)
}

/// Normalize chart title text for duplicate comparison.
pub(crate) fn normalized_title_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Whether an inner ECharts title repeats the outer block title.
pub fn inner_title_duplicates(entry: &Value, outer_title: &str) -> bool {
    let outer = normalized_title_text(outer_title.trim());
    if outer.is_empty() {
        return false;
    }
    ["text", "subtext"].iter().any(|key| {
        entry
            .get(key)
            .and_then(Value::as_str)
            .map(normalized_title_text)
            .is_some_and(|text| !text.is_empty() && text == outer)
    })
}

/// Center titles are KPI labels inside donut charts, not duplicated chart titles.
pub fn is_center_inner_title(entry: &Value) -> bool {
    matches!(
        entry
            .get("top")
            .and_then(Value::as_str)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("center" | "middle")
    )
}

/// A single inner title is allowed as a lightweight subplot label when it is
/// anchored away from the default top-center position or uses a small font.
pub fn is_positioned_or_light_inner_title(entry: &Value) -> bool {
    let positioned = ["left", "right", "bottom"].iter().any(|key| {
        entry
            .get(*key)
            .is_some_and(|value| !value.is_null() && value.as_str() != Some(""))
    });
    let light = entry
        .get("textStyle")
        .and_then(|style| style.get("fontSize"))
        .and_then(Value::as_f64)
        .is_some_and(|size| size <= 13.0);
    positioned || light
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
pub fn has_data_source(option: &Value) -> bool {
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
pub fn find_js_function_string(value: &Value, path: &str) -> Option<String> {
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
