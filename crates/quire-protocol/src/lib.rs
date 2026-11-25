//! Pure domain protocol for multi-agent reports.
//!
//! This crate performs no I/O. It owns the canonical Block AST, deterministic fractional ranks,
//! declarative patch operations, citation resolution, Markdown projection for agents, table
//! schema validation and lint contracts. The pure rule engine lives in `quire-lint`. Frontend
//! HTML/Tiptap rendering is deliberately out of scope and lives in the external read-only viewer.

#![forbid(unsafe_code)]

pub mod block;
pub mod citation;
pub mod comment;
pub mod computed;
pub mod diff;
pub(crate) mod echarts;
pub(crate) mod echarts_view;
pub mod error;
pub mod evaluation;
pub mod events;
pub mod ids;
pub(crate) mod inline;
pub(crate) mod json;
pub mod lint;
pub mod markdown;
pub mod model;
pub mod normalize;
pub mod ops;
pub mod rank;
pub mod short_id;
pub mod sync;
pub mod table;
pub(crate) mod text;

pub use block::{
    Align, Block, BlockKind, BlockStyle, CellValue, ColumnDataType, ColumnNode, ColumnSizing,
    CustomHtmlBlock, DataType, EchartsBlock, NumberingKind, RichCell, TableBlock,
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
    LayoutLeafColumn, TableLayout, TocNode, chart_data_tables_for_block, chart_requirements,
    compute_block_framing, compute_document_meta, compute_table_layout, compute_toc_tree,
};
pub use diff::{BlockDiffEntry, DocumentDiffReport, DocumentMetadataDiff, diff_documents};
pub use echarts::series as chart_series;
pub use echarts::{
    ChartDataTable, axis_count, chart_data_tables, configured_axis_count, effective_chart_option,
    entries, find_js_function_string, has_data_source, inner_title_duplicates,
    is_center_inner_title, is_positioned_or_light_inner_title, is_valid_series_type,
    merge_chart_option_patch,
};
pub use echarts_view::{
    EchartsAxisLabelView, EchartsAxisView, EchartsComponent, EchartsDatasetView,
    EchartsEndLabelView, EchartsLabelLayoutView, EchartsOptionView, EchartsSeriesView,
    EchartsTextStyleView, EchartsTitleView,
};
pub use error::{ApiErrorEnvelope, ApiErrorInfo, ProtocolError};
pub use evaluation::{
    BlockEvaluationResult, ClaimGroundingScore, ContextualBlockSpan, ContextualDocumentView,
    EmbeddingTaskType, EscalationStatus, EvaluationDimension, EvaluationFinding,
    EvaluationThreadMeta, GroundingFact, SectionEvaluationBatch, SentenceSpan,
    build_contextual_document_view, partition_section_batches, segment_prose_sentences,
};
pub use events::{
    AgentPresenceEvent, CommentUpdatedEvent, DEFAULT_PREVIEW_TTL_MS, EphemeralClearEvent,
    EphemeralPayload, EphemeralPreviewEvent, PartialBlockPreview, PreviewAnchor, PreviewStatus,
    RevisionDelta, SessionDelta, SyncRequiredDelta,
};
pub use ids::{BlockId, DocId, Rank, Revision, SessionId, SourceId};
pub use inline::inline_citations;
pub use json::merge_optional_json;
pub use lint::{Diagnostic, LintConfig, LintReport, LintSummary, Severity};
pub use markdown::{
    MarkdownError, MarkdownOptions, MarkdownProjector, project_blocks, project_markdown,
    project_section,
};
pub use model::{
    COVER_VARIANTS, CitationStyle, CoverConfig, HeadingPageBreak, PageChromeConfig,
    PageChromeSlots, ReferencesConfig, ReportDocument, SourceEntry, TOC_VARIANTS, ThemeConfig,
    ThemeSummary, TocConfig, is_safe_css_token_value, is_valid_css_token_name,
};
pub use ops::{
    BlockPatch, BlockStylePatch, CellUpdate, IdAssignment, PatchError, PatchOp, PatchOutcome,
    apply_operations, apply_operations_in_place, remap_operations_sources, remap_source_references,
};
pub use rank::{RankError, rank_after, rank_before, rank_between, try_rank_between};
pub use short_id::{DEFAULT_SHORT_ID_LEN, bpe_word_pair, compact_id, short_id, short_token};
pub use sync::{DocumentSyncDelta, sync_documents};

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
    LeafColumn, TableError, TableShape, compute_column_decimals, display_cell,
    display_cell_aligned, unknown_row_keys, unknown_width_overrides, validate_table,
};
pub use text::{CssBraceIssue, css_brace_issue, safe_href};
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
