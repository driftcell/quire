//! Wire-format payloads for the read-only SSE contract.
//!
//! These types are the single source of truth for event data consumed by the independent
//! frontend. Storage publishes them verbatim to Redis Streams and the HTTP server serializes
//! them unchanged, so the generated JSON Schema cannot drift from the live wire format.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{DocId, Revision, SessionId};

/// Payload of an SSE `revision` event.
///
/// Emitted after each committed revision, in strictly ascending revision order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RevisionDelta {
    pub doc_id: DocId,
    pub revision: Revision,
    /// Agent or admin identity recorded for the audit trail.
    pub actor: String,
    pub summary: String,
    /// Forward patch operations; metadata-only revisions may carry an empty array.
    pub ops: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

impl RevisionDelta {
    /// Build a delta stamped at the current server time.
    pub fn new(
        doc_id: DocId,
        revision: Revision,
        actor: impl Into<String>,
        summary: impl Into<String>,
        ops: serde_json::Value,
    ) -> Self {
        Self {
            doc_id,
            revision,
            actor: actor.into(),
            summary: summary.into(),
            ops,
            created_at: Utc::now(),
        }
    }
}

/// Payload of an SSE `doc_created` event on the session discovery stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SessionDelta {
    /// Event name, always `doc_created`.
    pub event: String,
    pub session_id: SessionId,
    pub doc_id: DocId,
    pub title: String,
    pub revision: Revision,
    pub created_at: DateTime<Utc>,
}

/// Payload of an SSE `sync_required` event.
///
/// Sent when replay detects a gap or a storage failure. Clients must fetch a full snapshot
/// from `GET /api/docs/{doc_id}` before resuming from a new revision cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SyncRequiredDelta {
    pub doc_id: DocId,
    /// Last revision the stream delivered before detecting the gap.
    pub since_revision: Revision,
    pub reason: String,
    pub hint: String,
}
