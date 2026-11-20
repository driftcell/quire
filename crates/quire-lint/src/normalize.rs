//! Diagnostic-producing write-time normalization for chart chrome.

use quire_protocol::{
    Block, BlockKind, Diagnostic, ReportDocument, merge_optional_json,
    normalize::{strip_chart_toolbox, strip_duplicate_inner_title},
};

use crate::registry::RuleId;

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
        diagnostics.push(diagnostic(
            RuleId::EchartsInnerTitleStripped,
            format!(
                "chart `{}` had a top-aligned `option.title` alongside outer `title`; the inner title was stripped",
                block.id
            ),
            "Put titles in outer `title` ONLY; keep `option.title` only for center donut KPIs (`top: \"center\"`).",
            block.id.clone(),
            "option.title",
        ));
    }
    if stripped_toolbox {
        diagnostics.push(diagnostic(
            RuleId::EchartsToolboxStripped,
            format!(
                "chart `{}` used an interactive `toolbox`; it was stripped for static report export",
                block.id
            ),
            "Remove `toolbox` from stored options. Static reports and SSG exports do not expose interactive chart controls.",
            block.id.clone(),
            "option.toolbox",
        ));
    }
    diagnostics
}

fn diagnostic(
    rule: RuleId,
    message: String,
    hint: &str,
    block_id: quire_protocol::BlockId,
    path: &str,
) -> Diagnostic {
    let meta = rule.meta();
    Diagnostic {
        code: meta.code.to_string(),
        rule: meta.name.to_string(),
        severity: meta.default_severity,
        message,
        hint: hint.to_string(),
        suppressible: meta.suppressible,
        block_id: Some(block_id),
        path: Some(path.to_string()),
        valid_values: Vec::new(),
    }
}
