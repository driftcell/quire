//! Agent-facing Markdown projection.
//!
//! This is not a frontend renderer. It produces token-efficient Markdown with stable block
//! anchors so Lead and Reviewer agents can read, quote and patch exact blocks. Tables are
//! flattened to GFM, citations are expanded to footnote markers with source titles, and chart
//! series are projected into compact data tables for numeric verification.

use std::{collections::BTreeMap, fmt::Write as _};

use thiserror::Error;

use crate::{
    block::{Block, BlockKind, EchartsBlock, TableBlock},
    citation::resolve_citations,
    computed::{BlockFraming, compute_block_framing},
    echarts::{MAX_CHART_ROWS, chart_data_tables},
    ids::{BlockId, SourceId},
    inline::{InlineNode, parse_inline},
    json::merge_optional_json,
    model::{CitationStyle, ReportDocument},
    normalize::strip_duplicate_inner_title,
    table::{TableError, display_cell, validate_table},
    text::collapse_whitespace,
};

#[derive(Debug, Clone)]
pub struct MarkdownOptions {
    /// Expand ECharts series into Markdown data tables.
    pub include_chart_data: bool,
    /// Emit a small report header (ID, revision, title, cover metadata).
    pub include_front_matter: bool,
}

impl Default for MarkdownOptions {
    fn default() -> Self {
        Self {
            include_chart_data: true,
            include_front_matter: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Selection<'a> {
    All,
    Blocks(&'a [BlockId]),
    Section(&'a str),
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum MarkdownError {
    #[error("block `{block_id}` was not found. Valid block_ids: {valid_ids}")]
    BlockNotFound {
        block_id: BlockId,
        valid_ids: String,
    },
    #[error("section heading `{query}` was not found. Available headings: {headings}")]
    SectionNotFound { query: String, headings: String },
    #[error("table block `{block_id}` cannot be projected: {source}")]
    InvalidTable {
        block_id: BlockId,
        #[source]
        source: TableError,
    },
    #[error("no blocks matched the requested slice")]
    EmptySelection,
}

/// Project a whole report.
pub fn project_markdown(
    document: &ReportDocument,
    options: &MarkdownOptions,
) -> Result<String, MarkdownError> {
    MarkdownProjector::new(document, options).render()
}

/// Project specific block IDs, preserving document order.
pub fn project_blocks(
    document: &ReportDocument,
    block_ids: &[BlockId],
    options: &MarkdownOptions,
) -> Result<String, MarkdownError> {
    MarkdownProjector::new(document, options).render_blocks(block_ids)
}

/// Project one section starting at the first matching heading.
pub fn project_section(
    document: &ReportDocument,
    heading: &str,
    options: &MarkdownOptions,
) -> Result<String, MarkdownError> {
    MarkdownProjector::new(document, options).render_section(heading)
}

/// Stateful projector. Citation resolution happens once per document.
pub struct MarkdownProjector<'a> {
    document: &'a ReportDocument,
    options: MarkdownOptions,
    framing: BTreeMap<BlockId, BlockFraming>,
}

impl<'a> MarkdownProjector<'a> {
    pub fn new(document: &'a ReportDocument, options: &MarkdownOptions) -> Self {
        let citations = resolve_citations(document);
        Self {
            document,
            options: options.clone(),
            framing: compute_block_framing(document, &citations),
        }
    }

    pub fn render(&self) -> Result<String, MarkdownError> {
        self.render_selection(Selection::All)
    }

    pub fn render_blocks(&self, block_ids: &[BlockId]) -> Result<String, MarkdownError> {
        self.render_selection(Selection::Blocks(block_ids))
    }

    pub fn render_section(&self, heading: &str) -> Result<String, MarkdownError> {
        self.render_selection(Selection::Section(heading))
    }

    fn render_selection(&self, selection: Selection<'_>) -> Result<String, MarkdownError> {
        let selected_indices = self.select_indices(selection)?;
        if selected_indices.is_empty() {
            return Err(MarkdownError::EmptySelection);
        }
        let mut output = String::new();
        if self.options.include_front_matter {
            self.write_front_matter(&mut output);
        }
        for (position, index) in selected_indices.iter().enumerate() {
            if let Some(block) = self.document.blocks.get(*index) {
                self.write_block(&mut output, block)?;
            }
            if position + 1 < selected_indices.len() {
                output.push('\n');
            }
        }
        Ok(output.trim_end().to_string() + "\n")
    }

    fn select_indices(&self, selection: Selection<'_>) -> Result<Vec<usize>, MarkdownError> {
        match selection {
            Selection::All => Ok((0..self.document.blocks.len()).collect()),
            Selection::Blocks(block_ids) => {
                let mut selected = Vec::new();
                for block_id in block_ids {
                    let Some(index) = self.document.position(block_id.as_str()) else {
                        return Err(MarkdownError::BlockNotFound {
                            block_id: (*block_id).clone(),
                            valid_ids: block_id_list(self.document),
                        });
                    };
                    selected.push(index);
                }
                selected.sort_unstable();
                selected.dedup();
                Ok(selected)
            }
            Selection::Section(query) => {
                let query_lower = query.trim().to_lowercase();
                let Some(start) = self.document.blocks.iter().position(|block| {
                    if let BlockKind::Heading { text, .. } = &block.body {
                        crate::inline::inline_plain_text(text)
                            .to_lowercase()
                            .contains(&query_lower)
                    } else {
                        false
                    }
                }) else {
                    return Err(MarkdownError::SectionNotFound {
                        query: query.to_string(),
                        headings: heading_list(self.document),
                    });
                };
                let level = match self.document.blocks.get(start).map(|block| &block.body) {
                    Some(BlockKind::Heading { level, .. }) => *level,
                    _ => 1,
                };
                let mut indices = vec![start];
                for (index, block) in self.document.blocks.iter().enumerate().skip(start + 1) {
                    if let BlockKind::Heading {
                        level: next_level, ..
                    } = &block.body
                        && *next_level <= level
                    {
                        break;
                    }
                    indices.push(index);
                }
                Ok(indices)
            }
        }
    }

    fn write_front_matter(&self, output: &mut String) {
        let _ = writeln!(
            output,
            "<!-- report:{} | revision:{} -->",
            self.document.doc_id, self.document.revision
        );
        let title = self.document.title();
        let _ = writeln!(output, "# {title}\n");
        if let Some(subtitle) = self
            .document
            .cover
            .subtitle
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            let _ = writeln!(output, "> {subtitle}\n");
        }
        if !self.document.cover.authors.is_empty() {
            let mut line = format!("_{}_", self.document.cover.authors.join(" · "));
            if let Some(date) = self
                .document
                .cover
                .date
                .as_deref()
                .filter(|value| !value.is_empty())
            {
                line.push_str(&format!(" · {date}"));
            }
            let _ = writeln!(output, "{line}\n");
        }
    }

    fn write_block(&self, output: &mut String, block: &Block) -> Result<(), MarkdownError> {
        let _ = writeln!(
            output,
            "<!-- block:{} | type:{} -->",
            block.id,
            block.block_type()
        );
        match &block.body {
            BlockKind::Heading { level, text } => {
                let level = (*level).clamp(1, 6);
                let _ = writeln!(
                    output,
                    "{} {}\n",
                    "#".repeat(usize::from(level)),
                    self.render_inline(text)
                );
            }
            BlockKind::Paragraph { text } => {
                let rendered = self.render_inline(text);
                output.push_str(&rendered);
                output.push_str("\n\n");
            }
            BlockKind::Quote { text } => {
                let rendered = self.render_inline(text);
                for line in rendered.lines() {
                    let _ = writeln!(output, "> {line}");
                }
                output.push('\n');
            }
            BlockKind::Table(table) => {
                self.write_table(output, block, table)?;
            }
            BlockKind::Echarts(chart) => {
                self.write_chart(output, block, chart);
            }
            BlockKind::CustomHtml(html) => {
                if let Some(framing) = self.framing.get(&block.id) {
                    if let Some(title) = html
                        .title
                        .as_deref()
                        .filter(|title| !title.trim().is_empty())
                    {
                        let _ = writeln!(output, "**{} — {title}**\n", framing.label);
                    } else {
                        let _ = writeln!(output, "**{}**\n", framing.label);
                    }
                } else if let Some(title) = html
                    .title
                    .as_deref()
                    .filter(|title| !title.trim().is_empty())
                {
                    let _ = writeln!(output, "**{title}**\n");
                }
                output.push_str("> _Custom HTML block; agent-facing fallback text:_\n>\n");
                for line in html.fallback_text.lines() {
                    let _ = writeln!(output, "> {line}");
                }
                if let Some(attribution) = self
                    .framing
                    .get(&block.id)
                    .and_then(|framing| framing.attribution.as_deref())
                {
                    let _ = writeln!(output, "\n_{attribution}_");
                }
                output.push('\n');
            }
        }
        self.write_block_sources(output, block);
        Ok(())
    }

    fn write_table(
        &self,
        output: &mut String,
        block: &Block,
        table: &TableBlock,
    ) -> Result<(), MarkdownError> {
        let shape = validate_table(table).map_err(|source| MarkdownError::InvalidTable {
            block_id: block.id.clone(),
            source,
        })?;
        let framing = self.framing.get(&block.id);
        match (
            framing.map(|framing| framing.label.as_str()),
            table
                .title
                .as_deref()
                .filter(|title| !title.trim().is_empty()),
        ) {
            (Some(label), Some(title)) => {
                let _ = writeln!(output, "**{label} — {title}**\n");
            }
            (Some(label), None) => {
                let _ = writeln!(output, "**{label}**\n");
            }
            (None, Some(title)) => {
                let _ = writeln!(output, "**{title}**\n");
            }
            (None, None) => {}
        }
        output.push('|');
        for leaf in &shape.leaves {
            let _ = write!(output, " {} |", escape_table_cell(&leaf.flattened_label()));
        }
        output.push('\n');
        output.push('|');
        for leaf in &shape.leaves {
            let alignment = match leaf.align {
                crate::block::Align::Left => ":---",
                crate::block::Align::Center => ":---:",
                crate::block::Align::Right => "---:",
            };
            let _ = write!(output, " {alignment} |");
        }
        output.push('\n');
        for row in &table.rows {
            output.push('|');
            for leaf in &shape.leaves {
                let cell = row
                    .get(&leaf.key)
                    .map(|value| display_cell(value, leaf.data_type))
                    .unwrap_or_default();
                let _ = write!(output, " {} |", escape_table_cell(&cell));
            }
            output.push('\n');
        }
        if !table.summary_rows.is_empty() {
            for row in &table.summary_rows {
                output.push('|');
                for leaf in &shape.leaves {
                    let cell = row
                        .get(&leaf.key)
                        .map(|value| display_cell(value, leaf.data_type))
                        .unwrap_or_default();
                    let _ = write!(output, " **{}** |", escape_table_cell(&cell));
                }
                output.push('\n');
            }
        }
        if let Some(attribution) = framing.and_then(|framing| framing.attribution.as_deref()) {
            let _ = writeln!(output, "_{attribution}_\n");
        }
        output.push('\n');
        Ok(())
    }

    fn write_chart(&self, output: &mut String, block: &Block, chart: &EchartsBlock) {
        if let Some(framing) = self.framing.get(&block.id) {
            if let Some(title) = chart
                .title
                .as_deref()
                .filter(|title| !title.trim().is_empty())
            {
                let _ = writeln!(output, "**{} — {title}**\n", framing.label);
            } else {
                let _ = writeln!(output, "**{}**\n", framing.label);
            }
        } else if let Some(title) = chart
            .title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
        {
            let _ = writeln!(output, "**Chart: {title}**\n");
        }
        let summary = chart.fallback_summary.trim();
        if !summary.is_empty() {
            let _ = writeln!(output, "{summary}\n");
        }
        if !self.options.include_chart_data {
            return;
        }
        let option = effective_chart_option(chart);
        let tables = chart_data_tables(&option);
        if tables.is_empty() {
            output.push_str("_No chart series data supplied; use the fallback summary above._\n\n");
        } else {
            for table in &tables {
                let _ = writeln!(output, "_Chart data: {}_", table.title);
                write_markdown_table(output, &table.headers, &table.rows);
            }
        }
        if let Some(attribution) = self
            .framing
            .get(&block.id)
            .and_then(|framing| framing.attribution.as_deref())
        {
            let _ = writeln!(output, "_{attribution}_\n");
        }
    }

    fn write_block_sources(&self, output: &mut String, block: &Block) {
        if block.source_ids.is_empty() {
            return;
        }
        let markers: Vec<String> = block
            .source_ids
            .iter()
            .map(|source_id| self.citation_marker(source_id))
            .collect();
        let _ = writeln!(output, "Sources: {}\n", markers.join(", "));
    }

    fn citation_marker(&self, source_id: &str) -> String {
        let title = self
            .document
            .sources
            .get(source_id)
            .map(|entry| entry.title.as_str())
            .unwrap_or("UNRESOLVED SOURCE");
        format!("[^{source_id}: {}]", escape_footnote(title))
    }

    fn render_inline(&self, text: &str) -> String {
        render_inline_nodes(&parse_inline(text), self.document)
    }
}

/// Compute the effective option agents describe. Preset templates are a frontend concern; data
/// supplied through `option` and `option_patch` is merged here so reviewers can inspect it.
pub fn effective_chart_option(chart: &EchartsBlock) -> serde_json::Value {
    let mut value = chart
        .option
        .clone()
        .unwrap_or_else(|| serde_json::Value::Object(Default::default()));
    if let Some(patch) = &chart.option_patch {
        value = merge_optional_json(Some(&value), patch);
    }
    let has_outer_title = chart
        .title
        .as_deref()
        .is_some_and(|title| !title.trim().is_empty());
    let _ = strip_duplicate_inner_title(&mut value, has_outer_title);
    value
}

fn write_markdown_table(output: &mut String, header: &[String], body: &[Vec<String>]) {
    output.push('|');
    for cell in header {
        let _ = write!(output, " {} |", escape_table_cell(cell));
    }
    output.push('\n');
    output.push('|');
    for index in 0..header.len() {
        output.push_str(if index == 0 { " :--- |" } else { " ---: |" });
    }
    output.push('\n');
    for row in body.iter().take(MAX_CHART_ROWS) {
        output.push('|');
        for (index, _) in header.iter().enumerate() {
            let cell = row.get(index).map(String::as_str).unwrap_or_default();
            let _ = write!(output, " {} |", escape_table_cell(cell));
        }
        output.push('\n');
    }
    if body.len() > MAX_CHART_ROWS {
        let mut cells = vec![String::new(); header.len()];
        if let Some(first) = cells.first_mut() {
            *first = "_truncated_".to_string();
        }
        if let Some(second) = cells.get_mut(1) {
            *second = format!("{} of {} rows shown", MAX_CHART_ROWS, body.len());
        }
        output.push('|');
        for cell in &cells {
            let _ = write!(output, " {} |", escape_table_cell(cell));
        }
        output.push('\n');
    }
    output.push('\n');
}

fn render_inline_nodes(nodes: &[InlineNode], document: &ReportDocument) -> String {
    let mut output = String::new();
    for node in nodes {
        match node {
            InlineNode::Text(text) => output.push_str(text),
            InlineNode::Strong(children) => {
                output.push_str("**");
                output.push_str(&render_inline_nodes(children, document));
                output.push_str("**");
            }
            InlineNode::Em(children) => {
                output.push('*');
                output.push_str(&render_inline_nodes(children, document));
                output.push('*');
            }
            InlineNode::Code(text) => {
                let fence = if text.contains('`') { "``" } else { "`" };
                output.push_str(fence);
                output.push_str(text);
                output.push_str(fence);
            }
            InlineNode::Link { href, children } => {
                output.push('[');
                output.push_str(&render_inline_nodes(children, document));
                output.push_str("](");
                output.push_str(href);
                output.push(')');
            }
            InlineNode::Citation(source_id) => {
                let title = document
                    .sources
                    .get(source_id)
                    .map(|entry| entry.title.as_str())
                    .unwrap_or("UNRESOLVED SOURCE");
                output.push_str(&format!("[^{source_id}: {}]", escape_footnote(title)));
            }
            InlineNode::Break => output.push_str("  \n"),
        }
    }
    output
}

fn escape_footnote(input: &str) -> String {
    input.replace(']', "\\]").replace('\n', " ")
}

fn escape_table_cell(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace(['\r', '\n'], "<br>")
}

fn block_id_list(document: &ReportDocument) -> String {
    if document.blocks.is_empty() {
        "(none)".to_string()
    } else {
        document
            .blocks
            .iter()
            .map(|block| block.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn heading_list(document: &ReportDocument) -> String {
    let headings: Vec<String> = document
        .blocks
        .iter()
        .filter_map(|block| match &block.body {
            BlockKind::Heading { level, text } => Some(format!(
                "h{level} {} ({})",
                collapse_whitespace(&crate::inline::inline_plain_text(text)),
                block.id
            )),
            _ => None,
        })
        .collect();
    if headings.is_empty() {
        "(none)".to_string()
    } else {
        headings.join("; ")
    }
}

/// Return the citation style label used in projected references. Kept for API symmetry.
pub fn citation_style_label(style: CitationStyle) -> &'static str {
    match style {
        CitationStyle::Numeric => "numeric",
        CitationStyle::AuthorYear => "author_year",
    }
}

/// Resolve citation order for callers that need numbering metadata alongside Markdown.
pub fn citation_numbers(document: &ReportDocument) -> std::collections::BTreeMap<SourceId, usize> {
    resolve_citations(document).numbers.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use crate::{
        block::{Block, CellValue, ColumnDataType, ColumnNode},
        model::{ReportDocument, SourceEntry},
    };
    use std::collections::BTreeMap;

    fn sample() -> ReportDocument {
        let mut doc = ReportDocument::new(doc_id("md-report"));
        doc.revision = revision(4);
        doc.cover.title = "Q3 Review".into();
        doc.cover.subtitle = Some("Momentum".into());
        doc.cover.authors = vec!["Ada Lovelace".into()];
        doc.cover.date = Some("2026-10-01".into());
        doc.sources
            .insert(source_id("sec10"), SourceEntry::new("Annual filing 10-K"));
        doc.blocks
            .push(Block::heading(block_id("intro"), 1, "Introduction"));
        doc.blocks.push(
            Block::paragraph(
                block_id("summary"),
                "Revenue grew **18%** [^sec10] with `automation` momentum.",
            )
            .with_sources([source_id("sec10")]),
        );
        let mut row1 = BTreeMap::new();
        row1.insert("region".to_string(), CellValue::text("North America"));
        row1.insert("q1".to_string(), CellValue::Integer(100));
        row1.insert("q2".to_string(), CellValue::Integer(120));
        let mut row2 = BTreeMap::new();
        row2.insert("region".to_string(), CellValue::text("EMEA"));
        row2.insert("q1".to_string(), CellValue::Integer(80));
        row2.insert("q2".to_string(), CellValue::Integer(95));
        let mut summary = BTreeMap::new();
        summary.insert("region".to_string(), CellValue::text("Total"));
        summary.insert("q1".to_string(), CellValue::Integer(180));
        summary.insert("q2".to_string(), CellValue::Integer(215));
        doc.blocks.push(Block::table(
            block_id("financials"),
            TableBlock {
                title: Some("Revenue by region".into()),
                numbering: None,
                note: None,
                columns: vec![
                    ColumnNode::leaf("region", "Region"),
                    ColumnNode::group(
                        "revenue",
                        "Revenue",
                        vec![
                            ColumnNode::leaf("q1", "Q1").with_type(ColumnDataType::Currency),
                            ColumnNode::leaf("q2", "Q2").with_type(ColumnDataType::Currency),
                        ],
                    ),
                ],
                rows: vec![row1, row2],
                summary_rows: vec![summary],
            },
        ));
        doc.blocks.push(Block::new(
            block_id("chart"),
            BlockKind::Echarts(EchartsBlock {
                title: Some("Revenue trend".into()),
                height: 320,
                mode: crate::block::EchartsMode::Preset,
                preset: Some(crate::block::EchartsPreset::Bar),
                option: Some(serde_json::json!({
                    "xAxis": {"data": ["Q1", "Q2"]},
                    "series": [{"name": "North America", "type": "bar", "data": [100, 120]}]
                })),
                option_patch: None,
                fallback_summary: "Revenue rose from 100 to 120.".into(),
                numbering: None,
                note: None,
            }),
        ));
        doc.blocks
            .push(Block::heading(block_id("appendix"), 1, "Appendix"));
        doc.blocks
            .push(Block::paragraph(block_id("notes"), "Appendix notes"));
        doc
    }

    #[test]
    fn full_projection_has_anchors_expanded_citations_and_flattened_tables() {
        let output = project_markdown(&sample(), &MarkdownOptions::default()).unwrap();
        assert!(output.contains("<!-- report:md-report | revision:4 -->"));
        assert!(output.contains("<!-- block:summary | type:paragraph -->"));
        assert!(output.contains("[^sec10: Annual filing 10-K]"));
        assert!(output.contains("**表 1 — Revenue by region**"));
        assert!(output.contains("| Region | Revenue / Q1 | Revenue / Q2 |"));
        assert!(output.contains("| :--- | ---: | ---: |"));
        assert!(output.contains("| **Total** | **180** | **215** |"));
        assert!(output.contains("<!-- block:chart | type:echarts -->"));
        assert!(output.contains("**图 1 — Revenue trend**"));
        assert!(output.contains("| Q1 | 100 |"));
    }

    #[test]
    fn chart_data_can_be_omitted() {
        let options = MarkdownOptions {
            include_chart_data: false,
            ..Default::default()
        };
        let output = project_markdown(&sample(), &options).unwrap();
        assert!(output.contains("Revenue rose from 100 to 120."));
        assert!(!output.contains("_Chart data:"));
    }

    fn chart_doc(option: serde_json::Value) -> ReportDocument {
        let mut doc = ReportDocument::new(doc_id("chart-report"));
        doc.blocks.push(Block::new(
            block_id("chart"),
            BlockKind::Echarts(EchartsBlock {
                title: Some("Chart".into()),
                height: 320,
                mode: crate::block::EchartsMode::Option,
                preset: None,
                option: Some(option),
                option_patch: None,
                fallback_summary: "Fallback summary.".into(),
                numbering: None,
                note: None,
            }),
        ));
        doc
    }

    fn project_chart(option: serde_json::Value) -> String {
        project_markdown(&chart_doc(option), &MarkdownOptions::default()).unwrap()
    }

    #[test]
    fn projects_dataset_arrays_and_object_rows() {
        let output = project_chart(serde_json::json!({
            "dataset": {"source": [
                ["Quarter", "Revenue", "Margin"],
                ["Q1", 120, 62.5],
                ["Q2", 150, 64.0]
            ]},
            "series": [{"type": "bar", "datasetIndex": 0}]
        }));
        assert!(output.contains("_Chart data: Dataset 1 (dataset)_"));
        assert!(output.contains("| Quarter | Revenue | Margin |"));
        assert!(output.contains("| Q1 | 120 | 62.5 |"));

        let output = project_chart(serde_json::json!({
            "dataset": [
                {"id": "kpi", "source": [
                    {"region": "EMEA", "value": 10},
                    {"region": "APAC", "value": 20}
                ]},
                {"id": "extra", "source": [
                    ["Metric", "Value"],
                    ["NPS", 42]
                ]}
            ],
            "series": [{"type": "bar", "datasetIndex": 0}]
        }));
        assert!(output.contains("_Chart data: kpi (dataset)_"));
        assert!(output.contains("| region | value |"));
        assert!(output.contains("| EMEA | 10 |"));
        assert!(output.contains("_Chart data: extra (dataset)_"));
        assert!(output.contains("| NPS | 42 |"));
    }

    #[test]
    fn projects_multi_series_cartesian_table() {
        let output = project_chart(serde_json::json!({
            "xAxis": {"data": ["Q1", "Q2"]},
            "series": [
                {"name": "Revenue", "type": "bar", "data": [120, 150]},
                {"name": "Margin", "type": "line", "data": [62.5, 64.0]}
            ]
        }));
        assert!(output.contains("_Chart data: Revenue, Margin (bar, line)_"));
        assert!(output.contains("| Category | Revenue | Margin |"));
        assert!(output.contains("| Q1 | 120 | 62.5 |"));
        assert!(output.contains("| Q2 | 150 | 64 |"));
    }

    #[test]
    fn projects_relation_charts_as_source_target_value() {
        let output = project_chart(serde_json::json!({
            "series": [
                {
                    "name": "Flow",
                    "type": "sankey",
                    "data": [{"name": "A"}, {"name": "B"}],
                    "links": [
                        {"source": "A", "target": "B", "value": 5},
                        {"source": "B", "target": "C", "value": 3}
                    ]
                },
                {
                    "name": "Network",
                    "type": "graph",
                    "data": [{"name": "X"}, {"name": "Y"}],
                    "edges": [{"from": "X", "to": "Y", "value": 9}]
                }
            ]
        }));
        assert!(output.contains("_Chart data: Flow (sankey links)_"));
        assert!(output.contains("| Source | Target | Value |"));
        assert!(output.contains("| A | B | 5 |"));
        assert!(output.contains("| B | C | 3 |"));
        assert!(output.contains("_Chart data: Network (graph links)_"));
        assert!(output.contains("| X | Y | 9 |"));
    }

    #[test]
    fn projects_radar_indicators_as_columns() {
        let output = project_chart(serde_json::json!({
            "radar": {"indicator": [{"name": "Revenue"}, {"name": "Margin"}]},
            "series": [{
                "name": "Plan",
                "type": "radar",
                "data": [
                    {"name": "Actual", "value": [10, 20]},
                    {"name": "Target", "value": [12, 18]}
                ]
            }]
        }));
        assert!(output.contains("_Chart data: Plan (radar)_"));
        assert!(output.contains("| Series | Revenue | Margin |"));
        assert!(output.contains("| Actual | 10 | 20 |"));
        assert!(output.contains("| Target | 12 | 18 |"));
    }

    #[test]
    fn projects_hierarchy_charts_with_paths() {
        let output = project_chart(serde_json::json!({
            "series": [{
                "name": "Segments",
                "type": "treemap",
                "data": [{
                    "name": "Region",
                    "children": [
                        {"name": "EMEA", "children": [{"name": "UK", "value": 6}]},
                        {"name": "APAC", "value": 20}
                    ]
                }]
            }]
        }));
        assert!(output.contains("_Chart data: Segments (treemap)_"));
        assert!(output.contains("| Path | Value |"));
        assert!(output.contains("| Region / EMEA / UK | 6 |"));
        assert!(output.contains("| Region / APAC | 20 |"));
    }

    #[test]
    fn projects_multidimensional_series_dimensions() {
        let output = project_chart(serde_json::json!({
            "xAxis": {"data": ["2026-01", "2026-02"]},
            "series": [{
                "name": "OHLC",
                "type": "candlestick",
                "data": [[10, 12, 9, 13], [12, 13, 11, 14]]
            }]
        }));
        assert!(output.contains("| Category | Open | Close | Low | High |"));
        assert!(output.contains("| 2026-01 | 10 | 12 | 9 | 13 |"));

        let output = project_chart(serde_json::json!({
            "series": [{
                "name": "Heat",
                "type": "heatmap",
                "data": [[0, 0, 5], [0, 1, 8]]
            }]
        }));
        assert!(output.contains("| X | Y | Value |"));
        assert!(output.contains("| 0 | 1 | 8 |"));

        let output = project_chart(serde_json::json!({
            "series": [{
                "name": "Cloud",
                "type": "scatter",
                "data": [[1, 2, 3]]
            }]
        }));
        assert!(output.contains("| X | Y | Value 3 |"));
        assert!(output.contains("| 1 | 2 | 3 |"));
    }

    #[test]
    fn section_projection_stops_at_same_level_heading() {
        let output =
            project_section(&sample(), "introduction", &MarkdownOptions::default()).unwrap();
        assert!(output.contains("block:intro"));
        assert!(output.contains("block:chart"));
        assert!(!output.contains("block:appendix"));
    }

    #[test]
    fn block_projection_preserves_document_order_and_missing_ids_self_heal() {
        let output = project_blocks(
            &sample(),
            &[block_id("chart"), block_id("summary")],
            &MarkdownOptions {
                include_front_matter: false,
                ..Default::default()
            },
        )
        .unwrap();
        let summary_pos = output.find("block:summary").unwrap();
        let chart_pos = output.find("block:chart").unwrap();
        assert!(summary_pos < chart_pos);
        let error = project_blocks(
            &sample(),
            &[block_id("missing")],
            &MarkdownOptions::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("Valid block_ids"));
        assert!(error.to_string().contains("summary"));
    }

    #[test]
    fn missing_section_lists_headings() {
        let error = project_section(&sample(), "nope", &MarkdownOptions::default()).unwrap_err();
        assert!(matches!(error, MarkdownError::SectionNotFound { .. }));
        assert!(error.to_string().contains("Introduction"));
    }
}
