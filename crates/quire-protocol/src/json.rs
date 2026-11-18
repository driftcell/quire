//! Shared JSON merge helpers for ECharts option patches and declarative block patches.

use serde_json::Value;

/// Deep-merge JSON objects. Arrays and scalars in `overlay` replace the base value.
pub fn deep_merge(base: &mut Value, overlay: &Value) {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            for (key, overlay_value) in overlay_map {
                match base_map.get_mut(key) {
                    Some(base_value) => deep_merge(base_value, overlay_value),
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

/// Merge `overlay` into an optional JSON object, creating an object when absent.
pub fn merge_optional_json(base: Option<&Value>, overlay: &Value) -> Value {
    let mut merged = base
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));
    deep_merge(&mut merged, overlay);
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deep_merge_replaces_arrays_and_merges_objects() {
        let mut base = json!({"series":[{"type":"bar","data":[1]}],"title":{"text":"A"}});
        deep_merge(
            &mut base,
            &json!({"series":[{"data":[2,3]}],"title":{"subtext":"B"}}),
        );
        assert_eq!(base["series"], json!([{"data":[2,3]}]));
        assert_eq!(base["title"]["text"], "A");
        assert_eq!(base["title"]["subtext"], "B");
    }
}
