//! Reusable write-time normalization helpers for visual block payloads.
//!
//! Diagnostic-producing document normalization lives in `quire-lint`; this module only contains
//! pure JSON transforms shared by Markdown projection and the lint normalizer.

use serde_json::Value;

use crate::echarts::{
    inner_title_duplicates, is_center_inner_title, is_positioned_or_light_inner_title,
};

/// Strip inner ECharts titles that duplicate the outer title or are not valid
/// multi-panel subtitles. Returns `true` when anything was removed.
pub fn strip_duplicate_inner_title(option: &mut Value, outer_title: Option<&str>) -> bool {
    let Some(outer_title) = outer_title.map(str::trim).filter(|title| !title.is_empty()) else {
        return false;
    };
    let Some(title) = option.get_mut("title") else {
        return false;
    };
    match title {
        Value::Object(map) => {
            let entry = Value::Object(map.clone());
            let keep = !inner_title_duplicates(&entry, outer_title)
                && (is_center_inner_title(&entry) || is_positioned_or_light_inner_title(&entry));
            if !keep {
                option.as_object_mut().map(|object| object.remove("title"));
            }
            !keep
        }
        Value::Array(titles) => {
            let before = titles.len();
            titles.retain(|entry| entry.is_object() && !inner_title_duplicates(entry, outer_title));
            let stripped = titles.len() != before;
            if titles.is_empty()
                && let Some(object) = option.as_object_mut()
            {
                object.remove("title");
            }
            stripped
        }
        _ => false,
    }
}

/// Remove the interactive `toolbox` from a static report chart. Returns `true` when removed.
pub fn strip_chart_toolbox(option: &mut Value) -> bool {
    option
        .as_object_mut()
        .is_some_and(|object| object.remove("toolbox").is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_duplicate_title_but_keeps_center_titles() {
        let mut option = serde_json::json!({
            "title": [
                {"text": "Outer", "left": "4%"},
                {"text": "KPI", "top": "center"}
            ]
        });
        assert!(strip_duplicate_inner_title(&mut option, Some("Outer")));
        let titles = option["title"].as_array().expect("titles");
        assert_eq!(titles.len(), 1);
        assert_eq!(titles[0]["text"], "KPI");

        let mut center = serde_json::json!({"title": {"text": "KPI", "top": "center"}});
        assert!(!strip_duplicate_inner_title(&mut center, Some("Outer")));
        assert!(center.get("title").is_some());
    }

    #[test]
    fn strips_toolbox() {
        let mut option = serde_json::json!({
            "toolbox": {"show": true},
            "series": [{"type": "bar", "data": [1]}]
        });
        assert!(strip_chart_toolbox(&mut option));
        assert!(option.get("toolbox").is_none());
        assert!(option.get("series").is_some());
    }
}
