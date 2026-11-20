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
    AwaitingInput,
}

impl ActiveRunStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
            Self::AwaitingInput => "awaiting_input",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "idle" => Some(Self::Idle),
            "running" => Some(Self::Running),
            "interrupted" => Some(Self::Interrupted),
            "failed" => Some(Self::Failed),
            "awaiting_input" => Some(Self::AwaitingInput),
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
    Compaction,
}

impl MessageRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
            Self::System => "system",
            Self::Compaction => "compaction",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            "tool" => Some(Self::Tool),
            "system" => Some(Self::System),
            "compaction" => Some(Self::Compaction),
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

/// Why a message is waiting in the per-session queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PendingQueueSource {
    User,
    Comment,
}

impl PendingQueueSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Comment => "comment",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "comment" => Some(Self::Comment),
            _ => None,
        }
    }
}

/// One message waiting for the session's active run to finish.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PendingQueueItem {
    pub queue_id: String,
    pub session_id: String,
    pub content: String,
    pub source: PendingQueueSource,
    /// Comment thread that produced this item, when `source` is `comment`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
    pub created_at: DateTime<Utc>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct QuestionOption {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionItem {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub options: Vec<QuestionOption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionAnswer {
    pub question_id: String,
    #[serde(default)]
    pub selected_option_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freeform_text: Option<String>,
    #[serde(default)]
    pub skipped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PendingQuestion {
    pub question_id: String,
    pub run_id: String,
    /// The `ask_user` tool call that opened this question, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    pub questions: Vec<QuestionItem>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunTaskFocus {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunTraceToolItem {
    pub tool_call_id: String,
    pub tool_name: String,
    pub status: String,
    pub input_preview: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunTraceSubagent {
    pub subagent_run_id: String,
    pub agent_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_id: Option<String>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub thinking: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_focus: Option<RunTaskFocus>,
    #[serde(default)]
    pub tools: Vec<RunTraceToolItem>,
}

/// One ordered slice of a run. Text output closes the preceding thinking and
/// execution phase, so later reasoning or tools start a new segment instead of
/// being folded back into an older block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunTraceSegment {
    Thinking {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
    },
    Execution {
        #[serde(default)]
        tools: Vec<RunTraceToolItem>,
        #[serde(default)]
        subagents: Vec<RunTraceSubagent>,
    },
    Text {
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunTraceSummary {
    pub run_id: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_focus: Option<RunTaskFocus>,
    #[serde(default)]
    pub segments: Vec<RunTraceSegment>,
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
    /// Knowledge-base branch used by this session. `None` only for sessions created before
    /// branch-scoped knowledge bases existed; those fall back to the current active branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kb_branch: Option<String>,
    pub status: SessionStatus,
    pub active_run_status: ActiveRunStatus,
    pub last_event_seq: u64,
    #[serde(default)]
    pub context_usage: ContextUsage,
    #[serde(default)]
    pub compacting: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compacted_at: Option<DateTime<Utc>>,
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
    /// Latest unresolved question card, restored after refresh or reconnect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_question: Option<PendingQuestion>,
    /// Answered, skipped and dismissed question cards in creation order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resolved_questions: Vec<ResolvedQuestion>,
    /// Messages waiting for the current run to finish, in FIFO order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_queue: Vec<PendingQueueItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ResolvedQuestion {
    pub question: PendingQuestion,
    pub status: String,
    #[serde(default)]
    pub answers: Vec<QuestionAnswer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateSessionRequest {
    pub title: String,
    /// Optional existing report to bind immediately.
    #[serde(default)]
    pub doc_id: Option<String>,
    /// Test a draft knowledge-base branch. Omit to use the current active branch.
    #[serde(default)]
    pub kb_branch: Option<String>,
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
                kb_branch: Some("main".into()),
                status: SessionStatus::Active,
                active_run_status: ActiveRunStatus::Idle,
                last_event_seq: 4,
                context_usage: ContextUsage {
                    context_used: 120,
                    context_limit: 200_000,
                    ..ContextUsage::default()
                },
                compacting: false,
                compacted_at: None,
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
            resolved_questions: Vec::new(),
            pending_question: Some(PendingQuestion {
                question_id: "q_1".into(),
                run_id: "run_1".into(),
                tool_call_id: Some("run_1:call_1".into()),
                questions: vec![QuestionItem {
                    id: "scope".into(),
                    title: "Which scope?".into(),
                    description: None,
                    multiple: false,
                    required: true,
                    options: vec![QuestionOption {
                        id: "q3".into(),
                        label: "Q3".into(),
                        description: None,
                    }],
                }],
                created_at: now,
            }),
            pending_queue: Vec::new(),
        };
        let json = serde_json::to_value(&detail).unwrap();
        assert_eq!(json["session_id"], "sess_1");
        assert_eq!(json["active_run_status"], "idle");
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["pending_question"]["question_id"], "q_1");
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
            ActiveRunStatus::parse("awaiting_input"),
            Some(ActiveRunStatus::AwaitingInput)
        );
        assert_eq!(
            MessageRole::parse("assistant"),
            Some(MessageRole::Assistant)
        );
        assert!(ActiveRunStatus::parse("bogus").is_none());
    }
}
