//! Per-call, block-indexed precomputation shared by all lint passes.

use quire_protocol::{
    BlockKind, CitationScan, ReportDocument, TableError, TableShape, effective_chart_option,
    scan_document_citations, validate_table,
};
use rayon::prelude::*;
use serde_json::Value;

use crate::rules::prose::masking::{
    has_book_title, has_inline_citation, has_list_structure, mask_text, masked_plain_char_count,
    strip_wall_markup, stripped_wall_sentence_count,
};

pub(crate) const PARALLEL_BLOCK_THRESHOLD: usize = 40;

pub(crate) struct ParagraphPrecomputed {
    pub masked: String,
    pub masked_plain_chars: usize,
    pub wall_chars: usize,
    pub wall_sentences: usize,
    pub has_inline_citation: bool,
    pub has_book_title: bool,
    pub has_list_structure: bool,
}

impl ParagraphPrecomputed {
    fn new(text: &str) -> Self {
        let masked = mask_text(text);
        let stripped_markup = strip_wall_markup(text);
        let masked_plain_chars = masked_plain_char_count(&masked);
        let wall_chars = masked_plain_char_count(&stripped_markup);
        let wall_sentences = stripped_wall_sentence_count(&stripped_markup);
        Self {
            masked,
            masked_plain_chars,
            wall_chars,
            wall_sentences,
            has_inline_citation: has_inline_citation(text),
            has_book_title: has_book_title(text),
            has_list_structure: has_list_structure(text),
        }
    }
}

pub(crate) enum BlockPrecomputed {
    Paragraph(ParagraphPrecomputed),
    Table(Result<TableShape, TableError>),
    Echarts { effective_option: Value },
    Other,
}

impl BlockPrecomputed {
    fn new(block: &quire_protocol::Block) -> Self {
        match &block.body {
            BlockKind::Paragraph { text } | BlockKind::Quote { text } => {
                Self::Paragraph(ParagraphPrecomputed::new(text))
            }
            BlockKind::Table(table) => Self::Table(validate_table(table)),
            BlockKind::Echarts(chart) => Self::Echarts {
                effective_option: effective_chart_option(chart),
            },
            BlockKind::Heading { .. } | BlockKind::CustomHtml(_) => Self::Other,
        }
    }
}

pub(crate) struct LintContext<'a> {
    pub document: &'a ReportDocument,
    pub citation_scan: CitationScan,
    blocks: Vec<BlockPrecomputed>,
}

impl<'a> LintContext<'a> {
    pub(crate) fn build(document: &'a ReportDocument, parallel: bool) -> Self {
        let citation_scan = scan_document_citations(document);
        let build_block = |block: &quire_protocol::Block| BlockPrecomputed::new(block);
        let blocks = if parallel {
            document.blocks.par_iter().map(build_block).collect()
        } else {
            document.blocks.iter().map(build_block).collect()
        };
        Self {
            document,
            citation_scan,
            blocks,
        }
    }

    pub(crate) fn paragraph(&self, index: usize) -> Option<&ParagraphPrecomputed> {
        match self.blocks.get(index) {
            Some(BlockPrecomputed::Paragraph(paragraph)) => Some(paragraph),
            _ => None,
        }
    }

    pub(crate) fn table_shape(&self, index: usize) -> Option<&Result<TableShape, TableError>> {
        match self.blocks.get(index) {
            Some(BlockPrecomputed::Table(shape)) => Some(shape),
            _ => None,
        }
    }

    pub(crate) fn effective_option(&self, index: usize) -> Option<&Value> {
        match self.blocks.get(index) {
            Some(BlockPrecomputed::Echarts { effective_option }) => Some(effective_option),
            _ => None,
        }
    }
}
