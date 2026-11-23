use std::collections::BTreeMap;

use indexmap::IndexMap;
use quire_protocol::{CoverConfig, IdAssignment, PatchOp, SourceEntry, SourceId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Input for `apply_operations`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyOperationsParams {
    pub doc_id: String,
    /// Revision you read. Smart CAS rebases disjoint edits; overlapping edits return a conflict.
    pub expected_revision: i64,
    /// Short intent summary, for example "write chapter 2 revenue section".
    pub summary: String,
    /// Declarative patch operations. Example: `[{"type":"append","blocks":[...]}]`.
    pub ops: Vec<PatchOp>,
    /// Optional sources upserted atomically in the same transaction as the blocks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<BTreeMap<SourceId, SourceInput>>")]
    pub sources: Option<IndexMap<SourceId, SourceInput>>,
    /// Ephemeral preview IDs superseded by this commit. The emitted revision carries
    /// `cleared_preview_ids` so the frontend replaces streamed placeholders with no flicker.
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
    pub doc_id: String,
    /// Target revision in 1..current_revision-1.
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
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SourceInput {
    /// Stable short source ID, e.g. "sec10". Required by `upsert_sources`; optional when the
    /// source is supplied as a map value (`sources: {"sec10": {...}}`).
    #[serde(default, alias = "source_id", skip_serializing_if = "Option::is_none")]
    pub id: Option<SourceId>,
    #[serde(flatten)]
    pub entry: SourceEntry,
}

/// Input for `upsert_sources`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpsertSourcesParams {
    /// Report that owns the sources.
    pub doc_id: String,
    /// Sources to register/update. Duplicate URLs or normalized titles map to one canonical ID.
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
    pub doc_id: String,
    /// Optional exact source IDs. Unknown IDs return an error instead of being silently omitted.
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
