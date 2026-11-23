//! ECharts contract and safety checks.

use std::collections::BTreeMap;

use quire_protocol::{
    BlockId, BlockKind, EchartsBlock, EchartsComponent, EchartsOptionView, chart_series, entries,
    find_js_function_string, is_valid_series_type,
};
use serde_json::Value;

use crate::{
    context::LintContext,
    engine::{Engine, Finding},
    registry::RuleId,
};

mod cartesian;
mod colors;
mod contract;

pub(crate) fn check(context: &LintContext<'_>) -> Vec<Finding> {
    let mut engine = Engine::new();
    for (index, block) in context.document.blocks.iter().enumerate() {
        let BlockKind::Echarts(chart) = &block.body else {
            continue;
        };
        let Some(option) = context.effective_option(index) else {
            continue;
        };
        let Some(view) = context.chart_view(index) else {
            continue;
        };
        contract::check_inner_title(&mut engine, block, chart, view, index);
        contract::check_chart(&mut engine, block, chart, option, view, index);
    }
    colors::check_cross_chart_colors(context, &mut engine);
    engine.into_findings()
}
