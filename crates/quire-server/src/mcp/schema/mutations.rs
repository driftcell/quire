use std::collections::BTreeMap;

use indexmap::IndexMap;
use quire_protocol::{CoverConfig, IdAssignment, PatchOp, SourceEntry, SourceId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Input for `apply_operations`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyOperationsParams {
    /// Target report `doc_id`.
    pub doc_id: String,
    /// Base document revision you read from `get_document_digest`, `read_as_markdown`, or `read_blocks`. Smart CAS automatically rebases non-overlapping concurrent edits and returns a conflict error with valid IDs if overlapping blocks changed.
    pub expected_revision: i64,
    /// Concise audit summary of this mutation batch (e.g. `"author ch2 revenue analysis and KPI table"`).
    pub summary: String,
    /// Ordered declarative patch operations (`append`, `insert_before`, `insert_after`, `patch_block`, `replace_range`, `delete_blocks`, `move_block`). Prefer `patch_block` for in-place edits so review comment anchors are preserved.
    pub ops: Vec<PatchOp>,
    /// Optional bibliography sources (`{"src_id": {...}}`) registered atomically in the same transaction before validating citations in `ops`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<BTreeMap<SourceId, SourceInput>>")]
    pub sources: Option<IndexMap<SourceId, SourceInput>>,
    /// Ephemeral streamed preview IDs superseded by this commit (populated automatically by the runtime streaming hook for flicker-free replacement).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preview_ids: Vec<String>,
}

/// Output for `apply_operations`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ApplyOperationsReceipt {
    pub new_revision: i64,
    /// Revision supplied as `expected_revision`.
    pub base_revision: i64,
    /// True when Smart CAS committed the batch on top of newer revisions.
    pub rebased: bool,
    /// Number of committed revisions skipped by a successful rebase.
    pub rebased_over: usize,
    /// True when the batch committed; inspect `blocking_errors` before continuing.
    pub committed: bool,
    /// Block IDs auto-assigned by the patch engine.
    pub assigned_ids: Vec<IdAssignment>,
    /// Non-blocking warnings/info diagnostics scoped to blocks touched by this batch.
    pub inline_warnings: Vec<quire_protocol::Diagnostic>,
    /// Lint errors scoped to blocks touched by this batch. A non-empty list means the batch
    /// committed but the document is not clean yet.
    pub blocking_errors: Vec<quire_protocol::Diagnostic>,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// Input for `rollback_revision`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RollbackParams {
    /// Target report `doc_id`.
    pub doc_id: String,
    /// Historical revision number to restore (`1..current_revision - 1`). Creates a new forward revision whose document state matches `target_revision`.
    pub target_revision: i64,
}

/// Output for `rollback_revision`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RollbackReceipt {
    pub doc_id: String,
    /// New monotonic revision created by the rollback.
    pub new_revision: i64,
    /// Revision state that was restored.
    pub rolled_back_to: i64,
    /// Number of inverse-op revisions replayed.
    pub replayed_revisions: usize,
    /// Compact human-readable outcome; inspect content with digest/read_blocks if needed.
    pub summary: String,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// Source payload accepted by `upsert_sources` and inline source maps.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SourceInput {
    /// Stable short semantic source ID (`[a-zA-Z0-9_-]{1,48}`, e.g. `"annual_rep_25"`). Required in `upsert_sources` list items; optional when supplied as a map value in `apply_operations.sources`.
    #[serde(default, alias = "source_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<SourceId>,
    #[serde(flatten)]
    pub entry: SourceEntry,
}

/// Input for `upsert_sources`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpsertSourcesParams {
    /// Target report `doc_id` that owns the bibliography registry (optional in chat sessions; if omitted, uses or auto-creates the session's bound document).
    #[serde(default)]
    pub doc_id: String,
    /// Non-empty list of real sources actually read during research. Include `title`, optional `url`/`author`/`published_at`/`publisher`/`note`, and a verbatim `evidence` excerpt supporting the cited claim. Duplicate URLs or normalized titles coalesce into one canonical source ID returned in `canonical_id_map`.
    pub sources: Vec<SourceInput>,
}

/// Output for `upsert_sources`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct UpsertSourcesReceipt {
    pub doc_id: String,
    pub revision: i64,
    /// False when every source already existed and no revision was created.
    pub changed: bool,
    /// Requested source ID -> canonical source ID.
    pub canonical_id_map: BTreeMap<String, String>,
    pub source_count: usize,
    pub inline_warnings: Vec<quire_protocol::Diagnostic>,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// Input for `list_sources`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListSourcesParams {
    /// Target report `doc_id` (optional in chat sessions; if omitted, uses the session's bound document).
    #[serde(default)]
    pub doc_id: String,
    /// Optional list of exact `SourceId`s to fetch. Omit to list all registered sources on the report; any unknown ID returns an error with the valid registered IDs.
    #[serde(default)]
    pub source_ids: Option<Vec<String>>,
}

/// Output for `list_sources`, including evidence excerpts needed for claim-level review.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListSourcesReceipt {
    pub doc_id: String,
    pub revision: i64,
    #[schemars(with = "BTreeMap<SourceId, SourceEntry>")]
    pub sources: IndexMap<SourceId, SourceEntry>,
    pub count: usize,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// Output for `set_cover`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SetCoverReceipt {
    pub doc_id: String,
    pub revision: i64,
    pub cover: CoverConfig,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}
