//! Pre-computed metadata for the read-only external frontend.
//!
//! This is deliberately not a renderer. It resolves the layout mathematics a frontend would
//! otherwise have to duplicate: multi-level table spans, P90 percentage widths, citation
//! numbering/bibliography, and the heading tree filtered by `toc.max_depth`.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    block::{
        Align, BlockKind, ColumnDataType, ColumnNode, ColumnSizing, NumberingKind, TableBlock,
    },
    citation::{BibliographyEntry, CitationResolution, resolve_citations},
    echarts::{
        ChartDataTable, chart_data_tables, merged_option, required_extensions, required_maps,
    },
    ids::{BlockId, SourceId},
    inline::inline_plain_text,
    model::ReportDocument,
    table::{TableError, display_cell, validate_table},
    text::{slugify, visual_width_multiline},
};

/// A resolved multi-level table header cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutHeaderCell {
    pub key: String,
    pub label: String,
    pub colspan: usize,
    pub rowspan: usize,
    pub data_type: ColumnDataType,
    pub align: Align,
    pub is_group: bool,
}

/// A layout-ready leaf column with normalized percentage width.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutLeafColumn {
    pub key: String,
    pub label: String,
    pub path: Vec<String>,
    pub data_type: ColumnDataType,
    pub sizing: ColumnSizing,
    pub weight: f32,
    pub align: Align,
    pub header_align: Align,
    pub index: usize,
    /// Normalized percentage. All leaves in a layout sum to 100.00.
    pub width_pct: f64,
    pub width_css: String,
}

impl LayoutLeafColumn {
    pub fn flattened_label(&self) -> String {
        let mut parts = self.path.clone();
        parts.push(self.label.clone());
        parts.join(" / ")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TableLayout {
    pub table_block_id: BlockId,
    pub header_rows: Vec<Vec<LayoutHeaderCell>>,
    pub leaf_columns: Vec<LayoutLeafColumn>,
    pub header_depth: usize,
    /// Always 100.00 when at least one leaf exists.
    pub total_width_pct: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TocNode {
    pub block_id: BlockId,
    pub level: u8,
    pub text: String,
    /// Hierarchical number such as `2.1`; frontends may hide it for the standard variant.
    pub number: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<TocNode>,
}

/// Pre-computed outer framing for a numbered visual block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BlockFraming {
    pub block_id: BlockId,
    pub numbering: NumberingKind,
    pub number: usize,
    /// Exact label shared with Markdown projection, for example `表 1` or `图 2`.
    pub label: String,
    /// Formatted source attribution/note line, when sources or a note exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CitationMapEntry {
    pub number: usize,
    pub label: String,
    pub title: String,
    pub source_id: SourceId,
}

/// ECharts maps and extension series that the frontend must register before rendering.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChartRequirements {
    /// Map names referenced by `geo.map` or `series[type=map].map`.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub required_maps: BTreeSet<String>,
    /// Extension series types such as `wordCloud`, `liquidFill` or `bar3D`.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub required_extensions: BTreeSet<String>,
}

impl ChartRequirements {
    pub fn is_empty(&self) -> bool {
        self.required_maps.is_empty() && self.required_extensions.is_empty()
    }
}

/// Compute map/extension dependencies for one chart block before it is committed.
pub fn chart_requirements(chart: &crate::block::EchartsBlock) -> ChartRequirements {
    let option = merged_option(chart);
    ChartRequirements {
        required_maps: required_maps(&option),
        required_extensions: required_extensions(&option),
    }
}

/// Compute the structured data tables for one chart block before it is committed.
pub fn chart_data_tables_for_block(chart: &crate::block::EchartsBlock) -> Vec<ChartDataTable> {
    chart_data_tables(&merged_option(chart))
}

/// Everything the external frontend needs beyond the canonical Block AST.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ComputedDocumentMeta {
    pub table_layouts: BTreeMap<BlockId, TableLayout>,
    pub citation_map: BTreeMap<SourceId, CitationMapEntry>,
    pub bibliography: Vec<BibliographyEntry>,
    pub toc_tree: Vec<TocNode>,
    /// Auto-numbering labels and source attribution for table/figure blocks.
    pub block_framing: BTreeMap<BlockId, BlockFraming>,
    /// ECharts map/extension dependencies grouped by chart block.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub chart_requirements: BTreeMap<BlockId, ChartRequirements>,
    /// Structured chart data tables grouped by chart block.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub chart_tables: BTreeMap<BlockId, Vec<ChartDataTable>>,
}

/// Compute frontend view metadata. Invalid table schemas are skipped here and surfaced by
/// `lint_document`, so a single bad table cannot block the rest of the document.
pub fn compute_document_meta(document: &ReportDocument) -> ComputedDocumentMeta {
    let citations = resolve_citations(document);
    let mut table_layouts: BTreeMap<BlockId, TableLayout> = BTreeMap::new();
    let mut chart_requirements = BTreeMap::new();
    let mut chart_tables = BTreeMap::new();
    for block in &document.blocks {
        match &block.body {
            BlockKind::Table(table) => {
                if let Ok(layout) = compute_table_layout(&block.id, table, &block.style) {
                    table_layouts.insert(block.id.clone(), layout);
                }
            }
            BlockKind::Echarts(chart) => {
                let requirements = crate::computed::chart_requirements(chart);
                if !requirements.is_empty() {
                    chart_requirements.insert(block.id.clone(), requirements);
                }
                let tables = chart_data_tables_for_block(chart);
                if !tables.is_empty() {
                    chart_tables.insert(block.id.clone(), tables);
                }
            }
            BlockKind::Heading { .. }
            | BlockKind::Paragraph { .. }
            | BlockKind::Quote { .. }
            | BlockKind::CustomHtml(_) => {}
        }
    }
    ComputedDocumentMeta {
        table_layouts,
        citation_map: citation_map(&citations),
        bibliography: citations.bibliography.clone(),
        toc_tree: compute_toc_tree(document),
        block_framing: compute_block_framing(document, &citations),
        chart_requirements,
        chart_tables,
    }
}

/// Compute deterministic `表 N` / `图 N` labels and attribution lines.
pub fn compute_block_framing(
    document: &ReportDocument,
    citations: &CitationResolution,
) -> BTreeMap<BlockId, BlockFraming> {
    let mut tables = 0usize;
    let mut figures = 0usize;
    let mut framing = BTreeMap::new();
    for block in &document.blocks {
        let (kind, note) = match &block.body {
            BlockKind::Table(table) => (table.numbering_kind(), table.note.as_deref()),
            BlockKind::Echarts(chart) => (chart.numbering_kind(), chart.note.as_deref()),
            BlockKind::CustomHtml(html) => (html.numbering_kind(), html.note.as_deref()),
            _ => continue,
        };
        let (number, label) = match kind {
            NumberingKind::Table => {
                tables += 1;
                (tables, format!("表 {tables}"))
            }
            NumberingKind::Figure => {
                figures += 1;
                (figures, format!("图 {figures}"))
            }
            NumberingKind::None => continue,
        };
        let source_note = block
            .source_note
            .as_deref()
            .map(str::trim)
            .filter(|note| !note.is_empty());
        let mut attribution_parts = Vec::new();
        if !block.source_ids.is_empty() || source_note.is_some() {
            let mut line = String::from("资料来源：");
            if let Some(source_note) = source_note {
                line.push_str(source_note);
            }
            let mut seen = BTreeSet::new();
            let labels: Vec<String> = block
                .source_ids
                .iter()
                .filter(|source_id| seen.insert((*source_id).clone()))
                .map(|source_id| citations.label_or_missing(source_id))
                .collect();
            if !labels.is_empty() {
                if source_note.is_some() {
                    line.push(' ');
                }
                line.push_str(&labels.join(" "));
            }
            attribution_parts.push(line);
        }
        if let Some(note) = note.map(str::trim).filter(|note| !note.is_empty()) {
            attribution_parts.push(format!("注：{note}"));
        }
        let attribution = if attribution_parts.is_empty() {
            None
        } else {
            Some(attribution_parts.join(" "))
        };
        framing.insert(
            block.id.clone(),
            BlockFraming {
                block_id: block.id.clone(),
                numbering: kind,
                number,
                label,
                attribution,
            },
        );
    }
    framing
}

fn citation_map(citations: &CitationResolution) -> BTreeMap<SourceId, CitationMapEntry> {
    let mut map = BTreeMap::new();
    for source_id in &citations.order {
        let Some(number) = citations.number(source_id) else {
            continue;
        };
        let label = citations.label(source_id).unwrap_or("[?]").to_string();
        let title = citations
            .bibliography
            .iter()
            .find(|entry| &entry.source_id == source_id)
            .map(|entry| entry.source.title.clone())
            .unwrap_or_default();
        map.insert(
            source_id.clone(),
            CitationMapEntry {
                number,
                label,
                title,
                source_id: source_id.clone(),
            },
        );
    }
    map
}

/// Compute the heading hierarchy filtered by `toc.max_depth` (1..=3).
pub fn compute_toc_tree(document: &ReportDocument) -> Vec<TocNode> {
    let max_depth = document.toc.max_depth.clamp(1, 3);
    let mut entries = Vec::new();
    let mut counters = [0u32; 6];
    for block in &document.blocks {
        let BlockKind::Heading { level, text } = &block.body else {
            continue;
        };
        let level = (*level).clamp(1, 6);
        if level > max_depth {
            continue;
        }
        let index = usize::from(level - 1);
        let Some(counter) = counters.get_mut(index) else {
            continue;
        };
        *counter += 1;
        for later in counters.iter_mut().skip(index + 1) {
            *later = 0;
        }
        let number = counters
            .get(..=index)
            .unwrap_or(&counters)
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(".");
        entries.push(TocNode {
            block_id: block.id.clone(),
            level,
            text: inline_plain_text(text),
            number,
            children: Vec::new(),
        });
    }

    fn build(entries: &[TocNode], cursor: &mut usize, parent_level: u8) -> Vec<TocNode> {
        let mut nodes = Vec::new();
        while *cursor < entries.len() {
            let Some(entry) = entries.get(*cursor) else {
                break;
            };
            if entry.level <= parent_level {
                break;
            }
            *cursor += 1;
            let level = entry.level;
            let mut node = entry.clone();
            node.children = build(entries, cursor, level);
            nodes.push(node);
        }
        nodes
    }

    let mut cursor = 0;
    build(&entries, &mut cursor, 0)
}

/// Compute a table's header geometry and normalized percentage widths.
pub fn compute_table_layout(
    block_id: &BlockId,
    table: &TableBlock,
    style: &crate::block::BlockStyle,
) -> Result<TableLayout, TableError> {
    let shape = validate_table(table)?;
    let header_depth = shape.max_depth;
    let mut header_rows = vec![Vec::new(); header_depth];
    for column in &table.columns {
        fill_header_rows(column, 0, header_depth, &[], &mut header_rows);
    }
    let widths = compute_widths(table, &shape, style);
    let leaf_columns: Vec<LayoutLeafColumn> = shape
        .leaves
        .into_iter()
        .map(|leaf| {
            let width = widths.get(leaf.index).copied().unwrap_or(0.0);
            LayoutLeafColumn {
                key: leaf.key,
                label: leaf.label,
                path: leaf.path,
                data_type: leaf.data_type,
                sizing: leaf.sizing,
                weight: leaf.weight,
                align: leaf.align,
                header_align: leaf.header_align,
                index: leaf.index,
                width_pct: width,
                width_css: format!("{}%", trim_number(width)),
            }
        })
        .collect();
    let total_width_pct = round2(leaf_columns.iter().map(|leaf| leaf.width_pct).sum());
    Ok(TableLayout {
        table_block_id: block_id.clone(),
        header_rows,
        leaf_columns,
        header_depth,
        total_width_pct,
    })
}

fn fill_header_rows(
    node: &ColumnNode,
    depth: usize,
    header_depth: usize,
    ancestors: &[String],
    rows: &mut [Vec<LayoutHeaderCell>],
) {
    let Some(row) = rows.get_mut(depth) else {
        return;
    };
    if node.is_leaf() {
        row.push(LayoutHeaderCell {
            key: node.key.clone().unwrap_or_default(),
            label: node.label.clone(),
            colspan: 1,
            rowspan: header_depth.saturating_sub(depth).max(1),
            data_type: node.effective_data_type(),
            align: node.effective_header_align(),
            is_group: false,
        });
        return;
    }
    let mut path = ancestors.to_vec();
    path.push(node.label.clone());
    row.push(LayoutHeaderCell {
        key: node
            .key
            .clone()
            .filter(|key| !key.trim().is_empty())
            .unwrap_or_else(|| format!("group-{}", slugify(&node.label))),
        label: node.label.clone(),
        colspan: node.leaf_count(),
        rowspan: 1,
        data_type: node.effective_data_type(),
        align: node.effective_header_align(),
        is_group: true,
    });
    for child in &node.children {
        fill_header_rows(child, depth + 1, header_depth, &path, rows);
    }
}

fn compute_widths(
    table: &TableBlock,
    shape: &crate::table::TableShape,
    style: &crate::block::BlockStyle,
) -> Vec<f64> {
    let mut explicit: Vec<Option<f64>> = vec![None; shape.leaves.len()];
    for leaf in &shape.leaves {
        let Some(slot) = explicit.get_mut(leaf.index) else {
            continue;
        };
        if let Some(raw) = style
            .column_widths
            .as_ref()
            .and_then(|widths| widths.get(&leaf.key))
            && let Some(percent) = parse_width_percent(raw)
        {
            *slot = Some(percent.clamp(0.0, 100.0));
        }
    }
    let explicit_total: f64 = explicit.iter().flatten().sum();
    let computed_indices: Vec<usize> = (0..shape.leaves.len())
        .filter(|index| explicit.get(*index).is_none_or(Option::is_none))
        .collect();
    let mut desired = vec![0.0; shape.leaves.len()];
    for leaf in &shape.leaves {
        let Some(slot) = desired.get_mut(leaf.index) else {
            continue;
        };
        if let Some(percent) = explicit.get(leaf.index).copied().flatten() {
            *slot = percent;
            continue;
        }
        let mut widths = vec![visual_width_multiline(&leaf.label)];
        for row in table.rows.iter().chain(table.summary_rows.iter()) {
            if let Some(value) = row.get(&leaf.key) {
                let mut text = display_cell(value, leaf.data_type);
                if leaf.data_type == ColumnDataType::Percent
                    && value.as_number().is_some()
                    && !text.ends_with('%')
                {
                    text.push('%');
                }
                widths.push(visual_width_multiline(&text));
            }
        }
        let p90 = percentile_90(&mut widths).clamp(0.0, 80.0);
        let weight = if leaf.weight.is_finite() {
            f64::from(leaf.weight).clamp(0.05, 100.0)
        } else {
            1.0
        };
        *slot = match leaf.sizing {
            ColumnSizing::Fit => (p90 + 2.0).max(4.0) * weight,
            ColumnSizing::Grow => 12.0 * weight,
        };
    }

    let mut explicit_total = explicit_total;
    if !computed_indices.is_empty() && explicit_total > 95.0 {
        let scale = 95.0 / explicit_total;
        for (index, value) in explicit.iter_mut().enumerate() {
            if let Some(percent) = value {
                *percent *= scale;
                if let Some(slot) = desired.get_mut(index) {
                    *slot = *percent;
                }
            }
        }
        explicit_total = 95.0;
    }
    if computed_indices.is_empty() {
        let total: f64 = desired.iter().sum();
        if total <= 0.0 {
            return vec![round2(100.0 / shape.leaves.len() as f64); shape.leaves.len()];
        }
        let mut widths: Vec<f64> = desired
            .iter()
            .map(|value| round2(value / total * 100.0))
            .collect();
        fix_rounding(&mut widths);
        return widths;
    }

    let remaining = (100.0 - explicit_total).max(0.0);
    let computed_total: f64 = computed_indices
        .iter()
        .map(|index| desired.get(*index).copied().unwrap_or_default().max(0.0001))
        .sum();
    let mut widths = desired
        .iter()
        .enumerate()
        .map(|(index, value)| {
            if explicit.get(index).copied().flatten().is_some() {
                round2(*value)
            } else if computed_total > 0.0 {
                round2(remaining * value.max(0.0001) / computed_total)
            } else {
                round2(remaining / computed_indices.len() as f64)
            }
        })
        .collect::<Vec<_>>();
    fix_rounding(&mut widths);
    widths
}

fn fix_rounding(widths: &mut [f64]) {
    let sum: f64 = widths.iter().sum();
    let residual = round2(100.0 - sum);
    if residual.abs() < 0.000_001 {
        return;
    }
    if let Some((index, _)) = widths
        .iter()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(right.1))
        && let Some(value) = widths.get_mut(index)
    {
        *value = round2(*value + residual).max(0.0);
    }
}

fn percentile_90(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|left, right| left.total_cmp(right));
    let rank = ((values.len() as f64) * 0.9).ceil() as usize;
    let index = rank.saturating_sub(1).min(values.len().saturating_sub(1));
    values.get(index).copied().unwrap_or_default()
}

fn parse_width_percent(raw: &str) -> Option<f64> {
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.is_empty() || trimmed.len() > 32 {
        return None;
    }
    if let Some(number) = trimmed.strip_suffix('%') {
        return number
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite());
    }
    let (number, unit) = split_number_unit(&trimmed)?;
    let value: f64 = number.parse().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let nominal_px = 960.0;
    let px = match unit {
        "" | "%" => return Some(value),
        "px" => value,
        "pt" => value * (96.0 / 72.0),
        "pc" => value * 16.0,
        "in" => value * 96.0,
        "cm" => value * (96.0 / 2.54),
        "mm" => value * (96.0 / 25.4),
        "em" | "rem" => value * 16.0,
        "ch" => value * 8.0,
        "vw" => value * 9.6,
        "vh" => value * 5.4,
        _ => return None,
    };
    Some(px / nominal_px * 100.0)
}

fn split_number_unit(input: &str) -> Option<(&str, &str)> {
    let unit_start = input
        .find(|ch: char| ch.is_ascii_alphabetic())
        .unwrap_or(input.len());
    let (number, unit) = input.split_at(unit_start);
    if number.is_empty()
        || !number.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
        || number.chars().filter(|ch| *ch == '.').count() > 1
    {
        return None;
    }
    Some((number, unit))
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn trim_number(value: f64) -> String {
    let mut rendered = format!("{value:.2}");
    while rendered.ends_with('0') {
        rendered.pop();
    }
    if rendered.ends_with('.') {
        rendered.pop();
    }
    if rendered.is_empty() || rendered == "-0" {
        "0".to_string()
    } else {
        rendered
    }
}

/// Return the set of table leaf keys for external validation helpers.
pub fn table_leaf_keys(layout: &TableLayout) -> BTreeSet<String> {
    layout
        .leaf_columns
        .iter()
        .map(|leaf| leaf.key.clone())
        .collect()
}

/// Convenience map for quick block lookups in frontend code.
pub fn layouts_by_key(document: &ReportDocument) -> BTreeMap<BlockId, TableLayout> {
    compute_document_meta(document)
        .table_layouts
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use crate::{
        block::{Block, BlockStyle, CellValue, ColumnNode, TableBlock},
        model::{ReportDocument, SourceEntry},
    };
    use std::collections::BTreeMap;

    fn nested_table() -> TableBlock {
        TableBlock {
            title: Some("Revenue".into()),
            numbering: None,
            note: None,
            columns: vec![
                ColumnNode::group(
                    "revenue",
                    "Revenue",
                    vec![
                        ColumnNode::leaf("q1", "Q1").with_type(ColumnDataType::Currency),
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
            ],
            rows: vec![
                BTreeMap::from([
                    ("region".to_string(), CellValue::text("North America")),
                    ("q1".to_string(), CellValue::Integer(120)),
                    ("q2".to_string(), CellValue::Integer(150)),
                    ("gm_q1".to_string(), CellValue::Float(62.5)),
                    ("gm_q2".to_string(), CellValue::Float(64.0)),
                ]),
                BTreeMap::from([
                    ("region".to_string(), CellValue::text("EMEA")),
                    ("q1".to_string(), CellValue::Integer(80)),
                    ("q2".to_string(), CellValue::Integer(95)),
                    ("gm_q1".to_string(), CellValue::Float(58.0)),
                    ("gm_q2".to_string(), CellValue::Float(60.0)),
                ]),
            ],
            summary_rows: vec![BTreeMap::from([
                ("region".to_string(), CellValue::text("Total")),
                ("q1".to_string(), CellValue::Integer(200)),
                ("q2".to_string(), CellValue::Integer(245)),
            ])],
        }
    }

    #[test]
    fn computes_three_level_spans_and_exact_widths() {
        let layout = compute_table_layout(
            &block_id("financials"),
            &nested_table(),
            &BlockStyle::default(),
        )
        .unwrap();
        assert_eq!(layout.header_depth, 3);
        assert_eq!(layout.leaf_columns.len(), 5);
        assert_eq!(layout.header_rows.len(), 3);
        let row0 = &layout.header_rows[0];
        assert_eq!(
            row0.iter()
                .map(|cell| (cell.label.as_str(), cell.colspan, cell.rowspan))
                .collect::<Vec<_>>(),
            vec![("Revenue", 2, 1), ("Region", 1, 3), ("Margin", 2, 1)]
        );
        let row1 = &layout.header_rows[1];
        assert_eq!(
            row1.iter()
                .map(|cell| (cell.label.as_str(), cell.colspan, cell.rowspan))
                .collect::<Vec<_>>(),
            vec![("Q1", 1, 2), ("Q2", 1, 2), ("Gross", 2, 1)]
        );
        assert_eq!(
            layout.total_width_pct,
            100.0,
            "{:?}",
            layout
                .leaf_columns
                .iter()
                .map(|leaf| leaf.width_pct)
                .collect::<Vec<_>>()
        );
        assert!(layout.leaf_columns.iter().all(|leaf| leaf.width_pct > 0.0));
        assert!(
            layout
                .leaf_columns
                .iter()
                .all(|leaf| leaf.width_css.ends_with('%'))
        );
    }

    #[test]
    fn p90_and_cjk_content_influence_widths() {
        let mut table = nested_table();
        table.rows[0].insert(
            "region".to_string(),
            CellValue::text("北美洲地区业务收入明细"),
        );
        let layout = compute_table_layout(&block_id("t"), &table, &BlockStyle::default()).unwrap();
        let region = layout
            .leaf_columns
            .iter()
            .find(|leaf| leaf.key == "region")
            .unwrap();
        assert!(region.width_pct > 10.0);
    }

    #[test]
    fn explicit_widths_are_respected_and_normalized() {
        let style = BlockStyle {
            column_widths: Some(BTreeMap::from([
                ("region".to_string(), "120px".to_string()),
                ("q1".to_string(), "15%".to_string()),
            ])),
            ..Default::default()
        };
        let layout = compute_table_layout(&block_id("t"), &nested_table(), &style).unwrap();
        let region = layout
            .leaf_columns
            .iter()
            .find(|leaf| leaf.key == "region")
            .unwrap();
        assert!((region.width_pct - 12.5).abs() < 0.01);
        let q1 = layout
            .leaf_columns
            .iter()
            .find(|leaf| leaf.key == "q1")
            .unwrap();
        assert!((q1.width_pct - 15.0).abs() < 0.01);
        assert_eq!(layout.total_width_pct, 100.0);
    }

    #[test]
    fn cjk_visual_width_widens_fitted_column() {
        let table = TableBlock {
            title: None,
            numbering: None,
            note: None,
            columns: vec![
                ColumnNode::leaf("cjk", "CJK"),
                ColumnNode::leaf("ascii", "ASCII"),
            ],
            rows: vec![BTreeMap::from([
                ("cjk".to_string(), CellValue::text("短期收入增长强劲")),
                ("ascii".to_string(), CellValue::text("shortrev")),
            ])],
            summary_rows: vec![],
        };
        let layout =
            compute_table_layout(&block_id("cjk"), &table, &BlockStyle::default()).unwrap();
        let cjk = layout
            .leaf_columns
            .iter()
            .find(|leaf| leaf.key == "cjk")
            .unwrap();
        let ascii = layout
            .leaf_columns
            .iter()
            .find(|leaf| leaf.key == "ascii")
            .unwrap();
        assert!(
            cjk.width_pct > ascii.width_pct,
            "CJK={} ASCII={}",
            cjk.width_pct,
            ascii.width_pct
        );
    }

    #[test]
    fn p90_ignores_a_single_huge_outlier() {
        let mut outlier_rows = Vec::new();
        for index in 0..10 {
            let outlier = if index == 9 {
                "X".repeat(80)
            } else {
                "x".to_string()
            };
            outlier_rows.push(BTreeMap::from([
                ("outlier".to_string(), CellValue::text(outlier)),
                ("steady".to_string(), CellValue::text("steadyvalue")),
            ]));
        }
        let table = TableBlock {
            title: None,
            numbering: None,
            note: None,
            columns: vec![
                ColumnNode::leaf("outlier", "O"),
                ColumnNode::leaf("steady", "S"),
            ],
            rows: outlier_rows,
            summary_rows: vec![],
        };
        let layout =
            compute_table_layout(&block_id("p90"), &table, &BlockStyle::default()).unwrap();
        let outlier = layout
            .leaf_columns
            .iter()
            .find(|leaf| leaf.key == "outlier")
            .unwrap();
        let steady = layout
            .leaf_columns
            .iter()
            .find(|leaf| leaf.key == "steady")
            .unwrap();
        assert!(
            outlier.width_pct < steady.width_pct,
            "P90 should ignore the 80-char outlier: outlier={} steady={}",
            outlier.width_pct,
            steady.width_pct
        );
    }

    #[test]
    fn citations_number_across_prose_tables_and_charts() {
        let mut doc = ReportDocument::new(doc_id("doc"));
        doc.sources
            .insert(source_id("a"), SourceEntry::new("Prose source"));
        doc.sources
            .insert(source_id("b"), SourceEntry::new("Table source"));
        doc.sources
            .insert(source_id("c"), SourceEntry::new("Chart source"));
        doc.blocks
            .push(Block::paragraph(block_id("p"), "Hello [^a]"));
        doc.blocks.push(
            Block::table(
                block_id("t"),
                TableBlock {
                    title: None,
                    numbering: None,
                    note: None,
                    columns: vec![ColumnNode::leaf("metric", "Metric")],
                    rows: vec![],
                    summary_rows: vec![],
                },
            )
            .with_sources([source_id("b")]),
        );
        doc.blocks.push(
            Block::new(
                block_id("chart"),
                crate::block::BlockKind::Echarts(crate::block::EchartsBlock {
                    title: None,
                    height: 240,
                    option: serde_json::json!({"series": [{"type": "bar", "data": [1]}]}),
                    option_patch: None,
                    fallback_summary: "Chart summary".into(),
                    numbering: None,
                    note: None,
                }),
            )
            .with_sources([source_id("c")]),
        );
        let meta = compute_document_meta(&doc);
        assert_eq!(
            meta.citation_map.get("a").map(|entry| entry.number),
            Some(1)
        );
        assert_eq!(
            meta.citation_map.get("b").map(|entry| entry.number),
            Some(2)
        );
        assert_eq!(
            meta.citation_map.get("c").map(|entry| entry.number),
            Some(3)
        );
        assert_eq!(
            meta.bibliography
                .iter()
                .map(|entry| entry.source_id.clone())
                .collect::<Vec<_>>(),
            vec![source_id("a"), source_id("b"), source_id("c")]
        );
    }

    #[test]
    fn citation_map_and_bibliography_follow_first_appearance() {
        let mut doc = ReportDocument::new(doc_id("doc"));
        doc.sources
            .insert(source_id("b"), SourceEntry::new("Second"));
        doc.sources
            .insert(source_id("a"), SourceEntry::new("First"));
        doc.blocks
            .push(Block::paragraph(block_id("p1"), "Hello [^b]"));
        doc.blocks
            .push(Block::paragraph(block_id("p2"), "World [^a] [^b]"));
        let meta = compute_document_meta(&doc);
        assert_eq!(meta.citation_map["b"].number, 1);
        assert_eq!(meta.citation_map["a"].number, 2);
        assert_eq!(meta.bibliography.len(), 2);
        assert_eq!(meta.citation_map["a"].title, "First");
    }

    #[test]
    fn block_framing_labels_and_attribution_are_deterministic() {
        let mut doc = ReportDocument::new(doc_id("framing"));
        doc.sources.insert(
            source_id("sec10"),
            SourceEntry {
                authors: vec!["Jane Smith".into()],
                date: Some("2025-01-01".into()),
                ..SourceEntry::new("Annual filing")
            },
        );
        doc.references.style = crate::model::CitationStyle::Numeric;
        doc.blocks.push(
            Block::table(
                block_id("t1"),
                TableBlock {
                    title: Some("Revenue".into()),
                    numbering: None,
                    note: Some("Audited".into()),
                    columns: vec![ColumnNode::leaf("region", "Region")],
                    rows: vec![],
                    summary_rows: vec![],
                },
            )
            .with_sources([source_id("sec10")]),
        );
        if let Some(block) = doc.blocks.last_mut() {
            block.source_note = Some("Internal filing".into());
        }
        doc.blocks.push(Block::new(
            block_id("chart"),
            BlockKind::Echarts(crate::block::EchartsBlock {
                title: Some("Trend".into()),
                height: 300,
                option: serde_json::json!({"series": [{"type": "line", "data": [1]}]}),
                option_patch: None,
                fallback_summary: "Upward trend".into(),
                numbering: None,
                note: None,
            }),
        ));
        doc.blocks.push(Block::new(
            block_id("card"),
            BlockKind::CustomHtml(crate::block::CustomHtmlBlock {
                title: Some("KPI".into()),
                numbering: Some(NumberingKind::Figure),
                note: None,
                html: "<div>18%</div>".into(),
                css: None,
                fallback_text: "18%".into(),
            }),
        ));
        let meta = compute_document_meta(&doc);
        assert_eq!(meta.block_framing[&block_id("t1")].label, "表 1");
        assert_eq!(meta.block_framing[&block_id("chart")].label, "图 1");
        assert_eq!(meta.block_framing[&block_id("card")].label, "图 2");
        let attribution = meta.block_framing[&block_id("t1")]
            .attribution
            .as_deref()
            .unwrap();
        assert!(attribution.contains("资料来源：Internal filing [1]"));
        assert!(attribution.contains("注：Audited"));
    }

    #[test]
    fn chart_requirements_collect_maps_and_extensions() {
        let mut doc = ReportDocument::new(doc_id("charts"));
        doc.blocks.push(Block::new(
            block_id("maps"),
            BlockKind::Echarts(crate::block::EchartsBlock {
                title: None,
                height: 300,
                option: serde_json::json!({
                    "geo": [{"map": "china"}],
                    "series": [
                        {"type": "map", "map": "world", "data": [1]},
                        {"type": "wordCloud", "data": [1]},
                        {"type": "bar3D", "data": [1]}
                    ]
                }),
                option_patch: None,
                fallback_summary: "Maps and extensions.".into(),
                numbering: None,
                note: None,
            }),
        ));
        doc.blocks.push(Block::new(
            block_id("plain"),
            BlockKind::Echarts(crate::block::EchartsBlock {
                title: None,
                height: 300,
                option: serde_json::json!({
                    "series": [{"type": "bar", "data": [1]}]
                }),
                option_patch: None,
                fallback_summary: "Plain chart.".into(),
                numbering: None,
                note: None,
            }),
        ));
        let meta = compute_document_meta(&doc);
        let requirements = meta
            .chart_requirements
            .get(&block_id("maps"))
            .expect("map requirements");
        assert!(requirements.required_maps.contains("china"));
        assert!(requirements.required_maps.contains("world"));
        assert!(requirements.required_extensions.contains("wordCloud"));
        assert!(requirements.required_extensions.contains("bar3D"));
        assert!(!meta.chart_requirements.contains_key(&block_id("plain")));
    }

    #[test]
    fn chart_tables_match_markdown_projection() {
        let mut doc = ReportDocument::new(doc_id("charts"));
        doc.blocks.push(Block::new(
            block_id("chart"),
            BlockKind::Echarts(crate::block::EchartsBlock {
                title: Some("Quarterly revenue".into()),
                height: 300,
                option: serde_json::json!({
                    "xAxis": {"type": "category", "data": ["Q1", "Q2"]},
                    "series": [{"name": "Revenue", "type": "bar", "data": [120, 150]}]
                }),
                option_patch: None,
                fallback_summary: "Revenue grew.".into(),
                numbering: None,
                note: None,
            }),
        ));
        let meta = compute_document_meta(&doc);
        let tables = meta
            .chart_tables
            .get(&block_id("chart"))
            .expect("chart table");
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].title, "Revenue (bar)");
        assert_eq!(
            tables[0].headers,
            vec!["Category".to_string(), "Value".to_string()]
        );
        assert_eq!(
            tables[0].rows,
            vec![
                vec!["Q1".to_string(), "120".to_string()],
                vec!["Q2".to_string(), "150".to_string()]
            ]
        );

        let markdown = crate::project_markdown(&doc, &crate::MarkdownOptions::default()).unwrap();
        assert!(markdown.contains("_Chart data: Revenue (bar)_"));
        assert!(markdown.contains("| Q1 | 120 |"));
        assert!(markdown.contains("| Q2 | 150 |"));
    }

    #[test]
    fn toc_tree_is_nested_and_depth_filtered() {
        let mut doc = ReportDocument::new(doc_id("doc"));
        doc.toc.max_depth = 2;
        doc.blocks
            .push(Block::heading(block_id("intro"), 1, "Introduction"));
        doc.blocks
            .push(Block::heading(block_id("context"), 2, "Context"));
        doc.blocks.push(Block::heading(block_id("deep"), 3, "Deep"));
        doc.blocks.push(Block::heading(block_id("next"), 2, "Next"));
        doc.blocks
            .push(Block::heading(block_id("appendix"), 1, "Appendix"));
        let tree = compute_toc_tree(&doc);
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].text, "Introduction");
        assert_eq!(tree[0].number, "1");
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(tree[0].children[1].number, "1.2");
        assert_eq!(tree[1].text, "Appendix");
    }
}
