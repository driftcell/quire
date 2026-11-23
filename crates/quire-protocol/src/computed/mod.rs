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
mod tables;
#[cfg(test)]
mod tests;

pub use tables::*;
