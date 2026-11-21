//! Core contract checks that can never be suppressed.

use std::collections::HashSet;

use quire_protocol::{
    BlockKind, ReportDocument, TableError, block::looks_like_uuid_v4, unknown_row_keys,
    unknown_width_overrides, validate_table,
};

use crate::{engine::Engine, registry::RuleId};

pub(crate) fn check(document: &ReportDocument, engine: &mut Engine<'_>) {
    let mut seen = HashSet::new();
    for (index, block) in document.blocks.iter().enumerate() {
        if !seen.insert(block.id.as_str()) {
            engine.report_values(
                RuleId::DuplicateBlockId,
                format!("duplicate block id `{}`", block.id),
                "rename one of the duplicate blocks",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].id")),
                vec![block_ids(document)],
            );
        }
        if looks_like_uuid_v4(block.id.as_str()) {
            engine.report(
                RuleId::BlockIdUuidV4,
                format!("block id `{}` is a UUIDv4", block.id),
                "replace it with a short semantic id such as `sec_intro` or `b_01`",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].id")),
            );
        }

        match &block.body {
            BlockKind::Heading { level, .. } => {
                if !(1..=6).contains(level) {
                    engine.report(
                        RuleId::HeadingLevelRange,
                        format!("heading `{}` has level {level}", block.id),
                        "heading level must be 1..=6",
                        Some(block.id.clone()),
                        Some(format!("blocks[{index}].level")),
                    );
                }
            }
            BlockKind::Table(table) => check_table_schema(document, engine, index, table),
            BlockKind::Paragraph { .. } | BlockKind::Quote { .. } | BlockKind::Echarts(_) => {}
            BlockKind::CustomHtml(_) => {}
        }
    }
}

fn check_table_schema(
    document: &ReportDocument,
    engine: &mut Engine<'_>,
    index: usize,
    table: &quire_protocol::TableBlock,
) {
    let Some(block) = document.blocks.get(index) else {
        return;
    };
    match validate_table(table) {
        Ok(shape) => {
            let unknown = unknown_row_keys(table, &shape);
            if !unknown.is_empty() {
                let valid = shape
                    .leaves
                    .iter()
                    .map(|leaf| leaf.key.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                engine.report_values(
                    RuleId::TableUnknownRowKey,
                    format!(
                        "table `{}` has row keys not present in leaf columns: {}",
                        block.id,
                        unknown.join(", ")
                    ),
                    format!("valid leaf keys: {valid}"),
                    Some(block.id.clone()),
                    Some(format!("blocks[{index}].rows")),
                    shape.leaves.iter().map(|leaf| leaf.key.clone()).collect(),
                );
            }
            let unknown_widths = unknown_width_overrides(&block.style, &shape);
            if !unknown_widths.is_empty() {
                let valid = shape
                    .leaves
                    .iter()
                    .map(|leaf| leaf.key.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                engine.report_values(
                    RuleId::TableUnknownWidthOverride,
                    format!(
                        "table `{}` has column_widths for unknown leaves: {}",
                        block.id,
                        unknown_widths.join(", ")
                    ),
                    format!("valid leaf keys: {valid}"),
                    Some(block.id.clone()),
                    Some(format!("blocks[{index}].style.column_widths")),
                    shape.leaves.iter().map(|leaf| leaf.key.clone()).collect(),
                );
            }
        }
        Err(error) => {
            let hint = match &error {
                TableError::MissingLeafKey { .. } | TableError::EmptyLeafKey { .. } => {
                    "add a unique short `key` to every leaf column; groups may omit keys"
                }
                TableError::DuplicateLeafKey { .. } => "rename one duplicate leaf column key",
                TableError::EmptyColumns => "declare at least one leaf ColumnNode with a key",
                TableError::EmptyGroup { .. } => "remove the empty group or add child columns",
            };
            engine.report(
                RuleId::TableSchemaInvalid,
                format!("table `{}`: {error}", block.id),
                hint,
                Some(block.id.clone()),
                Some(format!("blocks[{index}].columns")),
            );
        }
    }
}

fn block_ids(document: &ReportDocument) -> String {
    document
        .blocks
        .iter()
        .map(|block| block.id.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}
