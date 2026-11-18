//! Table schema validation and deterministic leaf-column flattening.
//!
//! Rendering (colspans, widths, pagination) belongs to the external frontend. This module
//! validates recursive header trees and exposes the ordered leaf columns that Markdown
//! projection, storage validation, and the linter need.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::block::{Align, CellValue, ColumnDataType, ColumnNode, ColumnSizing, TableBlock};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TableError {
    #[error("table has no columns; declare at least one leaf column with a unique `key`")]
    EmptyColumns,
    #[error("leaf column `{label}` is missing a `key`; every leaf needs a short stable key")]
    MissingLeafKey { label: String },
    #[error("leaf column `{label}` has an empty `key`")]
    EmptyLeafKey { label: String },
    #[error("duplicate leaf column key `{key}`; every leaf `key` must be unique")]
    DuplicateLeafKey { key: String },
    #[error("column group `{label}` contains no leaf columns")]
    EmptyGroup { label: String },
}

/// An ordered, fully resolved leaf column.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LeafColumn {
    pub key: String,
    pub label: String,
    /// Ancestor group labels from outermost to innermost.
    pub path: Vec<String>,
    pub data_type: ColumnDataType,
    pub sizing: ColumnSizing,
    pub weight: f32,
    pub align: Align,
    pub header_align: Align,
    /// Zero-based column position.
    pub index: usize,
}

impl LeafColumn {
    /// `Parent / Child` label used by the Markdown projector.
    pub fn flattened_label(&self) -> String {
        let mut parts = self.path.clone();
        parts.push(self.label.clone());
        parts.join(" / ")
    }
}

/// Validated shape of a table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TableShape {
    pub leaves: Vec<LeafColumn>,
    pub max_depth: usize,
}

impl TableShape {
    pub fn leaf(&self, key: &str) -> Option<&LeafColumn> {
        self.leaves.iter().find(|leaf| leaf.key == key)
    }

    pub fn leaf_keys(&self) -> BTreeSet<String> {
        self.leaves.iter().map(|leaf| leaf.key.clone()).collect()
    }

    pub fn has_key(&self, key: &str) -> bool {
        self.leaves.iter().any(|leaf| leaf.key == key)
    }
}

/// Validate a recursive column tree and flatten it into ordered leaf columns.
pub fn validate_table(table: &TableBlock) -> Result<TableShape, TableError> {
    if table.columns.is_empty() {
        return Err(TableError::EmptyColumns);
    }
    let mut seen = BTreeSet::new();
    let max_depth = table
        .columns
        .iter()
        .map(ColumnNode::max_depth)
        .max()
        .unwrap_or(1);
    let mut leaves = Vec::new();
    for column in &table.columns {
        flatten_node(column, &[], &mut seen, &mut leaves)?;
    }
    if leaves.is_empty() {
        return Err(TableError::EmptyColumns);
    }
    Ok(TableShape { leaves, max_depth })
}

fn flatten_node(
    node: &ColumnNode,
    path: &[String],
    seen: &mut BTreeSet<String>,
    leaves: &mut Vec<LeafColumn>,
) -> Result<(), TableError> {
    if !node.is_leaf() {
        let mut next_path = path.to_vec();
        next_path.push(node.label.clone());
        for child in &node.children {
            flatten_node(child, &next_path, seen, leaves)?;
        }
        return Ok(());
    }

    let key = match node.key.as_deref() {
        Some(key) if !key.trim().is_empty() => key.trim().to_string(),
        Some(_) => {
            return Err(TableError::EmptyLeafKey {
                label: node.label.clone(),
            });
        }
        None => {
            return Err(TableError::MissingLeafKey {
                label: node.label.clone(),
            });
        }
    };
    if !seen.insert(key.clone()) {
        return Err(TableError::DuplicateLeafKey { key });
    }
    let index = leaves.len();
    leaves.push(LeafColumn {
        key,
        label: node.label.clone(),
        path: path.to_vec(),
        data_type: node.effective_data_type(),
        sizing: node.effective_sizing(),
        weight: node.effective_weight(),
        align: node.effective_align(),
        header_align: node.effective_header_align(),
        index,
    });
    Ok(())
}

/// Unknown row keys, preserving stable first-seen ordering. BTreeMap cannot contain duplicate keys,
/// but nested objects may still produce unknown keys after agent-side JSON manipulation.
pub fn unknown_row_keys(table: &TableBlock, shape: &TableShape) -> Vec<String> {
    let valid = shape.leaf_keys();
    let mut unknown = BTreeSet::new();
    for row in table.rows.iter().chain(table.summary_rows.iter()) {
        for key in row.keys() {
            if !valid.contains(key) {
                unknown.insert(key.clone());
            }
        }
    }
    unknown.into_iter().collect()
}

/// Find explicit width overrides that do not match a leaf key.
pub fn unknown_width_overrides(
    style: &crate::block::BlockStyle,
    shape: &TableShape,
) -> Vec<String> {
    let mut unknown: Vec<String> = style
        .column_widths
        .as_ref()
        .map(|widths: &std::collections::BTreeMap<String, String>| {
            widths
                .keys()
                .filter(|key| !shape.has_key(key))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    unknown.sort();
    unknown
}

/// Display a cell with type-aware numeric annotation.
pub fn display_cell(cell: &CellValue, data_type: ColumnDataType) -> String {
    let mut text = cell.display_text();
    if data_type == ColumnDataType::Percent && cell.as_number().is_some() && !text.ends_with('%') {
        text.push('%');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{ColumnDataType, ColumnNode, ColumnSizing, TableBlock};
    use std::collections::BTreeMap;

    fn table(columns: Vec<ColumnNode>) -> TableBlock {
        TableBlock {
            title: None,
            numbering: None,
            note: None,
            columns,
            rows: vec![],
            summary_rows: vec![],
        }
    }

    #[test]
    fn nested_columns_flatten_in_display_order() {
        let columns = vec![
            ColumnNode::group(
                "revenue",
                "Revenue",
                vec![
                    ColumnNode::leaf("q1", "Q1")
                        .with_type(ColumnDataType::Currency)
                        .with_sizing(ColumnSizing::Grow),
                    ColumnNode::leaf("q2", "Q2").with_type(ColumnDataType::Currency),
                ],
            ),
            ColumnNode::leaf("region", "Region"),
            ColumnNode::group(
                "margin",
                "Margin",
                vec![ColumnNode::group(
                    "gross",
                    "Gross",
                    vec![
                        ColumnNode::leaf("gm_q1", "Q1").with_type(ColumnDataType::Percent),
                        ColumnNode::leaf("gm_q2", "Q2").with_type(ColumnDataType::Percent),
                    ],
                )],
            ),
        ];
        let shape = validate_table(&table(columns)).unwrap();
        assert_eq!(shape.max_depth, 3);
        assert_eq!(
            shape
                .leaves
                .iter()
                .map(|leaf| leaf.key.as_str())
                .collect::<Vec<_>>(),
            vec!["q1", "q2", "region", "gm_q1", "gm_q2"]
        );
        assert_eq!(shape.leaves[0].flattened_label(), "Revenue / Q1");
        assert_eq!(shape.leaves[2].flattened_label(), "Region");
        assert_eq!(shape.leaves[3].flattened_label(), "Margin / Gross / Q1");
        assert_eq!(shape.leaves[0].align, Align::Right);
        assert_eq!(shape.leaves[2].align, Align::Left);
        assert_eq!(shape.leaves[0].sizing, ColumnSizing::Grow);
    }

    #[test]
    fn alignment_defaults_follow_data_type() {
        let shape = validate_table(&table(vec![
            ColumnNode::leaf("metric", "Metric"),
            ColumnNode::leaf("amount", "Amount").with_type(ColumnDataType::Number),
            ColumnNode::leaf("rate", "Rate").with_type(ColumnDataType::Percent),
            ColumnNode::leaf("date", "Date").with_type(ColumnDataType::Date),
            ColumnNode::leaf("status", "Status").with_type(ColumnDataType::Badge),
        ]))
        .unwrap();
        assert_eq!(
            shape
                .leaves
                .iter()
                .map(|leaf| leaf.align)
                .collect::<Vec<_>>(),
            vec![
                Align::Left,
                Align::Right,
                Align::Right,
                Align::Center,
                Align::Center
            ]
        );
    }

    #[test]
    fn missing_duplicate_and_empty_columns_are_distinct_errors() {
        let mut missing = ColumnNode::leaf("ignored", "No key");
        missing.key = None;
        assert!(matches!(
            validate_table(&table(vec![missing])),
            Err(TableError::MissingLeafKey { .. })
        ));

        let empty = ColumnNode::leaf("", "Empty");
        assert!(matches!(
            validate_table(&table(vec![empty])),
            Err(TableError::EmptyLeafKey { .. })
        ));

        assert!(matches!(
            validate_table(&table(vec![
                ColumnNode::leaf("a", "A"),
                ColumnNode::leaf("a", "B")
            ])),
            Err(TableError::DuplicateLeafKey { .. })
        ));

        assert!(matches!(
            validate_table(&table(vec![])),
            Err(TableError::EmptyColumns)
        ));
    }

    #[test]
    fn unknown_row_and_width_keys_are_reported() {
        let mut table = table(vec![ColumnNode::leaf("a", "A")]);
        table.rows.push(BTreeMap::from([
            ("a".to_string(), CellValue::text("ok")),
            ("b".to_string(), CellValue::text("extra")),
        ]));
        let shape = validate_table(&table).unwrap();
        assert_eq!(unknown_row_keys(&table, &shape), vec!["b"]);
        let style = crate::block::BlockStyle {
            column_widths: Some(BTreeMap::from([("zzz".to_string(), "20%".to_string())])),
            ..Default::default()
        };
        assert_eq!(unknown_width_overrides(&style, &shape), vec!["zzz"]);
    }
}
