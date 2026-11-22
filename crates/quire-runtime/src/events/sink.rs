use std::time::Instant;

use chrono::Utc;
use quire_session_protocol::{
    ContextUsage, PendingQuestion, SessionEvent, SessionEventKind, ToolCallDetail,
};
use serde_json::Value;

use crate::{
    delta::{CoalescedDelta, DeltaCoalescer, DeltaKind},
    error::RuntimeResult,
};

use super::{
    EventScope, SessionEventLog,
    project::{EventAction, ToolFinishedAction, ToolStartedAction},
};

/// Result of applying one model event to the session log.
#[derive(Debug, Default)]
pub struct ApplyOutcome {
    /// Set when an `ask_user` question was created and emitted.
    pub question: Option<PendingQuestion>,
    /// Only main-agent usage updates the session context meter.
    pub main_usage: Option<ContextUsage>,
    /// A model event was deliberately out of scope and ignored.
    pub ignored: Option<&'static str>,
}

/// Single owner of delta coalescing, structural flush ordering and event side effects.
///
/// No caller needs to remember to flush before a structural event: `emit` and
/// every non-delta action do it here, once.
pub struct EventSink<L> {
    session_id: String,
    log: L,
    coalescer: DeltaCoalescer,
}

impl<L: SessionEventLog> EventSink<L> {
    pub fn new(session_id: impl Into<String>, log: L) -> Self {
        Self {
            session_id: session_id.into(),
            log,
            coalescer: DeltaCoalescer::default(),
        }
    }

    pub async fn flush(&mut self) -> RuntimeResult<()> {
        let deltas = self.coalescer.flush_all();
        self.emit_coalesced(deltas).await
    }

    pub async fn flush_due(&mut self, now: Instant) -> RuntimeResult<()> {
        let deltas = self.coalescer.flush_due(now);
        self.emit_coalesced(deltas).await
    }

    /// Persist a structural event after every preceding delta has been flushed.
    pub async fn emit(
        &mut self,
        scope: EventScope,
        kind: SessionEventKind,
    ) -> RuntimeResult<SessionEvent> {
        self.flush().await?;
        self.log.append_event(&self.session_id, &scope, kind).await
    }

    pub async fn apply(&mut self, action: EventAction) -> RuntimeResult<ApplyOutcome> {
        match action {
            EventAction::Delta { scope, kind, text } => {
                let deltas = self.coalescer.push(Instant::now(), &scope, kind, &text);
                self.emit_coalesced(deltas).await?;
                Ok(ApplyOutcome::default())
            }
            EventAction::Emit { scope, kind } => {
                self.emit(scope, kind).await?;
                Ok(ApplyOutcome::default())
            }
            EventAction::ToolStarted(action) => {
                self.tool_started(action).await?;
                Ok(ApplyOutcome::default())
            }
            EventAction::ToolFinished(action) => {
                self.tool_finished(action).await?;
                Ok(ApplyOutcome::default())
            }
            EventAction::Question {
                scope,
                tool_call_id,
                questions,
            } => {
                self.flush().await?;
                let (question, _event) = self
                    .log
                    .create_question_with_event(
                        &self.session_id,
                        &scope,
                        tool_call_id.as_deref(),
                        &questions,
                    )
                    .await?;
                Ok(ApplyOutcome {
                    question: Some(question),
                    ..ApplyOutcome::default()
                })
            }
            EventAction::Usage { scope, usage } => {
                let main_usage = scope.is_main().then(|| usage.clone());
                let kind = SessionEventKind::ContextUsage {
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    cached_tokens: usage.cached_tokens,
                    context_used: usage.context_used,
                    context_limit: usage.context_limit,
                    cost_rmb: usage.cost_rmb,
                };
                self.emit(scope, kind).await?;
                Ok(ApplyOutcome {
                    main_usage,
                    ..ApplyOutcome::default()
                })
            }
            EventAction::Ignore { reason } => Ok(ApplyOutcome {
                ignored: Some(reason),
                ..ApplyOutcome::default()
            }),
        }
    }

    async fn tool_started(&mut self, action: ToolStartedAction) -> RuntimeResult<()> {
        let ToolStartedAction {
            scope,
            tool_call_id,
            tool_name,
            input_preview,
            truncated,
            input,
        } = action;
        self.flush().await?;
        let detail = ToolCallDetail {
            tool_call_id: tool_call_id.clone(),
            session_id: self.session_id.clone(),
            run_id: scope.run_id.clone(),
            tool_name: tool_name.clone(),
            status: "running".into(),
            input,
            output: Value::Null,
            error: None,
            duration_ms: 0,
            created_at: Utc::now(),
        };
        let kind = SessionEventKind::ToolCallStarted {
            tool_call_id,
            tool_name,
            input_preview,
            truncated,
        };
        self.log
            .save_tool_call_with_event(&detail, &scope, kind)
            .await?;
        Ok(())
    }

    async fn tool_finished(&mut self, action: ToolFinishedAction) -> RuntimeResult<()> {
        let ToolFinishedAction {
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
        } = action;
        self.flush().await?;
        let detail = ToolCallDetail {
            tool_call_id: tool_call_id.clone(),
            session_id: self.session_id.clone(),
            run_id: scope.run_id.clone(),
            tool_name,
            status: status.clone(),
            input,
            output,
            error,
            duration_ms,
            created_at: Utc::now(),
        };
        let kind = SessionEventKind::ToolCallFinished {
            tool_call_id,
            status,
            output_preview,
            truncated,
            duration_ms,
        };
        self.log
            .save_tool_call_with_event(&detail, &scope, kind)
            .await?;
        Ok(())
    }

    async fn emit_coalesced(&mut self, deltas: Vec<CoalescedDelta>) -> RuntimeResult<()> {
        if deltas.is_empty() {
            return Ok(());
        }
        let events = deltas
            .into_iter()
            .map(|delta| {
                let kind = match delta.kind {
                    DeltaKind::Thinking => SessionEventKind::ThinkingDelta { delta: delta.delta },
                    DeltaKind::Text => SessionEventKind::TextDelta { delta: delta.delta },
                };
                (delta.scope, kind)
            })
            .collect();
        self.log.append_events(&self.session_id, events).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use quire_session_protocol::QuestionItem;
    use serde_json::json;

    use crate::error::RuntimeError;

    #[derive(Debug, Clone)]
    enum RecordedCall {
        Append(SessionEvent),
        SaveTool(ToolCallDetail),
    }

    #[derive(Clone, Default)]
    struct FakeLog {
        calls: Arc<Mutex<Vec<RecordedCall>>>,
        seq: Arc<Mutex<u64>>,
        fail_save: bool,
    }

    impl FakeLog {
        fn calls(&self) -> Vec<RecordedCall> {
            self.calls.lock().unwrap().clone()
        }

        fn events(&self) -> Vec<SessionEvent> {
            self.calls()
                .into_iter()
                .filter_map(|call| match call {
                    RecordedCall::Append(event) => Some(event),
                    RecordedCall::SaveTool(_) => None,
                })
                .collect()
        }
    }

    #[async_trait::async_trait]
    impl SessionEventLog for FakeLog {
        async fn append_event(
            &self,
            _session_id: &str,
            scope: &EventScope,
            kind: SessionEventKind,
        ) -> RuntimeResult<SessionEvent> {
            let mut seq = self.seq.lock().unwrap();
            *seq += 1;
            let event = SessionEvent::new(
                *seq,
                &scope.run_id,
                &scope.agent_id,
                scope.parent_run_id.clone(),
                kind,
            );
            self.calls
                .lock()
                .unwrap()
                .push(RecordedCall::Append(event.clone()));
            Ok(event)
        }

        async fn save_tool_call(&self, detail: &ToolCallDetail) -> RuntimeResult<()> {
            if self.fail_save {
                return Err(RuntimeError::Internal("tool save failed".into()));
            }
            self.calls
                .lock()
                .unwrap()
                .push(RecordedCall::SaveTool(detail.clone()));
            Ok(())
        }

        async fn create_question(
            &self,
            _session_id: &str,
            run_id: &str,
            tool_call_id: Option<&str>,
            questions: &[QuestionItem],
        ) -> RuntimeResult<PendingQuestion> {
            Ok(PendingQuestion {
                question_id: "q_1".into(),
                run_id: run_id.into(),
                tool_call_id: tool_call_id.map(str::to_string),
                questions: questions.to_vec(),
                created_at: Utc::now(),
            })
        }
    }

    fn usage() -> ContextUsage {
        ContextUsage {
            input_tokens: 100,
            output_tokens: 20,
            cached_tokens: 10,
            context_used: 120,
            context_limit: 200_000,
            cost_rmb: None,
        }
    }

    fn tool_started(scope: EventScope) -> EventAction {
        EventAction::ToolStarted(ToolStartedAction {
            scope,
            tool_call_id: "tc_1".into(),
            tool_name: "apply_operations".into(),
            input_preview: "{\"ops\":[".into(),
            truncated: true,
            input: json!({"ops": [{"type": "append_to_doc"}]}),
        })
    }

    #[tokio::test]
    async fn structural_events_flush_deltas_before_appending() {
        let log = FakeLog::default();
        let mut sink = EventSink::new("sess_1", log.clone());
        let main = EventScope::main("run_main");
        sink.apply(EventAction::Delta {
            scope: main.clone(),
            kind: DeltaKind::Thinking,
            text: "plan".into(),
        })
        .await
        .unwrap();
        assert!(log.events().is_empty(), "the small delta is buffered");

        sink.emit(
            main,
            SessionEventKind::TaskFocus {
                title: "Draft".into(),
                detail: None,
                section_id: None,
            },
        )
        .await
        .unwrap();

        let events = log.events();
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0].kind,
            SessionEventKind::ThinkingDelta { .. }
        ));
        assert!(matches!(events[1].kind, SessionEventKind::TaskFocus { .. }));
        assert_eq!(events[0].seq, 1);
        assert_eq!(events[1].seq, 2);
    }

    #[tokio::test]
    async fn subagent_deltas_are_coalesced_with_the_root_parent_scope() {
        let log = FakeLog::default();
        let mut sink = EventSink::new("sess_1", log.clone());
        let main = EventScope::main("run_main");
        let child = EventScope::subagent(&main, "run_sub_1", "alpha");
        for text in ["child ", "output"] {
            sink.apply(EventAction::Delta {
                scope: child.clone(),
                kind: DeltaKind::Text,
                text: text.into(),
            })
            .await
            .unwrap();
        }
        assert!(log.events().is_empty(), "subagent deltas are buffered too");

        sink.flush().await.unwrap();
        let events = log.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].run_id, "run_sub_1");
        assert_eq!(events[0].agent_id, "alpha");
        assert_eq!(events[0].parent_run_id.as_deref(), Some("run_main"));
        assert!(matches!(
            &events[0].kind,
            SessionEventKind::TextDelta { delta } if delta == "child output"
        ));
    }

    #[tokio::test]
    async fn tool_start_saves_full_detail_before_the_wire_event() {
        let log = FakeLog::default();
        let mut sink = EventSink::new("sess_1", log.clone());
        sink.apply(tool_started(EventScope::main("run_main")))
            .await
            .unwrap();

        let calls = log.calls();
        assert_eq!(calls.len(), 2);
        let RecordedCall::SaveTool(detail) = &calls[0] else {
            panic!("tool detail must be saved first");
        };
        assert_eq!(detail.status, "running");
        assert_eq!(detail.input["ops"][0]["type"], "append_to_doc");
        let RecordedCall::Append(event) = &calls[1] else {
            panic!("the wire event follows the tool detail");
        };
        assert!(matches!(
            &event.kind,
            SessionEventKind::ToolCallStarted { truncated, .. } if *truncated
        ));
    }

    #[tokio::test]
    async fn tool_persistence_failure_does_not_emit_a_wire_event() {
        let log = FakeLog {
            fail_save: true,
            ..FakeLog::default()
        };
        let mut sink = EventSink::new("sess_1", log.clone());
        let result = sink.apply(tool_started(EventScope::main("run_main"))).await;

        assert!(result.is_err());
        assert!(log.events().is_empty());
    }

    #[tokio::test]
    async fn only_main_usage_is_returned_for_session_accounting() {
        let log = FakeLog::default();
        let mut sink = EventSink::new("sess_1", log.clone());
        let main = EventScope::main("run_main");
        let child = EventScope::subagent(&main, "run_sub_1", "alpha");

        let main_outcome = sink
            .apply(EventAction::Usage {
                scope: main,
                usage: usage(),
            })
            .await
            .unwrap();
        assert_eq!(main_outcome.main_usage, Some(usage()));

        let child_outcome = sink
            .apply(EventAction::Usage {
                scope: child,
                usage: usage(),
            })
            .await
            .unwrap();
        assert_eq!(child_outcome.main_usage, None);
        assert_eq!(
            log.events().len(),
            2,
            "both scopes still emit their usage frame"
        );
    }

    #[tokio::test]
    async fn question_creation_supplies_the_wire_question_id() {
        let log = FakeLog::default();
        let mut sink = EventSink::new("sess_1", log.clone());
        let outcome = sink
            .apply(EventAction::Question {
                scope: EventScope::main("run_main"),
                tool_call_id: Some("tc_ask".into()),
                questions: vec![],
            })
            .await
            .unwrap();

        assert_eq!(
            outcome.question.map(|question| question.question_id),
            Some("q_1".into())
        );
        let events = log.events();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0].kind,
            SessionEventKind::QuestionnaireRequested { question_id, tool_call_id, .. }
                if question_id == "q_1" && tool_call_id.as_deref() == Some("tc_ask")
        ));
    }

    #[tokio::test]
    async fn ignored_events_do_not_write_to_the_log() {
        let log = FakeLog::default();
        let mut sink = EventSink::new("sess_1", log.clone());
        let outcome = sink
            .apply(EventAction::Ignore {
                reason: "out of scope",
            })
            .await
            .unwrap();

        assert_eq!(outcome.ignored, Some("out of scope"));
        assert!(log.events().is_empty());
    }
}
