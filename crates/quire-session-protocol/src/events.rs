//! Unified Session-SSE envelope and event payloads.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::session::{PendingQueueItem, QuestionAnswer, QuestionItem};

/// Common envelope for every Session-SSE frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SessionEvent {
    pub seq: u64,
    pub run_id: String,
    /// `"main"` or a stable subagent identifier.
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    #[serde(flatten)]
    pub kind: SessionEventKind,
}

impl SessionEvent {
    pub fn new(
        seq: u64,
        run_id: impl Into<String>,
        agent_id: impl Into<String>,
        parent_run_id: Option<String>,
        kind: SessionEventKind,
    ) -> Self {
        Self {
            seq,
            run_id: run_id.into(),
            agent_id: agent_id.into(),
            parent_run_id,
            timestamp: Utc::now(),
            kind,
        }
    }

    pub const fn event_name(&self) -> &'static str {
        self.kind.event_name()
    }
}

/// Event variants carried inside [`SessionEvent`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SessionEventKind {
    TurnStarted,
    TurnFinished,
    TurnFailed {
        error: String,
        recoverable: bool,
    },
    SyncRequired {
        after_seq: u64,
        reason: String,
    },
    ThinkingDelta {
        delta: String,
    },
    TextDelta {
        delta: String,
    },
    TaskFocus {
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        section_id: Option<String>,
    },
    ToolCallStarted {
        tool_call_id: String,
        tool_name: String,
        input_preview: String,
        truncated: bool,
    },
    ToolCallFinished {
        tool_call_id: String,
        status: String,
        output_preview: String,
        truncated: bool,
        duration_ms: u64,
    },
    SubagentStarted {
        subagent_run_id: String,
        agent_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
        task: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        section_id: Option<String>,
    },
    SubagentFinished {
        subagent_run_id: String,
        status: String,
        summary: String,
    },
    ContextUsage {
        input_tokens: u64,
        output_tokens: u64,
        cached_tokens: u64,
        context_used: u64,
        context_limit: u64,
    },
    QuestionnaireRequested {
        question_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_call_id: Option<String>,
        questions: Vec<QuestionItem>,
    },
    QuestionnaireAnswered {
        question_id: String,
        answers: Vec<QuestionAnswer>,
        skipped_all: bool,
    },
    QuestionnaireDismissed {
        question_id: String,
        reason: String,
    },
    DocLinked {
        doc_id: String,
        title: String,
    },
    SessionTitleUpdated {
        title: String,
    },
    UserMessageAdded {
        message_id: String,
    },
    /// A queued user message injected between tool results and the next model round.
    UserMessageInjected {
        message_id: String,
        content: String,
        created_at: DateTime<Utc>,
    },
    PendingQueueUpdated {
        items: Vec<PendingQueueItem>,
    },
    CompactionStarted,
    CompactionFinished {
        success: bool,
    },
}

impl SessionEventKind {
    pub const fn event_name(&self) -> &'static str {
        match self {
            Self::TurnStarted => "turn_started",
            Self::TurnFinished => "turn_finished",
            Self::TurnFailed { .. } => "turn_failed",
            Self::SyncRequired { .. } => "sync_required",
            Self::ThinkingDelta { .. } => "thinking_delta",
            Self::TextDelta { .. } => "text_delta",
            Self::TaskFocus { .. } => "task_focus",
            Self::ToolCallStarted { .. } => "tool_call_started",
            Self::ToolCallFinished { .. } => "tool_call_finished",
            Self::SubagentStarted { .. } => "subagent_started",
            Self::SubagentFinished { .. } => "subagent_finished",
            Self::ContextUsage { .. } => "context_usage",
            Self::QuestionnaireRequested { .. } => "questionnaire_requested",
            Self::QuestionnaireAnswered { .. } => "questionnaire_answered",
            Self::QuestionnaireDismissed { .. } => "questionnaire_dismissed",
            Self::DocLinked { .. } => "doc_linked",
            Self::SessionTitleUpdated { .. } => "session_title_updated",
            Self::UserMessageAdded { .. } => "user_message_added",
            Self::UserMessageInjected { .. } => "user_message_injected",
            Self::PendingQueueUpdated { .. } => "pending_queue_updated",
            Self::CompactionStarted => "compaction_started",
            Self::CompactionFinished { .. } => "compaction_finished",
        }
    }

    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::TurnFinished
                | Self::TurnFailed { .. }
                | Self::SyncRequired { .. }
                | Self::QuestionnaireRequested { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_event_roundtrips_with_flattened_envelope() {
        let event = SessionEvent::new(
            12,
            "run_main",
            "main",
            None,
            SessionEventKind::ToolCallStarted {
                tool_call_id: "tc_1".into(),
                tool_name: "apply_operations".into(),
                input_preview: "{\"ops\":[".into(),
                truncated: true,
            },
        );
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["event"], "tool_call_started");
        assert_eq!(json["seq"], 12);
        assert_eq!(json["agent_id"], "main");
        assert_eq!(json["tool_call_id"], "tc_1");
        let decoded: SessionEvent = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, event);
        assert_eq!(decoded.event_name(), "tool_call_started");
    }

    #[test]
    fn sync_required_is_terminal_and_roundtrips() {
        let event = SessionEvent::new(
            9,
            "run_main",
            "main",
            Some("run_parent".into()),
            SessionEventKind::SyncRequired {
                after_seq: 8,
                reason: "replay window exceeded".into(),
            },
        );
        assert!(event.kind.is_terminal());
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["event"], "sync_required");
        assert_eq!(json["after_seq"], 8);
        let decoded: SessionEvent = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, event);
    }

    #[test]
    fn context_usage_event_has_token_fields() {
        let event = SessionEvent::new(
            3,
            "run_main",
            "main",
            None,
            SessionEventKind::ContextUsage {
                input_tokens: 100,
                output_tokens: 20,
                cached_tokens: 60,
                context_used: 120,
                context_limit: 200_000,
            },
        );
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["cached_tokens"], 60);
        assert_eq!(json["context_limit"], 200_000);
    }

    #[test]
    fn task_focus_roundtrips_with_frontend_field_names() {
        let event = SessionEvent::new(
            5,
            "run_main",
            "analyst",
            None,
            SessionEventKind::TaskFocus {
                title: "正在绘制人才分布图".into(),
                detail: Some("按学科门类拆分".into()),
                section_id: Some("sec3_h2".into()),
            },
        );
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["event"], "task_focus");
        assert_eq!(json["title"], "正在绘制人才分布图");
        assert_eq!(json["detail"], "按学科门类拆分");
        assert_eq!(json["section_id"], "sec3_h2");
        assert_eq!(event.event_name(), "task_focus");
        let decoded: SessionEvent = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, event);
    }

    #[test]
    fn questionnaire_requested_is_terminal_and_roundtrips() {
        let event = SessionEvent::new(
            4,
            "run_main",
            "main",
            None,
            SessionEventKind::QuestionnaireRequested {
                question_id: "q_1".into(),
                tool_call_id: Some("run_main:call_1".into()),
                questions: vec![crate::session::QuestionItem {
                    id: "scope".into(),
                    title: "Which scope?".into(),
                    description: None,
                    multiple: false,
                    required: true,
                    options: vec![crate::session::QuestionOption {
                        id: "q3".into(),
                        label: "Q3".into(),
                        description: None,
                    }],
                }],
            },
        );
        assert!(event.kind.is_terminal());
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["event"], "questionnaire_requested");
        assert_eq!(json["questions"][0]["id"], "scope");
        let decoded: SessionEvent = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, event);
    }

    #[test]
    fn user_message_injected_roundtrips_with_message_timestamp() {
        let created_at = Utc::now();
        let event = SessionEvent::new(
            11,
            "run_main",
            "main",
            None,
            SessionEventKind::UserMessageInjected {
                message_id: "msg_injected".into(),
                content: "补充风险提示".into(),
                created_at,
            },
        );
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["event"], "user_message_injected");
        assert_eq!(json["message_id"], "msg_injected");
        assert_eq!(json["content"], "补充风险提示");
        let decoded: SessionEvent = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, event);
    }

    #[test]
    fn schema_exports_every_event_variant() {
        let schema = schemars::schema_for!(SessionEvent);
        let rendered = serde_json::to_string(&schema).unwrap();
        for event in [
            "turn_started",
            "turn_finished",
            "turn_failed",
            "sync_required",
            "thinking_delta",
            "text_delta",
            "task_focus",
            "tool_call_started",
            "tool_call_finished",
            "subagent_started",
            "subagent_finished",
            "context_usage",
            "questionnaire_requested",
            "questionnaire_answered",
            "questionnaire_dismissed",
            "doc_linked",
            "session_title_updated",
            "user_message_added",
            "user_message_injected",
            "pending_queue_updated",
            "compaction_started",
            "compaction_finished",
        ] {
            assert!(rendered.contains(event), "missing {event} in schema");
        }
    }
}
