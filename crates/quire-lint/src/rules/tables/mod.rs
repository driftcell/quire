//! Table layout risk checks: narrow CJK headers, empty/constant columns, A4 overflow and units.

use std::collections::BTreeMap;

use quire_protocol::{
    BlockKind, CellValue, ColumnDataType, LeafColumn, TableBlock, TableShape, display_cell,
};

use crate::{
    context::LintContext,
    engine::{Engine, Finding},
    registry::RuleId,
};

mod layout;
mod values;

pub(crate) fn check(context: &LintContext<'_>) -> Vec<Finding> {
    let mut engine = Engine::new();
    for (index, block) in context.document.blocks.iter().enumerate() {
        let BlockKind::Table(table) = &block.body else {
            continue;
        };
        let Some(Ok(shape)) = context.table_shape(index) else {
            continue;
        };
        layout::check_column_width(&mut engine, block, table, shape, index);
        layout::check_empty_columns(&mut engine, block, table, shape, index);
        layout::check_constant_columns(&mut engine, block, table, shape, index);
        layout::check_a4_width(&mut engine, block, table, shape, index);
        values::check_numeric_units(&mut engine, block, table, shape, index);
        values::check_summary_row(&mut engine, block, table, shape, index);
    }
    engine.into_findings()
}
