//! Pure domain protocol for multi-agent reports.
//!
//! This crate performs no I/O. It owns the canonical Block AST, deterministic fractional ranks,
//! declarative patch operations, citation resolution, Markdown projection for agents, table
//! schema validation and the static linter. Frontend HTML/Tiptap rendering is deliberately out
//! of scope and lives in the external read-only viewer.

#![forbid(unsafe_code)]

pub mod block;
pub mod citation;
pub mod comment;
pub mod computed;
pub mod diff;
pub mod draft;
pub(crate) mod echarts;
pub mod error;
pub mod events;
pub mod ids;
pub(crate) mod inline;
pub(crate) mod json;
pub mod linter;
pub mod markdown;
pub mod model;
pub mod normalize;
pub mod ops;
pub mod rank;
pub mod table;
pub(crate) mod text;

pub use block::{
    Align, Block, BlockKind, BlockStyle, CellValue, ColumnDataType, ColumnNode, ColumnSizing,
    CustomHtmlBlock, DataType, EchartsBlock, EchartsMode, EchartsPreset, NumberingKind, RichCell,
    TableBlock,
};
pub use citation::{
    BibliographyEntry, CitationLocation, CitationResolution, CitationScan, CitationUse,
    resolve_citations, scan_document_citations,
};
pub use comment::{
    CommentAuthorType, CommentMessage, CommentStatus, CommentThread, TextQuoteSelector,
};
pub use computed::{
    BlockFraming, ChartRequirements, CitationMapEntry, ComputedDocumentMeta, LayoutHeaderCell,
    LayoutLeafColumn, TableLayout, TocNode, compute_block_framing, compute_document_meta,
    compute_table_layout, compute_toc_tree,
};
pub use diff::{BlockDiffEntry, DocumentDiffReport, DocumentMetadataDiff, diff_documents};
pub use draft::{DraftError, ReportDraft};
pub use echarts::{ChartDataTable, chart_data_tables};
pub use error::ProtocolError;
pub use events::{
    AgentPresenceEvent, CommentUpdatedEvent, DEFAULT_PREVIEW_TTL_MS, EphemeralClearEvent,
    EphemeralPayload, EphemeralPreviewEvent, PartialBlockPreview, PreviewAnchor, PreviewStatus,
    RevisionDelta, SessionDelta, SyncRequiredDelta,
};
pub use ids::{BlockId, DocId, Rank, Revision, SessionId, SourceId};
pub use linter::{Diagnostic, LintReport, LintSummary, Severity, lint_document};
pub use markdown::{
    MarkdownError, MarkdownOptions, MarkdownProjector, project_blocks, project_markdown,
    project_section,
};
pub use model::{
    COVER_VARIANTS, CitationStyle, CoverConfig, ReferencesConfig, ReportDocument, SourceEntry,
    TOC_VARIANTS, ThemeConfig, ThemeSummary, TocConfig, is_safe_css_token_value,
    is_valid_css_token_name,
};
pub use ops::{
    BlockPatch, BlockStylePatch, CellUpdate, IdAssignment, PatchError, PatchOp, PatchOutcome,
    apply_operations, apply_operations_in_place, remap_operations_sources, remap_source_references,
};
pub use rank::{RankError, rank_after, rank_before, rank_between, try_rank_between};

/// Validate arbitrary scoped CSS and return an actionable located error.
pub fn validate_custom_css(css: &str) -> Result<(), ProtocolError> {
    match text::css_brace_issue(css) {
        Some(issue) => Err(ProtocolError::UnbalancedCssBraces {
            line: issue.line,
            column: issue.column,
            message: issue.message,
            recovery: "Balance braces and close strings/comments; the line/column points at the first problem.".into(),
        }),
        None => Ok(()),
    }
}
pub use table::{
    LeafColumn, TableError, TableShape, display_cell, unknown_row_keys, validate_table,
};
#[cfg(test)]
pub(crate) mod test_support {
    use crate::ids::{BlockId, DocId, Revision, SourceId};

    pub fn doc_id(raw: &str) -> DocId {
        DocId::parse(raw).expect("valid test doc_id")
    }

    pub fn block_id(raw: &str) -> BlockId {
        BlockId::parse(raw).expect("valid test block_id")
    }

    pub fn source_id(raw: &str) -> SourceId {
        SourceId::parse(raw).expect("valid test source_id")
    }

    pub fn revision(value: i64) -> Revision {
        Revision::new(value).expect("valid test revision")
    }
}
