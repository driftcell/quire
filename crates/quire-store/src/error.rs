//! Structured storage and event-bus errors.

use quire_protocol::{
    Block, BlockId, Diagnostic, DocId, PatchError, PatchOp, ProtocolError, Revision, SourceId,
};
use thiserror::Error;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RevisionConflict {
    pub doc_id: DocId,
    pub expected_revision: Revision,
    pub current_revision: Revision,
    pub conflicting_block_ids: Vec<BlockId>,
    #[serde(default)]
    pub conflicting_source_ids: Vec<SourceId>,
    /// Latest committed state of the conflicting blocks, so an agent can patch against it.
    pub latest_blocks: Vec<Block>,
    /// Revision numbers whose forward ops touched those blocks.
    pub conflicting_revisions: Vec<Revision>,
}

impl RevisionConflict {
    pub fn hint(&self) -> String {
        format!(
            "re-read the document, then retry with expected_revision={}. Conflicting block_ids: {}. Latest blocks are included in the error details.",
            self.current_revision,
            if self.conflicting_block_ids.is_empty() {
                "(none)".to_string()
            } else {
                self.conflicting_block_ids.join(", ")
            }
        )
    }
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("redis error: {0}")]
    Redis(#[from] redis::RedisError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("patched document is invalid: {0}")]
    Patch(#[from] PatchError),
    #[error("protocol validation failed: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("report `{doc_id}` was not found. Call list_reports or create_report first.")]
    NotFound { doc_id: DocId },
    #[error("document `{doc_id}` already exists")]
    AlreadyExists { doc_id: DocId },
    #[error(
        "expected_revision {expected} is ahead of current_revision {current} for `{doc_id}`; reload the document"
    )]
    RevisionAhead {
        doc_id: DocId,
        expected: Revision,
        current: Revision,
    },
    #[error(
        "revision conflict for `{}` at revision {} (expected {})",
        .0.doc_id,
        .0.current_revision,
        .0.expected_revision
    )]
    Conflict(Box<RevisionConflict>),
    #[error(
        "target revision {target} is invalid for `{doc_id}`; choose an earlier revision between 1 and {current} (rollback creates a new revision)"
    )]
    InvalidRollbackTarget {
        doc_id: DocId,
        target: Revision,
        current: Revision,
    },
    #[error(
        "revision {revision} of `{doc_id}` is missing or contains corrupt operation JSON: {message}"
    )]
    CorruptRevision {
        doc_id: DocId,
        revision: Revision,
        message: String,
    },
    #[error("document `{doc_id}` has an invalid id: {reason}")]
    InvalidDocId { doc_id: String, reason: String },
    #[error("source id `{source_id}` is invalid: {reason}")]
    InvalidSourceId { source_id: String, reason: String },
    #[error(
        "source `{source_id}` conflicts with normalized key `{normalized_key}`; canonical source is `{canonical_id}`"
    )]
    SourceConflict {
        source_id: SourceId,
        normalized_key: String,
        canonical_id: SourceId,
    },
    #[error("comment thread `{thread_id}` was not found for document `{doc_id}`")]
    CommentThreadNotFound { doc_id: DocId, thread_id: String },
    #[error("comment target block `{block_id}` does not exist in document `{doc_id}`")]
    CommentTargetNotFound { doc_id: DocId, block_id: BlockId },
    #[error("base-62 rank space exhausted between existing neighbours: {0}")]
    RankExhausted(String),
    #[error("snapshot compression failed: {0}")]
    Snapshot(String),
    #[error("access denied: {0}")]
    Forbidden(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("internal storage error: {0}")]
    Internal(String),
}

impl StoreError {
    /// Recovery hint suitable for MCP error data and HTTP JSON error responses.
    pub fn hint(&self) -> String {
        match self {
            Self::Conflict(conflict) => conflict.hint(),
            Self::NotFound { .. } => {
                "Call list_reports to discover valid doc_ids, or create_report to create one."
                    .into()
            }
            Self::RevisionAhead { current, .. } => {
                format!("Use expected_revision={current} or omit it by re-reading the document.")
            }
            Self::InvalidRollbackTarget { current, .. } => {
                let last = (current.get() - 1).max(1);
                format!(
                    "Choose target_revision between 1 and {last}; current_revision is {current}."
                )
            }
            Self::InvalidDocId { .. } => {
                "Use 1-64 ASCII letters/digits plus '-' or '_'; never a UUIDv4.".into()
            }
            Self::InvalidSourceId { .. } => {
                "Use 1-64 ASCII letters/digits plus '-' '_' or '.', starting alphanumeric.".into()
            }
            Self::SourceConflict { canonical_id, .. } => {
                format!("Reuse canonical source_id `{canonical_id}` or change the URL/title.")
            }
            Self::CommentThreadNotFound { .. } => {
                "Call list_comments or GET /api/docs/{doc_id}/comments to discover valid thread IDs."
                    .into()
            }
            Self::CommentTargetNotFound { .. } => {
                "Re-read get_document_digest and anchor the comment to an existing block ID.".into()
            }
            Self::Patch(error) => error.to_string(),
            Self::Protocol(error) => error.hint(),
            Self::CorruptRevision { .. } => {
                "Inspect GET /api/docs/{doc_id}/revisions and choose a valid rollback target."
                    .into()
            }
            Self::Forbidden(_) => {
                "Verify that your account or session owns the target report before accessing it.".into()
            }
            Self::InvalidRequest(_) => {
                "Check the request JSON against contracts/report-protocol.schema.json; invalid IDs and oversized payloads are rejected.".into()
            }
            _ => "Retry the request; if the problem persists inspect the server logs.".into(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Database(_) => "DATABASE_ERROR",
            Self::Redis(_) => "REDIS_ERROR",
            Self::Json(_) => "JSON_ERROR",
            Self::Patch(_) => "PATCH_ERROR",
            Self::Protocol(_) => "PROTOCOL_ERROR",
            Self::NotFound { .. } => "DOC_NOT_FOUND",
            Self::AlreadyExists { .. } => "DOC_EXISTS",
            Self::RevisionAhead { .. } => "REVISION_AHEAD",
            Self::Conflict(_) => "REVISION_CONFLICT",
            Self::InvalidRollbackTarget { .. } => "INVALID_ROLLBACK_TARGET",
            Self::CorruptRevision { .. } => "CORRUPT_REVISION",
            Self::InvalidDocId { .. } => "INVALID_DOC_ID",
            Self::InvalidSourceId { .. } => "INVALID_SOURCE_ID",
            Self::SourceConflict { .. } => "SOURCE_CONFLICT",
            Self::CommentThreadNotFound { .. } => "COMMENT_THREAD_NOT_FOUND",
            Self::CommentTargetNotFound { .. } => "COMMENT_TARGET_NOT_FOUND",
            Self::RankExhausted(_) => "RANK_EXHAUSTED",
            Self::Snapshot(_) => "SNAPSHOT_ERROR",
            Self::Forbidden(_) => "FORBIDDEN",
            Self::InvalidRequest(_) => "INVALID_REQUEST",
            Self::Internal(_) => "INTERNAL_ERROR",
        }
    }
}

/// Convert an operation list to JSON for forwarding / inverse logs.
pub fn ops_to_json(ops: &[PatchOp]) -> Result<serde_json::Value, StoreError> {
    Ok(serde_json::to_value(ops)?)
}

/// Diagnostics from the post-commit lint pass, surfaced as inline warnings by MCP.
pub type InlineWarnings = Vec<Diagnostic>;
