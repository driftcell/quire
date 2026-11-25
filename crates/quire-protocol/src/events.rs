//! Wire-format payloads for the read-only SSE contract.
//!
//! These types are the single source of truth for event data consumed by the independent
//! frontend. Storage publishes them verbatim to Redis Streams and the HTTP server serializes
//! them unchanged, so the generated JSON Schema cannot drift from the live wire format.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{BlockId, CommentStatus, DocId, Revision, SessionId};

/// Suggested lifetime for a transient preview when the producer omits `ttl_ms`.
pub const DEFAULT_PREVIEW_TTL_MS: u64 = 15_000;
/// Hard ceiling on ephemeral preview `ttl_ms` to prevent browser `setTimeout` 32-bit overflow.
pub const MAX_PREVIEW_TTL_MS: u64 = 300_000;

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
    /// Ephemeral previews atomically superseded by this revision.
    ///
    /// The frontend removes these previews in the same frame that applies the revision, so the
    /// committed block replaces the streamed placeholder with no visual flicker.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cleared_preview_ids: Vec<String>,
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
            cleared_preview_ids: Vec::new(),
            created_at: Utc::now(),
        }
    }

    /// Attach the preview IDs superseded by this revision.
    pub fn with_cleared_preview_ids(mut self, preview_ids: Vec<String>) -> Self {
        self.cleared_preview_ids = preview_ids;
        self
    }
}

/// Where an ephemeral preview block sits relative to the committed document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PreviewAnchor {
    /// Append below the section heading identified by `section_id`.
    AppendToSection { section_id: BlockId },
    /// Insert directly after the committed block.
    AfterBlock { block_id: BlockId },
    /// Insert directly before the committed block.
    BeforeBlock { block_id: BlockId },
    /// Temporarily replace a committed block, for example a placeholder heading.
    ReplaceBlock { block_id: BlockId },
    /// Append after the last committed block in the document.
    AppendToDoc,
}

/// Lifecycle state of one ephemeral preview stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PreviewStatus {
    Streaming,
    Completed,
    Aborted,
}

/// Incrementally streamed block content rendered before the revision is committed.
///
/// Text blocks carry partial `text`; tables and charts may only carry `title` while streaming so
/// the frontend can render a skeleton. `block_id` is pre-allocated when known and must match the
/// eventual committed block for seamless replacement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PartialBlockPreview {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<BlockId>,
    /// Canonical block type, for example `paragraph`, `heading`, `table` or `echarts`.
    pub block_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Payload of an SSE `ephemeral_preview` event.
///
/// Preview frames never enter the revision log. The server stamps `doc_id` and `received_at`
/// from the request path and wall clock so producers cannot spoof either field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EphemeralPreviewEvent {
    pub doc_id: DocId,
    /// Stable stream identity, normally the Runtime `tool_call_id`.
    pub preview_id: String,
    /// Agent identity that produced the preview.
    pub actor: String,
    pub anchor: PreviewAnchor,
    pub status: PreviewStatus,
    /// Monotonic sequence number; clients discard frames older than the latest seen value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    /// Client-side cleanup deadline for abandoned streams.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_ms: Option<u64>,
    pub block: PartialBlockPreview,
    pub received_at: DateTime<Utc>,
}

/// Payload of an SSE `agent_presence` event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AgentPresenceEvent {
    pub doc_id: DocId,
    /// Agent identity that changed state.
    pub actor: String,
    /// Free-form presence state such as `writing` or `idle`.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_id: Option<BlockId>,
    pub received_at: DateTime<Utc>,
}

/// Payload of an SSE `comment_updated` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CommentUpdatedEvent {
    pub doc_id: DocId,
    pub thread_id: String,
    pub status: CommentStatus,
    pub anchor_detached: bool,
    pub updated_at: DateTime<Utc>,
}

/// Immediate client-side cleanup for previews that will never become a revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EphemeralClearEvent {
    pub doc_id: DocId,
    /// Preview IDs to remove from the frontend immediately.
    pub preview_ids: Vec<String>,
    pub received_at: DateTime<Utc>,
}

impl EphemeralClearEvent {
    /// Build a clear event stamped at the current server time.
    pub fn new(doc_id: DocId, preview_ids: Vec<String>) -> Self {
        Self {
            doc_id,
            preview_ids,
            received_at: Utc::now(),
        }
    }
}

/// Tagged union accepted by `POST /api/docs/{doc_id}/ephemeral` and forwarded over SSE.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EphemeralPayload {
    Preview {
        #[serde(flatten)]
        event: EphemeralPreviewEvent,
    },
    Presence {
        #[serde(flatten)]
        event: AgentPresenceEvent,
    },
    Clear {
        #[serde(flatten)]
        event: EphemeralClearEvent,
    },
    CommentUpdated {
        #[serde(flatten)]
        event: CommentUpdatedEvent,
    },
}

impl EphemeralPayload {
    /// SSE event name this payload must be forwarded as.
    pub const fn event_name(&self) -> &'static str {
        match self {
            Self::Preview { .. } => "ephemeral_preview",
            Self::Presence { .. } => "agent_presence",
            Self::Clear { .. } => "ephemeral_clear",
            Self::CommentUpdated { .. } => "comment_updated",
        }
    }

    /// Override producer-controlled routing fields with values derived from the request.
    pub fn stamp_server_context(&mut self, doc_id: &DocId, received_at: DateTime<Utc>) {
        match self {
            Self::Preview { event: preview } => {
                preview.doc_id = doc_id.clone();
                preview.received_at = received_at;
                preview.ttl_ms.get_or_insert(DEFAULT_PREVIEW_TTL_MS);
            }
            Self::Presence { event: presence } => {
                presence.doc_id = doc_id.clone();
                presence.received_at = received_at;
            }
            Self::Clear { event: clear } => {
                clear.doc_id = doc_id.clone();
                clear.received_at = received_at;
            }
            Self::CommentUpdated { event } => {
                event.doc_id = doc_id.clone();
                event.updated_at = received_at;
            }
        }
    }

    /// Reject semantically empty identities that serde alone cannot catch.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Preview { event: preview } => {
                if preview.preview_id.trim().is_empty() {
                    return Err("preview_id must not be empty".into());
                }
                if preview.actor.trim().is_empty() {
                    return Err("actor must not be empty".into());
                }
                if preview.block.block_type.trim().is_empty() {
                    return Err("block.block_type must not be empty".into());
                }
                if let Some(ttl_ms) = preview.ttl_ms
                    && (ttl_ms == 0 || ttl_ms > MAX_PREVIEW_TTL_MS)
                {
                    return Err(format!(
                        "ttl_ms must be between 1 and {MAX_PREVIEW_TTL_MS} milliseconds"
                    ));
                }
            }
            Self::Presence { event: presence } => {
                if presence.actor.trim().is_empty() {
                    return Err("actor must not be empty".into());
                }
                if presence.status.trim().is_empty() {
                    return Err("status must not be empty".into());
                }
            }
            Self::Clear { event: clear } => {
                if clear.preview_ids.is_empty() {
                    return Err("preview_ids must not be empty".into());
                }
                if clear
                    .preview_ids
                    .iter()
                    .any(|preview_id| preview_id.trim().is_empty())
                {
                    return Err("preview_ids must not contain empty values".into());
                }
            }
            Self::CommentUpdated { event } => {
                if event.thread_id.trim().is_empty() {
                    return Err("thread_id must not be empty".into());
                }
            }
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{block_id, doc_id};

    fn preview_event() -> EphemeralPreviewEvent {
        EphemeralPreviewEvent {
            doc_id: doc_id("report_1"),
            preview_id: "tool_call_7".into(),
            actor: "sub-agent-a".into(),
            anchor: PreviewAnchor::AfterBlock {
                block_id: block_id("ch1_placeholder"),
            },
            status: PreviewStatus::Streaming,
            seq: Some(3),
            ttl_ms: Some(DEFAULT_PREVIEW_TTL_MS),
            block: PartialBlockPreview {
                block_id: Some(block_id("ch1_summary")),
                block_type: "paragraph".into(),
                title: None,
                level: None,
                text: Some("Revenue grew 18%".into()),
            },
            received_at: Utc::now(),
        }
    }

    fn presence_event() -> AgentPresenceEvent {
        AgentPresenceEvent {
            doc_id: doc_id("report_1"),
            actor: "sub-agent-a".into(),
            status: "writing".into(),
            section_id: Some(block_id("ch1_intro")),
            received_at: Utc::now(),
        }
    }

    #[test]
    fn preview_payload_roundtrips_with_kind_tag() {
        let payload = EphemeralPayload::Preview {
            event: preview_event(),
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["kind"], "preview");
        assert_eq!(json["anchor"]["type"], "after_block");
        assert_eq!(json["anchor"]["block_id"], "ch1_placeholder");
        assert_eq!(json["status"], "streaming");
        let decoded: EphemeralPayload = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, payload);
        assert_eq!(decoded.event_name(), "ephemeral_preview");
    }

    #[test]
    fn all_preview_anchors_roundtrip() {
        let anchors = [
            PreviewAnchor::AppendToSection {
                section_id: block_id("ch1_intro"),
            },
            PreviewAnchor::AfterBlock {
                block_id: block_id("ch1_intro"),
            },
            PreviewAnchor::BeforeBlock {
                block_id: block_id("ch2_intro"),
            },
            PreviewAnchor::ReplaceBlock {
                block_id: block_id("ch2_placeholder"),
            },
            PreviewAnchor::AppendToDoc,
        ];
        for anchor in anchors {
            let event = EphemeralPreviewEvent {
                anchor,
                ..preview_event()
            };
            let encoded = serde_json::to_string(&event).unwrap();
            let decoded: EphemeralPreviewEvent = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, event);
        }
    }

    #[test]
    fn preview_status_encodes_lifecycle_values() {
        for (status, wire) in [
            (PreviewStatus::Streaming, "streaming"),
            (PreviewStatus::Completed, "completed"),
            (PreviewStatus::Aborted, "aborted"),
        ] {
            assert_eq!(serde_json::to_value(status).unwrap(), wire);
            let decoded: PreviewStatus = serde_json::from_value(wire.into()).unwrap();
            assert_eq!(decoded, status);
        }
    }

    #[test]
    fn legacy_kind_aliases_are_rejected() {
        let preview = EphemeralPayload::Preview {
            event: preview_event(),
        };
        let presence = EphemeralPayload::Presence {
            event: presence_event(),
        };
        for (payload, legacy_kind) in [(preview, "ephemeral"), (presence, "agent_presence")] {
            let mut json = serde_json::to_value(&payload).unwrap();
            json["kind"] = legacy_kind.into();
            assert!(
                serde_json::from_value::<EphemeralPayload>(json).is_err(),
                "`{legacy_kind}` must not be accepted as a wire kind"
            );
        }
    }

    #[test]
    fn presence_roundtrips_with_canonical_kind() {
        let payload = EphemeralPayload::Presence {
            event: presence_event(),
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["kind"], "presence");
        let decoded: EphemeralPayload = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, payload);
        assert_eq!(decoded.event_name(), "agent_presence");
    }

    #[test]
    fn clear_payload_roundtrips_with_kind_tag() {
        let payload = EphemeralPayload::Clear {
            event: EphemeralClearEvent::new(
                doc_id("report_1"),
                vec!["tool_call_7".into(), "tool_call_8".into()],
            ),
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["kind"], "clear");
        assert_eq!(json["preview_ids"][0], "tool_call_7");
        assert_eq!(payload.event_name(), "ephemeral_clear");
        let decoded: EphemeralPayload = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, payload);
        assert!(decoded.validate().is_ok());
    }

    #[test]
    fn clear_requires_preview_ids() {
        let payload = EphemeralPayload::Clear {
            event: EphemeralClearEvent::new(doc_id("report_1"), Vec::new()),
        };
        assert!(payload.validate().is_err());
        let payload = EphemeralPayload::Clear {
            event: EphemeralClearEvent::new(doc_id("report_1"), vec![" ".into()]),
        };
        assert!(payload.validate().is_err());
    }

    #[test]
    fn comment_updated_roundtrips_with_kind_tag() {
        let payload = EphemeralPayload::CommentUpdated {
            event: CommentUpdatedEvent {
                doc_id: doc_id("report_1"),
                thread_id: "th_1".into(),
                status: CommentStatus::Addressed,
                anchor_detached: true,
                updated_at: Utc::now(),
            },
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["kind"], "comment_updated");
        assert_eq!(json["status"], "addressed");
        assert_eq!(payload.event_name(), "comment_updated");
        let decoded: EphemeralPayload = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, payload);
        assert!(decoded.validate().is_ok());
    }

    #[test]
    fn preview_block_rejects_unknown_fields() {
        let block = serde_json::json!({
            "block_type": "paragraph",
            "text": "Revenue",
            "bogus": true
        });
        assert!(serde_json::from_value::<PartialBlockPreview>(block).is_err());
    }

    #[test]
    fn server_context_overrides_routing_fields_and_defaults_ttl() {
        let other_doc = doc_id("other_doc");
        let received_at = DateTime::parse_from_rfc3339("2026-11-18T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut payload = EphemeralPayload::Preview {
            event: EphemeralPreviewEvent {
                ttl_ms: None,
                ..preview_event()
            },
        };
        payload.stamp_server_context(&other_doc, received_at);
        let EphemeralPayload::Preview { event: preview } = &payload else {
            panic!("expected preview payload");
        };
        assert_eq!(preview.doc_id, other_doc);
        assert_eq!(preview.received_at, received_at);
        assert_eq!(preview.ttl_ms, Some(DEFAULT_PREVIEW_TTL_MS));
        assert!(payload.validate().is_ok());
    }

    #[test]
    fn empty_identities_are_rejected() {
        let payload = EphemeralPayload::Preview {
            event: EphemeralPreviewEvent {
                preview_id: " ".into(),
                ..preview_event()
            },
        };
        assert!(payload.validate().is_err());
    }

    #[test]
    fn revision_delta_roundtrips_cleared_preview_ids() {
        let delta = RevisionDelta::new(
            doc_id("report_1"),
            crate::test_support::revision(5),
            "sub-agent-a",
            "commit preview",
            serde_json::json!([]),
        )
        .with_cleared_preview_ids(vec!["tool_call_7".into()]);
        let json = serde_json::to_value(&delta).unwrap();
        assert_eq!(json["cleared_preview_ids"][0], "tool_call_7");
        let decoded: RevisionDelta = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, delta);
    }

    #[test]
    fn revision_delta_omits_empty_cleared_preview_ids() {
        let delta = RevisionDelta::new(
            doc_id("report_1"),
            crate::test_support::revision(5),
            "sub-agent-a",
            "metadata only",
            serde_json::json!([]),
        );
        let json = serde_json::to_value(&delta).unwrap();
        assert!(json.get("cleared_preview_ids").is_none());
        let decoded: RevisionDelta = serde_json::from_value(json).unwrap();
        assert!(decoded.cleared_preview_ids.is_empty());
    }
}
