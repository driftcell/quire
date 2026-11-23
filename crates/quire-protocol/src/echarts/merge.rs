use super::*;

const ELEMENT_MERGE_ARRAY_KEYS: &[&str] = &[
    "series",
    "xAxis",
    "yAxis",
    "grid",
    "dataset",
    "radar",
    "polar",
    "visualMap",
    "dataZoom",
    "graphic",
];

const NUMERIC_AXIS_ONLY_KEYS: &[&str] = &[
    "min",
    "max",
    "interval",
    "minInterval",
    "maxInterval",
    "scale",
];

/// Deep-merge an ECharts `overlay` into `base`.
///
/// Object arrays under ECharts component keys (`series`, `xAxis`, `yAxis`, `grid`, etc.)
/// merge element-by-element when `overlay` is a partial array of objects (`overlay.len() <= base.len()`),
/// and transitioning an axis `type` to `"category"` automatically strips stale numeric-axis properties
/// (`min`, `max`, `interval`, `minInterval`, `maxInterval`, `scale`) not re-supplied in `overlay`.
pub fn merge_chart_option(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            if overlay_map.get("type").and_then(Value::as_str) == Some("category")
                && base_map.get("type").and_then(Value::as_str) != Some("category")
            {
                for stale_key in NUMERIC_AXIS_ONLY_KEYS {
                    if !overlay_map.contains_key(*stale_key) {
                        base_map.remove(*stale_key);
                    }
                }
            }
            for (key, overlay_value) in overlay_map {
                match base_map.get_mut(key) {
                    Some(base_value) => {
                        if ELEMENT_MERGE_ARRAY_KEYS.contains(&key.as_str()) {
                            merge_component_slot(base_value, overlay_value);
                        } else {
                            merge_chart_option(base_value, overlay_value);
                        }
                    }
                    None => {
                        base_map.insert(key.clone(), overlay_value.clone());
                    }
                }
            }
        }
        (base_slot, overlay_value) => {
            *base_slot = overlay_value.clone();
        }
    }
}

fn merge_component_slot(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        (Value::Array(base_items), Value::Array(overlay_items))
            if !overlay_items.is_empty()
                && overlay_items.len() <= base_items.len()
                && base_items.iter().all(Value::is_object)
                && overlay_items.iter().all(Value::is_object) =>
        {
            for (base_item, overlay_item) in base_items.iter_mut().zip(overlay_items.iter()) {
                merge_chart_option(base_item, overlay_item);
            }
        }
        (Value::Array(base_items), Value::Object(_))
            if base_items.len() == 1 && base_items.first().is_some_and(Value::is_object) =>
        {
            if let Some(first) = base_items.first_mut() {
                merge_chart_option(first, overlay);
            }
        }
        (base @ Value::Object(_), Value::Array(overlay_items))
            if overlay_items.len() == 1 && overlay_items.first().is_some_and(Value::is_object) =>
        {
            if let Some(first) = overlay_items.first() {
                merge_chart_option(base, first);
            }
        }
        (base_slot, overlay_value) => {
            merge_chart_option(base_slot, overlay_value);
        }
    }
}

/// Merge the base option with `option_patch`.
pub fn merged_option(chart: &EchartsBlock) -> Value {
    let mut value = chart.option.clone();
    if let Some(patch) = &chart.option_patch {
        merge_chart_option(&mut value, patch);
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
pub fn merge_chart_option_patch(base: Option<&Value>, patch: &Value) -> Value {
    let mut merged = base
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));
    merge_chart_option(&mut merged, patch);
    merged
}
