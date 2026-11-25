use quire_session_protocol::{ContextUsage, QuestionItem, SessionEventKind};
use serde_json::Value;

use crate::{delta::DeltaKind, model_stream::ModelStreamEvent};

use super::EventScope;

#[derive(Debug, Clone)]
pub struct ToolStartedAction {
    pub scope: EventScope,
    pub tool_call_id: String,
    pub tool_name: String,
    pub input_preview: String,
    pub truncated: bool,
    pub input: Value,
}

#[derive(Debug, Clone)]
pub struct ToolFinishedAction {
    pub scope: EventScope,
    pub tool_call_id: String,
    pub tool_name: String,
    pub status: String,
    pub output_preview: String,
    pub truncated: bool,
    pub duration_ms: u64,
    pub input: Value,
    pub output: Value,
    pub error: Option<String>,
}

/// One classified model event. Side effects are explicit instead of being
/// rediscovered independently by main-agent and subagent match arms.
#[derive(Debug, Clone)]
pub enum EventAction {
    Delta {
        scope: EventScope,
        kind: DeltaKind,
        text: String,
    },
    Emit {
        scope: EventScope,
        kind: SessionEventKind,
    },
    ToolStarted(ToolStartedAction),
    ToolFinished(ToolFinishedAction),
    Question {
        scope: EventScope,
        tool_call_id: Option<String>,
        questions: Vec<QuestionItem>,
    },
    Usage {
        scope: EventScope,
        usage: ContextUsage,
    },
    Ignore {
        reason: &'static str,
    },
}

impl EventAction {
    /// Main-agent text contributes to the persisted assistant message;
    /// subagent text remains inside the child trace.
    pub fn main_text_delta(&self) -> Option<&str> {
        match self {
            Self::Delta {
                scope,
                kind: DeltaKind::Text,
                text,
            } if scope.is_main() => Some(text),
            _ => None,
        }
    }
}

/// Translate one provider event into exactly one delivery action.
///
/// `SubagentEvent` wrappers are flattened here, once, rather than by a second
/// recursive service-side event mapper.
pub fn project(scope: EventScope, event: ModelStreamEvent) -> EventAction {
    match event {
        ModelStreamEvent::ThinkingDelta(delta) => EventAction::Delta {
            scope,
            kind: DeltaKind::Thinking,
            text: delta,
        },
        ModelStreamEvent::TextDelta(delta) => EventAction::Delta {
            scope,
            kind: DeltaKind::Text,
            text: delta,
        },
        ModelStreamEvent::TaskFocus {
            title,
            detail,
            section_id,
            todos,
        } => EventAction::Emit {
            scope,
            kind: SessionEventKind::TaskFocus {
                title,
                detail,
                section_id,
                todos,
            },
        },
        ModelStreamEvent::TodosUpdated { todos } => EventAction::Emit {
            scope,
            kind: SessionEventKind::TodosUpdated { todos },
        },
        ModelStreamEvent::DocLinked { doc_id, title } if scope.is_main() => EventAction::Emit {
            scope,
            kind: SessionEventKind::DocLinked { doc_id, title },
        },
        ModelStreamEvent::DocLinked { .. } => EventAction::Ignore {
            reason: "document/session metadata is only emitted for the main agent",
        },
        ModelStreamEvent::SessionTitleUpdated { title } if scope.is_main() => EventAction::Emit {
            scope,
            kind: SessionEventKind::SessionTitleUpdated { title },
        },
        ModelStreamEvent::SessionTitleUpdated { .. } => EventAction::Ignore {
            reason: "document/session metadata is only emitted for the main agent",
        },
        ModelStreamEvent::SubagentStarted {
            subagent_run_id,
            agent_name,
            display_name,
            task,
            section_id,
        } => {
            // Preserve the existing lifecycle envelope: on the main run the
            // child name is the agent_id; nested lifecycles retain their
            // immediate parent's envelope and the root parent_run_id.
            let wire_scope = if scope.is_main() {
                EventScope {
                    run_id: scope.run_id.clone(),
                    agent_id: agent_name.clone(),
                    parent_run_id: Some(scope.run_id.clone()),
                }
            } else {
                scope
            };
            EventAction::Emit {
                scope: wire_scope,
                kind: SessionEventKind::SubagentStarted {
                    subagent_run_id,
                    agent_name,
                    display_name,
                    task,
                    section_id,
                },
            }
        }
        ModelStreamEvent::SubagentFinished {
            subagent_run_id,
            status,
            summary,
        } => {
            let wire_scope = EventScope {
                run_id: scope.run_id.clone(),
                agent_id: scope.agent_id.clone(),
                parent_run_id: Some(scope.root_run_id().to_string()),
            };
            EventAction::Emit {
                scope: wire_scope,
                kind: SessionEventKind::SubagentFinished {
                    subagent_run_id,
                    status,
                    summary,
                },
            }
        }
        ModelStreamEvent::SubagentEvent {
            subagent_run_id,
            agent_name,
            event,
        } => project(
            EventScope::subagent(&scope, subagent_run_id, agent_name),
            *event,
        ),
        ModelStreamEvent::ToolCallStarted {
            tool_call_id,
            tool_name,
            input_preview,
            truncated,
            input,
        } => EventAction::ToolStarted(ToolStartedAction {
            scope,
            tool_call_id,
            tool_name,
            input_preview,
            truncated,
            input,
        }),
        ModelStreamEvent::ToolCallFinished {
            tool_call_id,
            tool_name,
            status,
            output_preview,
            truncated,
            duration_ms,
            input,
            output,
            error,
        } => EventAction::ToolFinished(ToolFinishedAction {
            scope,
            tool_call_id,
            tool_name,
            status,
            output_preview,
            truncated,
            duration_ms,
            input,
            output,
            error,
        }),
        ModelStreamEvent::QuestionnaireRequested {
            tool_call_id,
            questions,
        } if scope.is_main() => EventAction::Question {
            scope,
            tool_call_id,
            questions,
        },
        ModelStreamEvent::QuestionnaireRequested { .. } => EventAction::Ignore {
            reason: "ask_user is only supported by the main agent",
        },
        ModelStreamEvent::Usage(usage) => EventAction::Usage { scope, usage },
        ModelStreamEvent::Superseded => EventAction::Emit {
            scope,
            kind: SessionEventKind::TurnSuperseded,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_subagent_events_flatten_to_the_innermost_scope() {
        let action = project(
            EventScope::main("run_main"),
            ModelStreamEvent::SubagentEvent {
                subagent_run_id: "run_sub_1".into(),
                agent_name: "alpha".into(),
                event: Box::new(ModelStreamEvent::SubagentEvent {
                    subagent_run_id: "run_sub_2".into(),
                    agent_name: "beta".into(),
                    event: Box::new(ModelStreamEvent::TextDelta("child".into())),
                }),
            },
        );

        assert!(action.main_text_delta().is_none());
        let EventAction::Delta {
            scope,
            kind: DeltaKind::Text,
            text,
        } = action
        else {
            panic!("expected a text delta");
        };
        assert_eq!(scope.run_id, "run_sub_2");
        assert_eq!(scope.agent_id, "beta");
        assert_eq!(scope.parent_run_id.as_deref(), Some("run_main"));
        assert_eq!(text, "child");
    }

    #[test]
    fn subagent_lifecycle_preserves_the_existing_wire_envelopes() {
        let started = project(
            EventScope::main("run_main"),
            ModelStreamEvent::SubagentStarted {
                subagent_run_id: "run_sub_1".into(),
                agent_name: "alpha".into(),
                display_name: None,
                task: "draft".into(),
                section_id: None,
            },
        );
        let EventAction::Emit { scope, .. } = started else {
            panic!("expected an emitted lifecycle event");
        };
        assert_eq!(scope.run_id, "run_main");
        assert_eq!(scope.agent_id, "alpha");
        assert_eq!(scope.parent_run_id.as_deref(), Some("run_main"));

        let finished = project(
            EventScope::main("run_main"),
            ModelStreamEvent::SubagentFinished {
                subagent_run_id: "run_sub_1".into(),
                status: "completed".into(),
                summary: "done".into(),
            },
        );
        let EventAction::Emit { scope, .. } = finished else {
            panic!("expected an emitted lifecycle event");
        };
        assert_eq!(scope.run_id, "run_main");
        assert_eq!(scope.agent_id, "main");
        assert_eq!(scope.parent_run_id.as_deref(), Some("run_main"));
    }

    #[test]
    fn main_only_events_are_explicitly_ignored_for_subagents() {
        let metadata = project(
            EventScope::main("run_main"),
            ModelStreamEvent::SubagentEvent {
                subagent_run_id: "run_sub_1".into(),
                agent_name: "alpha".into(),
                event: Box::new(ModelStreamEvent::DocLinked {
                    doc_id: "doc_1".into(),
                    title: "Report".into(),
                }),
            },
        );
        assert!(matches!(metadata, EventAction::Ignore { .. }));

        let question = project(
            EventScope::main("run_main"),
            ModelStreamEvent::SubagentEvent {
                subagent_run_id: "run_sub_1".into(),
                agent_name: "alpha".into(),
                event: Box::new(ModelStreamEvent::QuestionnaireRequested {
                    tool_call_id: None,
                    questions: vec![],
                }),
            },
        );
        assert!(matches!(question, EventAction::Ignore { .. }));
    }

    #[test]
    fn every_model_event_has_a_main_action() {
        for event in crate::model_stream::sample_model_stream_events() {
            let action = project(EventScope::main("run_main"), event.clone());
            assert!(
                !matches!(action, EventAction::Ignore { .. }),
                "unhandled main model event: {event:?}"
            );
        }
    }

    #[test]
    fn only_main_text_contributes_to_the_assistant_response() {
        let main = project(
            EventScope::main("run_main"),
            ModelStreamEvent::TextDelta("answer".into()),
        );
        assert_eq!(main.main_text_delta(), Some("answer"));

        let thinking = project(
            EventScope::main("run_main"),
            ModelStreamEvent::ThinkingDelta("plan".into()),
        );
        assert!(thinking.main_text_delta().is_none());
    }
}
