//! Two-pass citation resolution and bibliography generation.
//!
//! Pass 1 walks the document in reading order and assigns deterministic numbers by first
//! appearance. Pass 2 produces labels and the ordered bibliography. Inline citations are read
//! from paragraph/quote text; block-level citations come from [`crate::block::Block::source_ids`].

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::{
    block::BlockKind,
    ids::{BlockId, SourceId},
    inline::inline_citations,
    model::{ReferenceStyle, ReportDocument, SourceEntry},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CitationLocation {
    Inline,
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CitationUse {
    pub source_id: SourceId,
    pub block_id: BlockId,
    pub location: CitationLocation,
}

#[derive(Debug, Clone, Default)]
pub struct CitationScan {
    pub uses: Vec<CitationUse>,
    pub dangling: Vec<CitationUse>,
    pub cited_source_ids: Vec<SourceId>,
}

/// Scan every citation occurrence without resolving labels.
pub fn scan_document_citations(document: &ReportDocument) -> CitationScan {
    let mut scan = CitationScan::default();
    let mut seen = std::collections::BTreeSet::new();
    for block in &document.blocks {
        match &block.body {
            BlockKind::Paragraph { text } | BlockKind::Quote { text } => {
                for source_id in inline_citations(text) {
                    let usage = CitationUse {
                        source_id: source_id.clone(),
                        block_id: block.id.clone(),
                        location: CitationLocation::Inline,
                    };
                    if !document.sources.contains_key(&source_id) {
                        scan.dangling.push(usage.clone());
                    }
                    if seen.insert(source_id) {
                        scan.cited_source_ids.push(usage.source_id.clone());
                    }
                    scan.uses.push(usage);
                }
            }
            _ => {}
        }
        for source_id in &block.source_ids {
            let usage = CitationUse {
                source_id: source_id.clone(),
                block_id: block.id.clone(),
                location: CitationLocation::Block,
            };
            if !document.sources.contains_key(source_id) {
                scan.dangling.push(usage.clone());
            }
            if seen.insert(source_id.clone()) {
                scan.cited_source_ids.push(source_id.clone());
            }
            scan.uses.push(usage);
        }
    }
    scan
}

/// A fully resolved bibliography entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BibliographyEntry {
    pub source_id: SourceId,
    pub number: usize,
    /// Display label, for example `[1]` or `(Smith, 2024)`.
    pub label: String,
    pub source: SourceEntry,
    /// Plain-text citation line, useful for LLM digests and non-HTML consumers.
    pub plain_text: String,
}

#[derive(Debug, Clone, Default)]
pub struct CitationResolution {
    pub style: ReferenceStyle,
    /// Known sources in numeric first-appearance order.
    pub order: Vec<SourceId>,
    pub numbers: IndexMap<SourceId, usize>,
    pub labels: IndexMap<SourceId, String>,
    pub bibliography: Vec<BibliographyEntry>,
    pub dangling: Vec<CitationUse>,
    pub cited_source_ids: Vec<SourceId>,
}

impl CitationResolution {
    pub fn number(&self, source_id: &str) -> Option<usize> {
        self.numbers.get(source_id).copied()
    }

    pub fn label(&self, source_id: &str) -> Option<&str> {
        self.labels.get(source_id).map(String::as_str)
    }

    /// Return a display label even for an unknown source, which lets self-healing UIs show
    /// the offending ID rather than silently dropping it.
    pub fn label_or_missing(&self, source_id: &str) -> String {
        self.label(source_id)
            .map(str::to_string)
            .unwrap_or_else(|| "[?]".to_string())
    }

    pub fn block_labels(&self, source_ids: &[SourceId]) -> Vec<String> {
        source_ids
            .iter()
            .map(|source_id| self.label_or_missing(source_id))
            .collect()
    }
}

/// Resolve citations and build the ordered bibliography.
pub fn resolve_citations(document: &ReportDocument) -> CitationResolution {
    let scan = scan_document_citations(document);
    let mut resolution = CitationResolution {
        style: document.references.style,
        dangling: scan.dangling.clone(),
        cited_source_ids: scan.cited_source_ids.clone(),
        ..Default::default()
    };

    for source_id in &scan.cited_source_ids {
        let Some(source) = document.sources.get(source_id) else {
            continue;
        };
        let number = resolution.order.len() + 1;
        resolution.order.push(source_id.clone());
        resolution.numbers.insert(source_id.clone(), number);
        let _ = source;
    }

    let mut label_totals: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    let mut label_seen: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    if matches!(document.references.style, ReferenceStyle::AuthorYear) {
        for source_id in &resolution.order {
            if let Some(source) = document.sources.get(source_id) {
                *label_totals.entry(author_year_base(source)).or_default() += 1;
            }
        }
    }

    for source_id in &resolution.order {
        let Some(source) = document.sources.get(source_id) else {
            continue;
        };
        let Some(number) = resolution.numbers.get(source_id).copied() else {
            continue;
        };
        let label = match document.references.style {
            ReferenceStyle::Numeric => format!("[{number}]"),
            ReferenceStyle::AuthorYear => {
                let base = author_year_base(source);
                let total = label_totals.get(&base).copied().unwrap_or(1);
                let seen = label_seen.entry(base.clone()).or_default();
                let suffix = if total > 1 {
                    let letter = (b'a' + (*seen as u8).min(25)) as char;
                    letter.to_string()
                } else {
                    String::new()
                };
                *seen += 1;
                format!("({base}{suffix})")
            }
        };
        let plain_text = format_bibliography_plain(source_id, number, &label, source);
        resolution.labels.insert(source_id.clone(), label.clone());
        resolution.bibliography.push(BibliographyEntry {
            source_id: source_id.clone(),
            number,
            label,
            source: source.clone(),
            plain_text,
        });
    }

    resolution
}

fn author_year_base(source: &SourceEntry) -> String {
    let author = source
        .author_label()
        .or_else(|| {
            let title = source.title.trim();
            if title.is_empty() {
                None
            } else {
                Some(crate::text::truncate_chars(title, 28))
            }
        })
        .unwrap_or_else(|| "Untitled".to_string());
    let year = source.year().unwrap_or("n.d.");
    format!("{author}, {year}")
}

fn format_bibliography_plain(
    source_id: &SourceId,
    number: usize,
    label: &str,
    source: &SourceEntry,
) -> String {
    let mut parts = Vec::new();
    parts.push(label.to_string());
    if !source.authors.is_empty() {
        parts.push(format!("{}.", source.authors.join(", ")));
    }
    let mut title = source.title.trim().to_string();
    if !title.is_empty() {
        if !title.ends_with(['.', '?', '!']) {
            title.push('.');
        }
        parts.push(title);
    }
    let mut publication = Vec::new();
    if let Some(publisher) = source
        .publisher
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        publication.push(publisher.to_string());
    }
    if let Some(year) = source.year() {
        publication.push(year.to_string());
    }
    if !publication.is_empty() {
        parts.push(format!("{}.", publication.join(", ")));
    }
    if let Some(url) = source.url.as_deref().filter(|value| !value.is_empty()) {
        parts.push(url.to_string());
    }
    if let Some(accessed) = source.accessed.as_deref().filter(|value| !value.is_empty()) {
        parts.push(format!("accessed {accessed}."));
    }
    if let Some(note) = source.note.as_deref().filter(|value| !value.is_empty()) {
        parts.push(note.to_string());
    }
    let _ = (source_id, number);
    parts.join(" ")
}

/// Return the valid source IDs in insertion order, for self-healing error messages.
pub fn valid_source_ids(document: &ReportDocument) -> Vec<SourceId> {
    document.sources.keys().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use crate::{
        block::{Block, BlockKind},
        model::ReportDocument,
    };

    fn source(title: &str, author: &str, date: &str) -> SourceEntry {
        SourceEntry {
            authors: vec![author.to_string()],
            date: Some(date.to_string()),
            ..SourceEntry::new(title)
        }
    }

    #[test]
    fn numbering_follows_first_appearance_across_inline_and_block_citations() {
        let mut doc = ReportDocument::new(doc_id("report"));
        doc.sources
            .insert(source_id("c"), source("C", "Chen", "2024"));
        doc.sources
            .insert(source_id("a"), source("A", "Adams", "2023"));
        doc.sources
            .insert(source_id("b"), source("B", "Baker", "2022"));
        doc.blocks
            .push(Block::paragraph(block_id("p1"), "First [^a] and [^c]"));
        doc.blocks
            .push(Block::paragraph(block_id("p2"), "Again [^a].").with_sources([source_id("b")]));
        doc.blocks
            .push(Block::paragraph(block_id("p3"), "Missing [^nope]"));

        let resolved = resolve_citations(&doc);
        assert_eq!(
            resolved.order,
            vec![source_id("a"), source_id("c"), source_id("b")]
        );
        assert_eq!(resolved.number("a"), Some(1));
        assert_eq!(resolved.number("b"), Some(3));
        assert_eq!(resolved.labels["a"], "[1]");
        assert_eq!(resolved.dangling.len(), 1);
        assert_eq!(resolved.dangling[0].source_id, "nope");
        assert_eq!(resolved.bibliography.len(), 3);
        assert!(
            resolved.bibliography[0]
                .plain_text
                .starts_with("[1] Adams.")
        );
    }

    #[test]
    fn author_year_labels_disambiguate_same_author_and_year() {
        let mut doc = ReportDocument::new(doc_id("report"));
        doc.references.style = ReferenceStyle::AuthorYear;
        doc.sources
            .insert(source_id("one"), source("First", "Smith", "2024-01-01"));
        doc.sources
            .insert(source_id("two"), source("Second", "Smith", "2024-06-01"));
        doc.blocks
            .push(Block::paragraph(block_id("p"), "[^one] [^two]"));
        let resolved = resolve_citations(&doc);
        assert_eq!(resolved.labels["one"], "(Smith, 2024a)");
        assert_eq!(resolved.labels["two"], "(Smith, 2024b)");
    }

    #[test]
    fn unknown_sources_do_not_consume_numbers() {
        let mut doc = ReportDocument::new(doc_id("report"));
        doc.sources
            .insert(source_id("known"), source("Known", "Rivera", "2025"));
        doc.blocks
            .push(Block::paragraph(block_id("p"), "[^missing] [^known]"));
        let resolved = resolve_citations(&doc);
        assert_eq!(resolved.number("known"), Some(1));
        assert_eq!(resolved.label_or_missing("missing"), "[?]");
    }

    #[test]
    fn block_level_source_ids_are_scanned() {
        let mut doc = ReportDocument::new(doc_id("report"));
        doc.sources
            .insert(source_id("block-src"), source("Block", "Lee", "2020"));
        let table = crate::block::TableBlock {
            title: None,
            numbering: None,
            note: None,
            columns: vec![crate::block::ColumnNode::leaf("metric", "Metric")],
            rows: vec![],
            summary_rows: vec![],
        };
        doc.blocks
            .push(Block::table(block_id("t1"), table).with_sources([source_id("block-src")]));
        let resolved = resolve_citations(&doc);
        assert_eq!(
            resolved.block_labels(&[source_id("block-src")]),
            vec!["[1]"]
        );
    }

    #[test]
    fn quote_citations_are_supported() {
        let mut doc = ReportDocument::new(doc_id("report"));
        doc.sources
            .insert(source_id("q"), source("Quote", "Ng", "2021"));
        doc.blocks.push(Block::new(
            block_id("quote"),
            BlockKind::Quote {
                text: "A quote [^q]".into(),
            },
        ));
        let scan = scan_document_citations(&doc);
        assert_eq!(scan.uses.len(), 1);
        assert_eq!(scan.uses[0].block_id, "quote");
    }
}
