//! Write-time normalization for visual blocks.

use serde_json::Value;

use crate::{
    block::{Block, BlockKind},
    json::merge_optional_json,
    linter::{Diagnostic, Severity},
    model::ReportDocument,
};

/// Strip a non-center inner ECharts title when the block already has an outer title.
/// Returns `true` when anything was removed.
pub fn strip_duplicate_inner_title(option: &mut Value, outer_title_present: bool) -> bool {
    if !outer_title_present {
        return false;
    }
    let Some(title) = option.get_mut("title") else {
        return false;
    };
    match title {
        Value::Object(map) => {
            if is_center_title(map) {
                false
            } else {
                option.as_object_mut().map(|object| object.remove("title"));
                true
            }
        }
        Value::Array(titles) => {
            let before = titles.len();
            titles.retain(|title| title.as_object().is_some_and(is_center_title));
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

fn is_center_title(map: &serde_json::Map<String, Value>) -> bool {
    matches!(
        map.get("top")
            .and_then(Value::as_str)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("center" | "middle")
    )
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
    let mut effective = chart
        .option
        .clone()
        .unwrap_or_else(|| Value::Object(Default::default()));
    if let Some(patch) = &chart.option_patch {
        effective = merge_optional_json(Some(&effective), patch);
    }
    let mut stripped_title = false;
    let mut stripped_toolbox = false;
    if has_outer_title {
        stripped_title |= strip_duplicate_inner_title(&mut effective, true);
    }
    stripped_toolbox |= strip_chart_toolbox(&mut effective);
    // Strip the same chrome from both sources so the merge cannot reintroduce it.
    for option in [chart.option.as_mut(), chart.option_patch.as_mut()]
        .into_iter()
        .flatten()
    {
        if has_outer_title {
            stripped_title |= strip_duplicate_inner_title(option, true);
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
        block::{Block, EchartsBlock, EchartsMode, EchartsPreset},
        test_support::*,
    };

    fn chart() -> Block {
        Block::new(
            block_id("chart"),
            BlockKind::Echarts(EchartsBlock {
                title: Some("Outer".into()),
                height: 300,
                mode: EchartsMode::Preset,
                preset: Some(EchartsPreset::Bar),
                option: Some(serde_json::json!({
                    "title": {"text": "Inner", "top": "top"}
                })),
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
        assert!(chart.option.as_ref().unwrap().get("title").is_none());
        assert_eq!(
            chart.option_patch.as_ref().unwrap()["title"]["top"],
            "center"
        );
    }

    #[test]
    fn keeps_center_title_without_warning() {
        let mut block = chart();
        if let BlockKind::Echarts(chart) = &mut block.body {
            chart.option = Some(serde_json::json!({
                "title": {"text": "Center KPI", "top": "center"}
            }));
        }
        let warnings = normalize_block(&mut block);
        assert!(warnings.is_empty());
    }

    #[test]
    fn strips_toolbox_from_option_and_patch() {
        let mut block = chart();
        {
            let BlockKind::Echarts(chart) = &mut block.body else {
                panic!("chart");
            };
            chart.option = Some(serde_json::json!({
                "toolbox": {"feature": {"saveAsImage": {}}}
            }));
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
        assert!(chart.option.as_ref().unwrap().get("toolbox").is_none());
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
            chart.option = Some(serde_json::json!({
                "title": {"text": "Outer duplicate", "top": "top"}
            }));
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
        assert!(chart.option.as_ref().unwrap().get("title").is_none());
        assert!(chart.option_patch.as_ref().unwrap().get("title").is_none());
    }
}
