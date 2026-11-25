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
    model::{CitationStyle, ReportDocument},
    normalize::strip_duplicate_inner_title,
    table::{TableError, compute_column_decimals, display_cell_aligned, validate_table},
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
    SectionAt(usize, u8),
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
    #[error("`{query}` matches multiple headings; pass an exact section_id: {ids}")]
    AmbiguousSection { query: String, ids: String },
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

/// Project a section using the legacy heading selector.
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
        let index = find_section_heading_start(self.document, heading)?;
        self.render_heading_at(index, heading)
    }

    /// The agent-facing path takes an exact heading ID from the digest; never fall back to text.
    pub fn render_section_by_id(&self, id: &BlockId) -> Result<String, MarkdownError> {
        self.render_heading_at(self.document.position(id), id)
    }

    fn render_heading_at(
        &self,
        index: Option<usize>,
        query: &str,
    ) -> Result<String, MarkdownError> {
        if let Some(start) = index
            && let Some(Block {
                body: BlockKind::Heading { level, .. },
                ..
            }) = self.document.blocks.get(start)
        {
            return self.render_selection(Selection::SectionAt(start, *level));
        }
        Err(MarkdownError::SectionNotFound {
            query: query.to_string(),
            headings: heading_list(self.document),
        })
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
        let mut cited_sources: Vec<String> = Vec::new();
        for (position, index) in selected_indices.iter().enumerate() {
            if let Some(block) = self.document.blocks.get(*index) {
                self.write_block(&mut output, block, &mut cited_sources)?;
            }
            if position + 1 < selected_indices.len() {
                output.push('\n');
            }
        }
        self.write_section_sources(&mut output, &cited_sources);
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
            Selection::SectionAt(start, level) => {
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
            "@report {} rev={}",
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

    fn write_block(
        &self,
        output: &mut String,
        block: &Block,
        cited_sources: &mut Vec<String>,
    ) -> Result<(), MarkdownError> {
        let (inline_ids, block_only_ids) = self.collect_block_sources(block);
        for id in inline_ids.iter().chain(block_only_ids.iter()) {
            if !cited_sources.iter().any(|existing| existing == id) {
                cited_sources.push(id.clone());
            }
        }
        let short_type = match &block.body {
            BlockKind::Heading { level, .. } => match level {
                1 => "h1",
                2 => "h2",
                3 => "h3",
                4 => "h4",
                5 => "h5",
                _ => "h6",
            },
            BlockKind::Paragraph { .. } => "p",
            BlockKind::Quote { .. } => "quote",
            BlockKind::Table(_) => "tbl",
            BlockKind::Echarts(_) => "chart",
            BlockKind::CustomHtml(_) => "html",
        };
        if block_only_ids.is_empty() {
            let _ = writeln!(output, "@block {} [{short_type}]", block.id);
        } else {
            let markers: Vec<String> = block_only_ids.iter().map(|id| format!("^{id}")).collect();
            let _ = writeln!(
                output,
                "@block {} [{short_type}] {}",
                block.id,
                markers.join(" ")
            );
        }
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
        let column_decimals = compute_column_decimals(table, &shape.leaves);
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
        for _ in &shape.leaves {
            output.push_str(" --- |");
        }
        output.push('\n');
        for row in &table.rows {
            output.push('|');
            for leaf in &shape.leaves {
                let min_decimals = column_decimals.get(&leaf.key).copied().unwrap_or(0);
                let cell = row
                    .get(&leaf.key)
                    .map(|value| display_cell_aligned(value, leaf.data_type, min_decimals))
                    .unwrap_or_default();
                let _ = write!(output, " {} |", escape_table_cell(&cell));
            }
            output.push('\n');
        }
        if !table.summary_rows.is_empty() {
            for row in &table.summary_rows {
                output.push('|');
                for leaf in &shape.leaves {
                    let min_decimals = column_decimals.get(&leaf.key).copied().unwrap_or(0);
                    let cell = row
                        .get(&leaf.key)
                        .map(|value| display_cell_aligned(value, leaf.data_type, min_decimals))
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
        let option = markdown_chart_option(chart);
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

    fn collect_block_sources(&self, block: &Block) -> (Vec<String>, Vec<String>) {
        let mut inline_ids: Vec<String> = Vec::new();
        let mut push_inline = |id: &str| {
            if !inline_ids.iter().any(|existing| existing == id) {
                inline_ids.push(id.to_string());
            }
        };
        match &block.body {
            BlockKind::Paragraph { text } | BlockKind::Quote { text } => {
                for id in crate::inline::inline_citations(text) {
                    push_inline(id.as_str());
                }
            }
            BlockKind::Table(table) => {
                if let Some(note) = table.note.as_deref() {
                    for id in crate::inline::inline_citations(note) {
                        push_inline(id.as_str());
                    }
                }
            }
            BlockKind::Echarts(chart) => {
                if let Some(note) = chart.note.as_deref() {
                    for id in crate::inline::inline_citations(note) {
                        push_inline(id.as_str());
                    }
                }
            }
            BlockKind::CustomHtml(html) => {
                if let Some(note) = html.note.as_deref() {
                    for id in crate::inline::inline_citations(note) {
                        push_inline(id.as_str());
                    }
                }
            }
            BlockKind::Heading { .. } => {}
        }
        if let Some(source_note) = block.source_note.as_deref() {
            for id in crate::inline::inline_citations(source_note) {
                push_inline(id.as_str());
            }
        }
        let mut block_only_ids: Vec<String> = Vec::new();
        for id in &block.source_ids {
            let s = id.as_str();
            if !inline_ids.iter().any(|existing| existing == s)
                && !block_only_ids.iter().any(|existing| existing == s)
            {
                block_only_ids.push(s.to_string());
            }
        }
        (inline_ids, block_only_ids)
    }

    fn write_section_sources(&self, output: &mut String, cited_sources: &[String]) {
        if cited_sources.is_empty() {
            return;
        }
        if !output.ends_with("\n\n") {
            if !output.ends_with('\n') {
                output.push('\n');
            }
            output.push('\n');
        }
        output.push_str("## Sources\n");
        for source_id in cited_sources {
            let title = self
                .document
                .sources
                .get(source_id.as_str())
                .map(|entry| entry.title.as_str())
                .unwrap_or("UNRESOLVED SOURCE");
            let _ = writeln!(output, "- [^{source_id}]: {}", escape_footnote(title));
        }
    }

    fn render_inline(&self, text: &str) -> String {
        render_inline_nodes(&parse_inline(text), self.document)
    }
}

/// Compute the effective ECharts option agents describe. The native `option` is the single
/// source of truth and `option_patch` is merged here so reviewers can inspect the result.
mod citations;
mod sections;
mod tables;
#[cfg(test)]
mod tests;

pub use citations::*;
pub(crate) use sections::{find_section_heading_start, heading_list};
pub(crate) use tables::{
    block_id_list, escape_footnote, escape_table_cell, markdown_chart_option, render_inline_nodes,
    write_markdown_table,
};
