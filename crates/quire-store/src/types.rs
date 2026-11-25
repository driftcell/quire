//! Revision, stream and receipt types shared by storage, HTTP and MCP layers.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use quire_protocol::{
    BlockId, CommentAuthorType, Diagnostic, DocId, EvaluationThreadMeta, PatchOutcome,
    ReportDocument, Revision, SessionId, SourceEntry, SourceId, TextQuoteSelector,
};
use serde::{Deserialize, Serialize};

use crate::error::RevisionConflict;

/// Revision delta broadcast by the SSE `revision` event and persisted in Redis Streams.
///
/// The wire type lives in `quire-protocol` so the generated frontend schema and the server
/// serialize the exact same struct.
pub use quire_protocol::RevisionDelta as RevisionEvent;

/// Session discovery payload broadcast by the SSE `doc_created` event.
pub use quire_protocol::SessionDelta as SessionEvent;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevisionRecord {
    pub doc_id: DocId,
    pub revision: Revision,
    pub actor: String,
    pub summary: String,
    pub forward_ops: serde_json::Value,
    pub inverse_ops: serde_json::Value,
    /// Complete set of block IDs read/written by this revision; used by Smart CAS.
    pub affected_block_ids: Vec<BlockId>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateResult {
    pub doc_id: DocId,
    pub revision: Revision,
    pub title: String,
    pub session_id: Option<SessionId>,
    pub idempotent_replay: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyResult {
    pub new_revision: Revision,
    /// Revision supplied by the caller as the Smart CAS base.
    pub base_revision: Revision,
    /// True when the write landed on top of newer revisions via Smart CAS.
    pub rebased: bool,
    /// Number of committed revisions skipped by a successful rebase.
    pub rebased_over: usize,
    pub outcome: PatchOutcome,
    pub inline_warnings: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackResult {
    pub new_revision: Revision,
    pub target_revision: Revision,
    pub replayed_revisions: usize,
    pub outcome: PatchOutcome,
    pub inline_warnings: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSnapshot {
    pub document: ReportDocument,
    pub current_rev: Revision,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportSummary {
    pub doc_id: DocId,
    pub title: String,
    pub session_id: Option<SessionId>,
    pub current_rev: Revision,
    pub block_count: i64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportList {
    pub reports: Vec<ReportSummary>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone)]
pub struct SourceUpsert {
    pub source_id: SourceId,
    pub source: SourceEntry,
}

/// Input for creating one side-channel comment thread and its first message.
#[derive(Debug, Clone)]
pub struct NewCommentThread {
    pub block_id: BlockId,
    /// Ending block for a cross-block selection.
    pub end_block_id: Option<BlockId>,
    /// Every selected block in document order. Empty falls back to the start/end blocks.
    pub anchor_blocks: Vec<BlockId>,
    pub author_type: CommentAuthorType,
    pub author: String,
    pub body: String,
    pub quote: Option<TextQuoteSelector>,
    /// End-block selector for cross-block selections.
    pub end_quote: Option<TextQuoteSelector>,
    /// Optional frontend-authored snapshot containing media placeholders.
    pub anchor_snapshot: Option<String>,
    pub evaluation_meta: Option<EvaluationThreadMeta>,
}

/// Input for appending one reply to an existing comment thread.
#[derive(Debug, Clone)]
pub struct NewCommentReply {
    pub author_type: CommentAuthorType,
    pub author: String,
    pub body: String,
    pub mark_addressed: bool,
    pub addressed_rev: Option<Revision>,
}

/// Optional side inputs committed atomically with an operation batch.
#[derive(Debug, Clone, Default)]
pub struct CommitExtras {
    /// Sources upserted in the same transaction.
    pub sources: Vec<SourceUpsert>,
    /// Ephemeral previews atomically superseded by the emitted revision.
    pub cleared_preview_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpsertSourcesResult {
    pub canonical_id_map: BTreeMap<SourceId, SourceId>,
    pub revision: Revision,
    pub changed: bool,
    pub sources: IndexMap<SourceId, SourceEntry>,
    pub inline_warnings: Vec<Diagnostic>,
}

#[derive(Debug, Clone)]
pub struct DeleteSourcesResult {
    pub revision: Revision,
    pub changed: bool,
    pub deleted_ids: Vec<SourceId>,
    pub source_count: usize,
}

#[derive(Debug, Clone)]
pub struct StreamRevision {
    pub stream_id: String,
    pub event: RevisionEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStreamEntry {
    pub stream_id: String,
    pub event: SessionEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHistoryState {
    pub rank: String,
    pub block: Option<quire_protocol::Block>,
}

/// Internal diff used when persisting only changed rows.
#[derive(Debug, Default)]
pub(crate) struct DocumentDiff {
    pub deleted_block_ids: Vec<BlockId>,
    pub changed_blocks: Vec<quire_protocol::Block>,
    pub changed_ranks: BTreeMap<BlockId, quire_protocol::Rank>,
}

/// Helper for callers that need a conflict receipt without panicking.
pub fn conflict_error(conflict: RevisionConflict) -> crate::error::StoreError {
    crate::error::StoreError::Conflict(Box::new(conflict))
}
