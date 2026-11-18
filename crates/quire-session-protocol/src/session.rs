//! REST-facing session models.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Archived,
}

impl SessionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Archived => "archived",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActiveRunStatus {
    Idle,
    Running,
    Interrupted,
    Failed,
}

impl ActiveRunStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "idle" => Some(Self::Idle),
            "running" => Some(Self::Running),
            "interrupted" => Some(Self::Interrupted),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    Tool,
    System,
}

impl MessageRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
            Self::System => "system",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            "tool" => Some(Self::Tool),
            "system" => Some(Self::System),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AttachmentMeta {
    pub attachment_id: String,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContextUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub context_used: u64,
    pub context_limit: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SessionMessage {
    pub message_id: String,
    pub session_id: String,
    pub role: MessageRole,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SessionSummary {
    pub session_id: String,
    pub title: String,
    /// Quire report bound to this session. `None` until the first run auto-binds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_id: Option<String>,
    /// Frontend-friendly alias of the bound report ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_doc_id: Option<String>,
    pub status: SessionStatus,
    pub active_run_status: ActiveRunStatus,
    pub last_event_seq: u64,
    #[serde(default)]
    pub context_usage: ContextUsage,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SessionDetail {
    #[serde(flatten)]
    pub summary: SessionSummary,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<SessionMessage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<AttachmentMeta>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SendMessageRequest {
    pub client_msg_id: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ToolCallDetail {
    pub tool_call_id: String,
    pub session_id: String,
    pub run_id: String,
    pub tool_name: String,
    pub status: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub output: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub duration_ms: u64,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_detail_roundtrips_and_flattens_summary() {
        let now = Utc::now();
        let detail = SessionDetail {
            summary: SessionSummary {
                session_id: "sess_1".into(),
                title: "Q3 review".into(),
                doc_id: Some("q3-review".into()),
                linked_doc_id: Some("q3-review".into()),
                status: SessionStatus::Active,
                active_run_status: ActiveRunStatus::Idle,
                last_event_seq: 4,
                context_usage: ContextUsage {
                    context_used: 120,
                    context_limit: 200_000,
                    ..ContextUsage::default()
                },
                created_at: now,
                updated_at: now,
            },
            messages: vec![SessionMessage {
                message_id: "msg_1".into(),
                session_id: "sess_1".into(),
                role: MessageRole::User,
                content: "Draft chapter 1".into(),
                attachment_ids: Vec::new(),
                run_id: None,
                created_at: now,
            }],
            attachments: Vec::new(),
        };
        let json = serde_json::to_value(&detail).unwrap();
        assert_eq!(json["session_id"], "sess_1");
        assert_eq!(json["active_run_status"], "idle");
        assert_eq!(json["messages"][0]["role"], "user");
        let decoded: SessionDetail = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, detail);
    }

    #[test]
    fn status_helpers_match_wire_values() {
        assert_eq!(SessionStatus::parse("active"), Some(SessionStatus::Active));
        assert_eq!(
            ActiveRunStatus::parse("interrupted"),
            Some(ActiveRunStatus::Interrupted)
        );
        assert_eq!(
            MessageRole::parse("assistant"),
            Some(MessageRole::Assistant)
        );
        assert!(ActiveRunStatus::parse("bogus").is_none());
    }
}
