//! Write-time normalization for visual blocks.

use serde_json::Value;

use crate::{
    block::{Block, BlockKind},
    echarts::{inner_title_duplicates, is_center_inner_title, is_positioned_or_light_inner_title},
    json::merge_optional_json,
    linter::{Diagnostic, Severity},
    model::ReportDocument,
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

/// Strip duplicate inner chart titles across a document and return non-blocking warnings.
pub fn normalize_document(document: &mut ReportDocument) -> Vec<Diagnostic> {
    let mut warnings = Vec::new();
    for block in &mut document.blocks {
        warnings.extend(normalize_block(block));
    }
    warnings
}

/// Normalize one block.
pub fn normalize_block(block: &mut Block) -> Vec<Diagnostic> {
    let BlockKind::Echarts(chart) = &mut block.body else {
        return Vec::new();
    };
    let has_outer_title = chart
        .title
        .as_deref()
        .is_some_and(|title| !title.trim().is_empty());
    let mut effective = chart.option.clone();
    if let Some(patch) = &chart.option_patch {
        effective = merge_optional_json(Some(&effective), patch);
    }
    let mut stripped_title = false;
    let mut stripped_toolbox = false;
    if has_outer_title {
        stripped_title |= strip_duplicate_inner_title(&mut effective, chart.title.as_deref());
    }
    stripped_toolbox |= strip_chart_toolbox(&mut effective);
    // Strip the same chrome from both sources so the merge cannot reintroduce it.
    for option in [Some(&mut chart.option), chart.option_patch.as_mut()]
        .into_iter()
        .flatten()
    {
        if has_outer_title {
            stripped_title |= strip_duplicate_inner_title(option, chart.title.as_deref());
        }
        stripped_toolbox |= strip_chart_toolbox(option);
    }
    let mut diagnostics = Vec::new();
    if stripped_title {
        diagnostics.push(Diagnostic {
            code: "ECHARTS_INNER_TITLE_STRIPPED".into(),
            severity: Severity::Warning,
            message: format!(
                "chart `{}` had a top-aligned `option.title` alongside outer `title`; the inner title was stripped",
                block.id
            ),
            hint: "Put titles in outer `title` ONLY; keep `option.title` only for center donut KPIs (`top: \"center\"`).".into(),
            block_id: Some(block.id.clone()),
            path: Some("option.title".into()),
            valid_values: Vec::new(),
        });
    }
    if stripped_toolbox {
        diagnostics.push(Diagnostic {
            code: "ECHARTS_TOOLBOX_STRIPPED".into(),
            severity: Severity::Warning,
            message: format!(
                "chart `{}` used an interactive `toolbox`; it was stripped for static report export",
                block.id
            ),
            hint: "Remove `toolbox` from stored options. Static reports and SSG exports do not expose interactive chart controls.".into(),
            block_id: Some(block.id.clone()),
            path: Some("option.toolbox".into()),
            valid_values: Vec::new(),
        });
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        block::{Block, EchartsBlock},
        test_support::*,
    };

    fn chart() -> Block {
        Block::new(
            block_id("chart"),
            BlockKind::Echarts(EchartsBlock {
                title: Some("Outer".into()),
                height: 300,
                option: serde_json::json!({
                    "title": {"text": "Inner", "top": "top"}
                }),
                option_patch: Some(serde_json::json!({
                    "title": {"text": "Center KPI", "top": "center"}
                })),
                fallback_summary: "Summary".into(),
                numbering: None,
                note: None,
            }),
        )
    }

    #[test]
    fn strips_non_center_inner_title_and_warns() {
        let mut block = chart();
        let warnings = normalize_block(&mut block);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].code, "ECHARTS_INNER_TITLE_STRIPPED");
        let BlockKind::Echarts(chart) = &block.body else {
            panic!("chart");
        };
        assert!(chart.option.get("title").is_none());
        assert_eq!(
            chart.option_patch.as_ref().unwrap()["title"]["top"],
            "center"
        );
    }

    #[test]
    fn keeps_center_title_without_warning() {
        let mut block = chart();
        if let BlockKind::Echarts(chart) = &mut block.body {
            chart.option = serde_json::json!({
                "title": {"text": "Center KPI", "top": "center"}
            });
        }
        let warnings = normalize_block(&mut block);
        assert!(warnings.is_empty());
    }

    #[test]
    fn keeps_multi_panel_subtitles_and_removes_outer_duplicates() {
        let mut block = chart();
        if let BlockKind::Echarts(chart) = &mut block.body {
            chart.option = serde_json::json!({
                "title": [
                    {"text": "华东区", "left": "4%", "top": 4, "textStyle": {"fontSize": 12}},
                    {"text": "Outer", "left": "54%", "top": 4},
                    {"text": "华南区", "left": "54%", "top": 4, "textStyle": {"fontSize": 12}}
                ]
            });
            chart.option_patch = None;
        }
        let warnings = normalize_block(&mut block);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.code == "ECHARTS_INNER_TITLE_STRIPPED")
        );
        let BlockKind::Echarts(chart) = &block.body else {
            panic!("chart");
        };
        let titles = chart.option["title"].as_array().expect("subtitle array");
        assert_eq!(titles.len(), 2);
        assert!(titles.iter().all(|title| title["text"] != "Outer"));
    }

    #[test]
    fn keeps_a_single_positioned_light_subtitle() {
        let mut block = chart();
        if let BlockKind::Echarts(chart) = &mut block.body {
            chart.option = serde_json::json!({
                "title": {
                    "text": "华东区",
                    "left": "24%",
                    "top": 4,
                    "textStyle": {"fontSize": 12}
                }
            });
            chart.option_patch = None;
        }
        let warnings = normalize_block(&mut block);
        assert!(warnings.is_empty());
        let BlockKind::Echarts(chart) = &block.body else {
            panic!("chart");
        };
        assert_eq!(chart.option["title"]["text"], "华东区");
    }

    #[test]
    fn strips_toolbox_from_option_and_patch() {
        let mut block = chart();
        {
            let BlockKind::Echarts(chart) = &mut block.body else {
                panic!("chart");
            };
            chart.option = serde_json::json!({
                "toolbox": {"feature": {"saveAsImage": {}}}
            });
            chart.option_patch = Some(serde_json::json!({
                "toolbox": {"show": true},
                "series": [{"type": "bar", "data": [1, 2]}]
            }));
        }
        let warnings = normalize_block(&mut block);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.code == "ECHARTS_TOOLBOX_STRIPPED")
        );
        let BlockKind::Echarts(chart) = &block.body else {
            panic!("chart");
        };
        assert!(chart.option.get("toolbox").is_none());
        assert!(
            chart
                .option_patch
                .as_ref()
                .unwrap()
                .get("toolbox")
                .is_none()
        );
        assert!(chart.option_patch.as_ref().unwrap().get("series").is_some());
    }

    #[test]
    fn strips_inner_title_from_merged_patch_override() {
        let mut block = chart();
        {
            let BlockKind::Echarts(chart) = &mut block.body else {
                panic!("chart");
            };
            chart.option = serde_json::json!({
                "title": {"text": "Outer duplicate", "top": "top"}
            });
            chart.option_patch = Some(serde_json::json!({
                "title": {"text": "Patch duplicate"}
            }));
        }
        let warnings = normalize_block(&mut block);
        assert!(
            warnings
                .iter()
                .any(|warning| warning.code == "ECHARTS_INNER_TITLE_STRIPPED")
        );
        let BlockKind::Echarts(chart) = &block.body else {
            panic!("chart");
        };
        assert!(chart.option.get("title").is_none());
        assert!(chart.option_patch.as_ref().unwrap().get("title").is_none());
    }
}
