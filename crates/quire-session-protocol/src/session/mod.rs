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

/// Immutable send-time snapshot of a report comment handed to the agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionCommentRef {
    pub thread_id: String,
    pub block_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_block_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchor_blocks: Vec<String>,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_quote: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_snapshot: Option<String>,
    pub comment: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ContextUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub context_used: u64,
    pub context_limit: u64,
    /// Cumulative session cost in RMB (CNY) across main agent, subagents, and compaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_rmb: Option<f64>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comment_refs: Vec<SessionCommentRef>,
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
    pub comment_refs: Vec<SessionCommentRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// One selectable option inside an `ask_user` `QuestionItem`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct QuestionOption {
    /// Stable short identifier for this option within the question (e.g. `"opt_exec"` or `"30k"`).
    pub id: String,
    /// Concise user-facing option label (put your recommended choice first and suffix it with `"(Recommended)"` / `"（推荐）"`).
    pub label: String,
    /// Optional 1-sentence explanation of the trade-off or implication of choosing this option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One structured question card rendered by `ask_user` (`1..=4` questions per call).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionItem {
    /// Stable short identifier for this question (e.g. `"report_scope"` or `"target_length"`).
    pub id: String,
    /// Direct, concise question prompt shown to the user.
    pub title: String,
    /// Optional brief context explaining why this decision matters for the report.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Allow selecting multiple options (`true`) or enforce single-choice (`false`, default).
    #[serde(default)]
    pub multiple: bool,
    /// Whether the user must answer this question before submitting (`false` by default).
    #[serde(default)]
    pub required: bool,
    /// Concrete selectable options (2–4 distinct choices recommended; the UI automatically provides a freeform custom text input alongside these options, so do not add a blank `"Other"` option).
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QuestionnaireResolutionStatus {
    Answered,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResolvedQuestionItemAnswer {
    pub question_id: String,
    pub question_title: String,
    pub selected_option_ids: Vec<String>,
    pub selected_option_labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freeform_text: Option<String>,
    pub skipped: bool,
}

/// The only model input produced when an ask_user card is answered or skipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AskUserAnswerPayload {
    pub kind: String,
    pub question_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    pub status: QuestionnaireResolutionStatus,
    pub skipped_all: bool,
    pub answers: Vec<ResolvedQuestionItemAnswer>,
}

impl AskUserAnswerPayload {
    pub const KIND: &'static str = "ask_user_response";

    pub fn from_resolution(
        question: &PendingQuestion,
        answers: &[QuestionAnswer],
        skipped_all: bool,
    ) -> Self {
        let answers = if skipped_all {
            Vec::new()
        } else {
            answers
                .iter()
                .map(|answer| {
                    let definition = question
                        .questions
                        .iter()
                        .find(|item| item.id == answer.question_id);
                    let selected_option_labels = answer
                        .selected_option_ids
                        .iter()
                        .filter_map(|selected| {
                            definition
                                .and_then(|item| {
                                    item.options.iter().find(|option| option.id == *selected)
                                })
                                .map(|option| option.label.clone())
                        })
                        .collect();
                    ResolvedQuestionItemAnswer {
                        question_id: answer.question_id.clone(),
                        question_title: definition
                            .map(|item| item.title.clone())
                            .unwrap_or_default(),
                        selected_option_ids: answer.selected_option_ids.clone(),
                        selected_option_labels,
                        freeform_text: answer.freeform_text.clone(),
                        skipped: answer.skipped,
                    }
                })
                .collect()
        };
        Self {
            kind: Self::KIND.to_string(),
            question_id: question.question_id.clone(),
            tool_call_id: question.tool_call_id.clone(),
            status: if skipped_all {
                QuestionnaireResolutionStatus::Skipped
            } else {
                QuestionnaireResolutionStatus::Answered
            },
            skipped_all,
            answers,
        }
    }
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
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Ordered subagent activity, isomorphic to the main run's segments. Text
    /// closes the preceding thinking/execution phase, so multi-round tool use
    /// keeps the same order in live and hydrated views.
    #[serde(default)]
    pub segments: Vec<RunTraceSegment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_focus: Option<RunTaskFocus>,
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
        /// Server timestamp when this thinking phase started. Retained while the
        /// phase is still streaming so clients can continue the timer after refresh.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        started_at: Option<DateTime<Utc>>,
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
    /// Server timestamp of the run's `turn_started` event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_at: Option<DateTime<Utc>>,
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
    /// Aggregated trace of the active run, captured at `last_event_seq`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_run_trace: Option<RunTraceSummary>,
    /// Subagents embedded in the active run, with their streaming clocks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_subagent_traces: Vec<RunTraceSubagent>,
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
    pub comment_refs: Vec<SessionCommentRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatePendingMessageRequest {
    pub content: String,
    #[serde(default)]
    pub comment_refs: Vec<SessionCommentRef>,
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
mod tests;
