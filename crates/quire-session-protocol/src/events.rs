//! Unified Session-SSE envelope and event payloads.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::session::{PendingQueueItem, QuestionAnswer, QuestionItem, TaskTodoItem};

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

    /// Owning run for trace aggregation, or `None` for session-level control/signal frames.
    ///
    /// Subagent activity is grouped under its root run through `parent_run_id`. Session-level
    /// frames never contribute to a run trace, so synthetic run IDs such as `queue` or
    /// `compaction` cannot materialise as phantom traces.
    pub fn trace_run_id(&self) -> Option<&str> {
        if !self.kind.is_run_scoped() {
            return None;
        }
        Some(self.parent_run_id.as_deref().unwrap_or(&self.run_id))
    }
}

/// Event variants carried inside [`SessionEvent`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SessionEventKind {
    TurnStarted,
    TurnFinished,
    TurnSuperseded,
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
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        todos: Vec<TaskTodoItem>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost_rmb: Option<f64>,
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
            Self::TurnSuperseded => "turn_superseded",
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
            Self::PendingQueueUpdated { .. } => "pending_queue_updated",
            Self::CompactionStarted => "compaction_started",
            Self::CompactionFinished { .. } => "compaction_finished",
        }
    }

    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::TurnFinished
                | Self::TurnSuperseded
                | Self::TurnFailed { .. }
                | Self::SyncRequired { .. }
                | Self::QuestionnaireRequested { .. }
        )
    }

    /// Whether the event belongs to a run's durable lifecycle/activity log.
    ///
    /// Session control frames (`pending_queue_updated`, compaction, `sync_required`) and
    /// invalidation signals (`doc_linked`, `session_title_updated`, `user_message_added`,
    /// questionnaire resolution) are excluded: they must not create or mutate run traces, even
    /// when they carry a synthetic `run_id`.
    pub const fn is_run_scoped(&self) -> bool {
        matches!(
            self,
            Self::TurnStarted
                | Self::TurnFinished
                | Self::TurnSuperseded
                | Self::TurnFailed { .. }
                | Self::ThinkingDelta { .. }
                | Self::TextDelta { .. }
                | Self::TaskFocus { .. }
                | Self::ToolCallStarted { .. }
                | Self::ToolCallFinished { .. }
                | Self::SubagentStarted { .. }
                | Self::SubagentFinished { .. }
                | Self::ContextUsage { .. }
                | Self::QuestionnaireRequested { .. }
        )
    }

    /// Every event variant, used by architecture guardrail tests to keep
    /// classification, naming and schema coverage exhaustive.
    pub fn sample_variants() -> Vec<Self> {
        vec![
            Self::TurnStarted,
            Self::TurnFinished,
            Self::TurnSuperseded,
            Self::TurnFailed {
                error: "error".into(),
                recoverable: true,
            },
            Self::SyncRequired {
                after_seq: 0,
                reason: "reason".into(),
            },
            Self::ThinkingDelta {
                delta: "thinking".into(),
            },
            Self::TextDelta {
                delta: "text".into(),
            },
            Self::TaskFocus {
                title: "title".into(),
                detail: None,
                section_id: None,
                todos: Vec::new(),
            },
            Self::ToolCallStarted {
                tool_call_id: "tc_1".into(),
                tool_name: "tool".into(),
                input_preview: "{}".into(),
                truncated: false,
            },
            Self::ToolCallFinished {
                tool_call_id: "tc_1".into(),
                status: "completed".into(),
                output_preview: "ok".into(),
                truncated: false,
                duration_ms: 1,
            },
            Self::SubagentStarted {
                subagent_run_id: "run_sub_1".into(),
                agent_name: "alpha".into(),
                display_name: None,
                task: "task".into(),
                section_id: None,
            },
            Self::SubagentFinished {
                subagent_run_id: "run_sub_1".into(),
                status: "completed".into(),
                summary: "done".into(),
            },
            Self::ContextUsage {
                input_tokens: 1,
                output_tokens: 1,
                cached_tokens: 0,
                context_used: 2,
                context_limit: 100,
                cost_rmb: None,
            },
            Self::QuestionnaireRequested {
                question_id: "q_1".into(),
                tool_call_id: Some("tc_ask".into()),
                questions: vec![],
            },
            Self::QuestionnaireAnswered {
                question_id: "q_1".into(),
                answers: vec![],
                skipped_all: false,
            },
            Self::QuestionnaireDismissed {
                question_id: "q_1".into(),
                reason: "reason".into(),
            },
            Self::DocLinked {
                doc_id: "doc_1".into(),
                title: "title".into(),
            },
            Self::SessionTitleUpdated {
                title: "title".into(),
            },
            Self::UserMessageAdded {
                message_id: "msg_1".into(),
            },
            Self::PendingQueueUpdated { items: vec![] },
            Self::CompactionStarted,
            Self::CompactionFinished { success: true },
        ]
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
                cost_rmb: Some(0.0125),
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
                todos: Vec::new(),
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
    fn schema_exports_every_event_variant() {
        let schema = schemars::schema_for!(SessionEvent);
        let rendered = serde_json::to_string(&schema).unwrap();
        let variants = SessionEventKind::sample_variants();
        assert_eq!(variants.len(), 22);
        for kind in variants {
            let name = kind.event_name();
            assert!(rendered.contains(name), "missing {name} in schema");
        }
    }

    #[test]
    fn sample_variants_have_consistent_scope_classification() {
        for kind in SessionEventKind::sample_variants() {
            let event = SessionEvent::new(1, "run_main", "main", None, kind.clone());
            assert_eq!(
                event.trace_run_id().is_some(),
                kind.is_run_scoped(),
                "scope mismatch for {}",
                kind.event_name()
            );
        }
    }

    #[test]
    fn session_control_and_signal_frames_never_own_a_trace() {
        let queue = SessionEvent::new(
            2,
            "queue",
            "system",
            None,
            SessionEventKind::PendingQueueUpdated { items: vec![] },
        );
        assert_eq!(queue.trace_run_id(), None);

        let compaction = SessionEvent::new(
            3,
            "compaction",
            "main",
            None,
            SessionEventKind::CompactionStarted,
        );
        assert_eq!(compaction.trace_run_id(), None);

        let added = SessionEvent::new(
            4,
            "run_main",
            "queue",
            None,
            SessionEventKind::UserMessageAdded {
                message_id: "msg_1".into(),
            },
        );
        assert_eq!(added.trace_run_id(), None);

        let linked = SessionEvent::new(
            5,
            "run_main",
            "main",
            None,
            SessionEventKind::DocLinked {
                doc_id: "doc_1".into(),
                title: "Report".into(),
            },
        );
        assert_eq!(linked.trace_run_id(), None);
    }

    #[test]
    fn run_scoped_frames_group_subagents_under_the_root_run() {
        let main = SessionEvent::new(1, "run_main", "main", None, SessionEventKind::TurnStarted);
        assert_eq!(main.trace_run_id(), Some("run_main"));

        let subagent = SessionEvent::new(
            2,
            "run_sub_1",
            "alpha",
            Some("run_main".into()),
            SessionEventKind::ThinkingDelta {
                delta: "plan".into(),
            },
        );
        assert_eq!(subagent.trace_run_id(), Some("run_main"));

        let terminal =
            SessionEvent::new(3, "run_main", "main", None, SessionEventKind::TurnFinished);
        assert_eq!(terminal.trace_run_id(), Some("run_main"));
    }
}
