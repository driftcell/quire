//! Export the canonical protocol JSON Schema used by the independent frontend.
//!
//! The bundle covers the stored document, computed presentation metadata, declarative patch
//! operations, lint reports and the SSE delta payloads emitted by the server. It is written
//! directly to `contracts/report-protocol.schema.json`.
//!
//! ```sh
//! cargo run -p quire-protocol --example export_schema
//! ```

use std::{fs, path::Path};

use quire_protocol::{
    AgentPresenceEvent, ChartDataTable, CommentMessage, CommentThread, CommentUpdatedEvent,
    ComputedDocumentMeta, DocumentDiffReport, DocumentMetadataDiff, EphemeralClearEvent,
    EphemeralPayload, EphemeralPreviewEvent, LintReport, PatchOp, PatchOutcome, ReportDocument,
    ReportDraft, RevisionDelta, SessionDelta, SyncRequiredDelta,
};
use schemars::JsonSchema;

// Fields exist solely to pull each public type into the generated `$defs`.
#[allow(dead_code)]
#[derive(Debug, JsonSchema)]
#[schemars(title = "Quire Report Protocol")]
struct ProtocolContract {
    /// Canonical stored/read document returned by `GET /api/docs/{doc_id}`.
    document: ReportDocument,
    /// Computed labels, table layouts, TOC tree and citation map for frontends.
    computed: ComputedDocumentMeta,
    /// Declarative patch operation submitted to `apply_operations`.
    patch_op: PatchOp,
    /// SSE `revision` delta payload.
    revision_delta: RevisionDelta,
    /// SSE `doc_created` session delta payload.
    session_delta: SessionDelta,
    /// SSE `sync_required` payload emitted when replay detects a gap.
    sync_required: SyncRequiredDelta,
    /// SSE `ephemeral_preview` payload for transient typewriter previews.
    ephemeral_preview: EphemeralPreviewEvent,
    /// SSE `agent_presence` payload for live agent activity.
    agent_presence: AgentPresenceEvent,
    /// SSE `ephemeral_clear` payload for immediate placeholder cleanup.
    ephemeral_clear: EphemeralClearEvent,
    /// SSE `comment_updated` payload for live review-thread changes.
    comment_updated: CommentUpdatedEvent,
    /// Tagged union accepted by `POST /api/docs/{doc_id}/ephemeral`.
    ephemeral_payload: EphemeralPayload,
    /// Side-channel review thread returned by the comments REST API and MCP tools.
    comment_thread: CommentThread,
    /// One review-thread message.
    comment_message: CommentMessage,
    /// Structured block-level diff between two revisions.
    document_diff: DocumentDiffReport,
    /// Non-block metadata flags included in a document diff.
    document_metadata_diff: DocumentMetadataDiff,
    /// Structured ECharts data table projected from a chart block.
    chart_data_table: ChartDataTable,
    /// Offline authoring draft accepted by `apply_draft`.
    draft: ReportDraft,
    /// Receipt returned by the in-memory patch engine.
    patch_outcome: PatchOutcome,
    /// Static validation report returned by `lint_document`.
    lint: LintReport,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let schema = schemars::schema_for!(ProtocolContract);
    let rendered = serde_json::to_string_pretty(&schema)?;
    let contract_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("contracts")
        .join("report-protocol.schema.json");
    if let Some(parent) = contract_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&contract_path, format!("{rendered}\n"))?;
    Ok(())
}
