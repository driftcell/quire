//! Write-time normalization for visual blocks.

use serde_json::Value;

use crate::{
    block::{Block, BlockKind},
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
    if chart
        .title
        .as_deref()
        .is_none_or(|title| title.trim().is_empty())
    {
        return Vec::new();
    }
    let mut stripped = false;
    if let Some(option) = &mut chart.option {
        stripped |= strip_duplicate_inner_title(option, true);
    }
    if let Some(option_patch) = &mut chart.option_patch {
        stripped |= strip_duplicate_inner_title(option_patch, true);
    }
    if stripped {
        vec![Diagnostic {
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
        }]
    } else {
        Vec::new()
    }
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
}
