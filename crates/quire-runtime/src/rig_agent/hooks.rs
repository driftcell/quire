//! Run control and tool lifecycle hooks for the native Rig runner.

use std::{collections::HashMap, sync::Arc, time::Instant};

use rig::{
    agent::{
        AgentHook, CompletionCallAction, CompletionCallEvent, DispatchAction, DispatchEvent,
        HookContext, InvalidToolCallAction, InvalidToolCallContext, InvalidToolCallReason,
        ModelTurnAction, ModelTurnFinished, ObservationAction, OutcomeAction, OutcomeEvent,
        ReasoningDelta, RequestPatch, RunStart, RunStartAction, TextDelta, ToolCallDelta,
    },
    completion::{Document, Usage},
    message::AssistantContent,
};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::{kb::KbService, model_stream::ModelStreamEvent, service::PendingQueueCoordinator};

use super::{
    AgentExecution, StreamEventSender,
    context::AskUserRequestedMeta,
    current_doc_id, preview_json,
    preview_stream::{PreviewState, inject_preview_ids},
    tool_output_value, update_doc_id,
};

pub(super) const CANCELLED_REASON: &str = "quire:cancelled";
pub(super) const SUPERSEDED_REASON: &str = "quire:superseded";
pub(super) const ASK_USER_REASON: &str = "quire:ask_user";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum RunTermination {
    #[default]
    None,
    Cancelled,
    Superseded,
    AskUser,
}

#[derive(Default)]
pub(super) struct TurnRunState {
    pub(super) termination: RunTermination,
    pub(super) last_usage: Option<Usage>,
    turn_has_ask_user: bool,
    started: HashMap<String, Instant>,
}

pub(super) type SharedTurnState = Arc<Mutex<TurnRunState>>;

async fn terminate(state: &SharedTurnState, termination: RunTermination) {
    state.lock().await.termination = termination;
}

/// Owns model-call control, cancellation, queue supersession, usage and streamed text.
pub(super) struct TurnControlHook {
    execution: AgentExecution,
    scope: StreamEventSender,
    queue: Option<PendingQueueCoordinator>,
    kb_update_notice: Option<String>,
    state: SharedTurnState,
}

impl TurnControlHook {
    pub(super) fn new(
        execution: AgentExecution,
        scope: StreamEventSender,
        queue: PendingQueueCoordinator,
        kb_update_notice: Option<String>,
        state: SharedTurnState,
    ) -> Self {
        Self {
            execution,
            scope,
            queue: Some(queue),
            kb_update_notice,
            state,
        }
    }

    #[cfg(test)]
    pub(super) fn for_test(
        execution: AgentExecution,
        scope: StreamEventSender,
        state: SharedTurnState,
    ) -> Self {
        Self {
            execution,
            scope,
            queue: None,
            kb_update_notice: None,
            state,
        }
    }

    pub(super) fn session_documents(&self) -> Vec<Document> {
        let doc_id = current_doc_id(&self.execution.doc_id);
        let text = match doc_id {
            Some(doc_id) => format!(
                "<runtime_session_state>\ndoc_id: {doc_id}\nUse this doc_id for document tools and do not call create_report for this session.\n</runtime_session_state>"
            ),
            None if self.execution.run_id.starts_with("run_sub_") =>
                "<runtime_session_state>\ndoc_id: none\n</runtime_session_state>".to_string(),
            None =>
                "<runtime_session_state>\ndoc_id: none\nNo Quire report is bound yet. If the user asks to write or generate a report, choose an appropriate formal report title and call create_report first; for pure Q&A, reply directly without creating a report.\n</runtime_session_state>".to_string(),
        };
        let mut documents = vec![Document {
            id: "quire-session-state".into(),
            text,
            additional_props: Default::default(),
        }];
        if let Some(notice) = self.kb_update_notice.as_ref() {
            documents.push(Document {
                id: "quire-knowledge-update".into(),
                text: notice.clone(),
                additional_props: Default::default(),
            });
        }
        documents
    }
}

impl AgentHook for TurnControlHook {
    async fn on_run_start(&self, _ctx: &HookContext, _event: RunStart<'_>) -> RunStartAction {
        if self.execution.cancellation.is_cancelled() {
            terminate(&self.state, RunTermination::Cancelled).await;
            RunStartAction::stop(CANCELLED_REASON)
        } else {
            RunStartAction::continue_run()
        }
    }

    async fn on_completion_call(
        &self,
        ctx: &HookContext,
        _event: CompletionCallEvent<'_>,
    ) -> CompletionCallAction {
        if self.execution.cancellation.is_cancelled() {
            terminate(&self.state, RunTermination::Cancelled).await;
            return CompletionCallAction::stop(CANCELLED_REASON);
        }
        if self.state.lock().await.termination == RunTermination::AskUser {
            return CompletionCallAction::stop(ASK_USER_REASON);
        }
        if ctx.turn() > 1
            && !self.execution.run_id.starts_with("run_sub_")
            && let Some(queue) = self.queue.as_ref()
            && queue
                .has_pending(&self.execution.session_id)
                .await
                .unwrap_or(false)
        {
            terminate(&self.state, RunTermination::Superseded).await;
            return CompletionCallAction::stop(SUPERSEDED_REASON);
        }
        CompletionCallAction::patch(RequestPatch::new().extra_context(self.session_documents()))
    }

    async fn on_model_turn_finished(
        &self,
        _ctx: &HookContext,
        event: ModelTurnFinished<'_>,
    ) -> ModelTurnAction {
        self.state.lock().await.last_usage = Some(event.usage);
        if self.execution.cancellation.is_cancelled() {
            terminate(&self.state, RunTermination::Cancelled).await;
            ModelTurnAction::stop(CANCELLED_REASON)
        } else {
            ModelTurnAction::continue_run()
        }
    }

    async fn on_text_delta(&self, _ctx: &HookContext, event: TextDelta<'_>) -> ObservationAction {
        if self.execution.cancellation.is_cancelled() {
            terminate(&self.state, RunTermination::Cancelled).await;
            return ObservationAction::stop(CANCELLED_REASON);
        }
        self.scope
            .send(ModelStreamEvent::TextDelta(event.delta.to_string()))
            .await;
        ObservationAction::continue_run()
    }

    async fn on_reasoning_delta(
        &self,
        _ctx: &HookContext,
        event: ReasoningDelta<'_>,
    ) -> ObservationAction {
        if self.execution.cancellation.is_cancelled() {
            terminate(&self.state, RunTermination::Cancelled).await;
            return ObservationAction::stop(CANCELLED_REASON);
        }
        self.scope
            .send(ModelStreamEvent::ThinkingDelta(event.delta.to_string()))
            .await;
        ObservationAction::continue_run()
    }

    async fn on_tool_call_delta(
        &self,
        _ctx: &HookContext,
        _event: ToolCallDelta<'_>,
    ) -> ObservationAction {
        if self.execution.cancellation.is_cancelled() {
            terminate(&self.state, RunTermination::Cancelled).await;
            ObservationAction::stop(CANCELLED_REASON)
        } else {
            ObservationAction::continue_run()
        }
    }

    async fn on_dispatch(&self, _ctx: &HookContext, _event: DispatchEvent<'_>) -> DispatchAction {
        if self.execution.cancellation.is_cancelled() {
            terminate(&self.state, RunTermination::Cancelled).await;
            DispatchAction::stop(CANCELLED_REASON)
        } else {
            DispatchAction::proceed()
        }
    }
}

/// Sole owner of tool preview, audit, suspension, invalid-call recovery and report binding.
pub(super) struct ToolLifecycleHook {
    execution: AgentExecution,
    scope: StreamEventSender,
    kb: Option<KbService>,
    previews: Arc<Mutex<PreviewState>>,
    state: SharedTurnState,
}

impl ToolLifecycleHook {
    pub(super) fn new(
        execution: AgentExecution,
        scope: StreamEventSender,
        kb: KbService,
        previews: Arc<Mutex<PreviewState>>,
        state: SharedTurnState,
    ) -> Self {
        Self {
            execution,
            scope,
            kb: Some(kb),
            previews,
            state,
        }
    }

    #[cfg(test)]
    pub(super) fn for_test(
        execution: AgentExecution,
        scope: StreamEventSender,
        previews: Arc<Mutex<PreviewState>>,
        state: SharedTurnState,
    ) -> Self {
        Self {
            execution,
            scope,
            kb: None,
            previews,
            state,
        }
    }

    fn arguments(raw: Option<&str>) -> Value {
        raw.and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or(Value::Null)
    }

    async fn send_started(&self, tool_call_id: String, tool_name: &str, arguments: Value) {
        let (input_preview, truncated) = preview_json(&arguments);
        self.state
            .lock()
            .await
            .started
            .insert(tool_call_id.clone(), Instant::now());
        self.scope
            .send(ModelStreamEvent::ToolCallStarted {
                tool_call_id,
                tool_name: tool_name.to_string(),
                input_preview,
                truncated,
                input: arguments,
            })
            .await;
    }

    async fn bind_created_report(&self, arguments: &Value, output: &Value) {
        let Some(doc_id) = output.get("doc_id").and_then(Value::as_str) else {
            return;
        };
        let title = output
            .get("title")
            .and_then(Value::as_str)
            .or_else(|| arguments.get("title").and_then(Value::as_str))
            .unwrap_or("未命名报告")
            .to_string();
        if let Some(kb) = self.kb.as_ref()
            && let Err(error) = kb
                .store()
                .bind_doc(&self.execution.session_id, doc_id)
                .await
        {
            tracing::warn!(%error, session_id = %self.execution.session_id, "failed to bind lazily created report");
            return;
        }
        update_doc_id(&self.execution.doc_id, Some(doc_id.to_string()));
        self.scope
            .send(ModelStreamEvent::DocLinked {
                doc_id: doc_id.to_string(),
                title,
            })
            .await;
    }
}

impl AgentHook for ToolLifecycleHook {
    async fn on_completion_call(
        &self,
        _ctx: &HookContext,
        _event: CompletionCallEvent<'_>,
    ) -> CompletionCallAction {
        self.previews.lock().await.abort_all().await;
        CompletionCallAction::continue_run()
    }

    async fn on_model_turn_finished(
        &self,
        _ctx: &HookContext,
        event: ModelTurnFinished<'_>,
    ) -> ModelTurnAction {
        let has_question = event.content.iter().any(|content| {
            matches!(content, AssistantContent::ToolCall(call) if call.function.name.as_str() == "ask_user")
        });
        self.state.lock().await.turn_has_ask_user = has_question;
        if has_question {
            self.previews.lock().await.abort_all().await;
        }
        ModelTurnAction::continue_run()
    }

    async fn on_tool_call_delta(
        &self,
        ctx: &HookContext,
        event: ToolCallDelta<'_>,
    ) -> ObservationAction {
        self.previews
            .lock()
            .await
            .on_tool_delta(ctx.turn(), event.part.index(), event.tool_name, event.delta)
            .await;
        ObservationAction::continue_run()
    }

    async fn on_dispatch(&self, ctx: &HookContext, event: DispatchEvent<'_>) -> DispatchAction {
        let Some(tool_name) = event.tool_name() else {
            return DispatchAction::proceed();
        };
        let has_question = self.state.lock().await.turn_has_ask_user;
        let Some(call_id) = event.call_id else {
            return if has_question && tool_name != "ask_user" {
                DispatchAction::skip("skipped because ask_user ends this turn")
            } else {
                DispatchAction::proceed()
            };
        };
        let tool_call_id = format!("{}:{}", self.execution.run_id, call_id.wire());
        let mut arguments = Self::arguments(event.tool_args());

        if has_question && tool_name != "ask_user" {
            self.previews.lock().await.abort_call(&call_id.wire()).await;
            self.send_started(tool_call_id, tool_name, arguments).await;
            let output = json!({
                "kind": "skipped",
                "reason": "ask_user_sibling",
                "message": "not executed because ask_user ends this turn; replan after the structured answer"
            });
            return DispatchAction::skip(output.to_string());
        }

        let mut action = DispatchAction::proceed();
        if tool_name == "apply_operations" {
            let mut previews = self.previews.lock().await;
            previews
                .finish_tool_call(ctx.turn(), &call_id.wire(), tool_name, &arguments)
                .await;
            if let Some(ids) = previews.take_preview_ids(&call_id.wire()) {
                inject_preview_ids(&mut arguments, &ids);
                action = DispatchAction::rewrite_tool_args(event.kind, arguments.clone());
            }
        }
        self.send_started(tool_call_id, tool_name, arguments).await;
        action
    }

    async fn on_outcome(&self, _ctx: &HookContext, event: OutcomeEvent<'_>) -> OutcomeAction {
        let (Some(tool_name), Some(call_id)) = (event.tool_name(), event.call_id) else {
            return OutcomeAction::proceed();
        };
        let wire_call_id = call_id.wire();
        let tool_call_id = format!("{}:{}", self.execution.run_id, wire_call_id);
        let arguments = Self::arguments(event.tool_args());
        let duration_ms = self
            .state
            .lock()
            .await
            .started
            .remove(&tool_call_id)
            .and_then(|started| u64::try_from(started.elapsed().as_millis()).ok())
            .unwrap_or(0);
        let (mut status, mut output, error) = if let Some(result) = event.tool_result() {
            let status = if result.is_skipped() {
                "skipped"
            } else if result.is_error() || result.is_refused() {
                "failed"
            } else {
                "completed"
            };
            let error = result
                .error()
                .map(ToString::to_string)
                .or_else(|| result.refusal().map(ToString::to_string));
            let output = if result.is_skipped() {
                result
                    .output()
                    .as_text()
                    .and_then(|text| serde_json::from_str(text).ok())
                    .unwrap_or_else(|| tool_output_value(result.output()))
            } else {
                tool_output_value(result.output())
            };
            (status, output, error)
        } else {
            let message = event
                .outcome
                .as_ref()
                .err()
                .map(|report| report.message.clone())
                .unwrap_or_else(|| "tool dispatch failed".into());
            ("failed", json!({"error": message.clone()}), Some(message))
        };

        if tool_name == "apply_operations" {
            let mut previews = self.previews.lock().await;
            if status == "completed" {
                previews.confirm_call(&wire_call_id);
            } else {
                previews.abort_call(&wire_call_id).await;
            }
        }

        let requested = (status == "completed" && tool_name == "ask_user")
            .then(|| {
                event
                    .tool_context()
                    .and_then(|context| context.result::<AskUserRequestedMeta>().ok().flatten())
            })
            .flatten();
        if self.execution.cancellation.is_cancelled() {
            terminate(&self.state, RunTermination::Cancelled).await;
        } else if let Some(requested) = requested {
            self.scope
                .send(ModelStreamEvent::QuestionnaireRequested {
                    tool_call_id: Some(tool_call_id.clone()),
                    questions: requested.questions,
                })
                .await;
            status = "awaiting_input";
            output = json!({"status": "awaiting_input"});
            terminate(&self.state, RunTermination::AskUser).await;
        } else if status == "completed" && tool_name == "create_report" {
            self.bind_created_report(&arguments, &output).await;
        }

        let (output_preview, truncated) = preview_json(&output);
        self.scope
            .send(ModelStreamEvent::ToolCallFinished {
                tool_call_id,
                tool_name: tool_name.to_string(),
                status: status.to_string(),
                output_preview,
                truncated,
                duration_ms,
                input: arguments,
                output,
                error,
            })
            .await;

        if self.state.lock().await.termination == RunTermination::Cancelled {
            OutcomeAction::stop(CANCELLED_REASON)
        } else {
            OutcomeAction::proceed()
        }
    }

    async fn on_invalid_tool_call(
        &self,
        _ctx: &HookContext,
        event: &InvalidToolCallContext,
    ) -> Option<InvalidToolCallAction> {
        match &event.reason {
            InvalidToolCallReason::MalformedArguments { error } => {
                Some(InvalidToolCallAction::retry(format!(
                    "Tool `{}` received malformed arguments: {error}. Return one JSON object matching the advertised schema.",
                    event.tool_name
                )))
            }
            InvalidToolCallReason::UnknownTool | InvalidToolCallReason::DisallowedByToolChoice => {
                Some(InvalidToolCallAction::retry(format!(
                    "Tool `{}` is not available for this turn. Use only these advertised tools: {:?}.",
                    event.tool_name, event.available_tools
                )))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::{
        completion::ResponseIdentity,
        effect::{Answered, EffectId, EffectKind},
        message::{CallId, Message, ToolCall, ToolFunction, ToolName},
        test_utils::HookTestHarness,
        tool::{ToolContext, ToolOutput, ToolResult},
    };
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    use super::super::{SharedDocId, new_shared_doc_id};

    fn execution() -> AgentExecution {
        AgentExecution {
            agent_id: "main".into(),
            model_route: "test/model".into(),
            allowed_tools: vec!["ask_user".into(), "apply_operations".into()],
            system_prompt: "test".into(),
            kb_branch: Some("main".into()),
            doc_id: new_shared_doc_id(None),
            session_id: "sess_test".into(),
            run_id: "run_test".into(),
            cancellation: CancellationToken::new(),
        }
    }

    fn push_tool(content: &mut Vec<AssistantContent>, call_id: &str, tool_name: &str) {
        let Ok(name) = ToolName::new(tool_name) else {
            return;
        };
        content.push(AssistantContent::ToolCall(ToolCall::from_wire(
            call_id,
            ToolFunction::new(name, json!({})),
        )));
    }

    fn question_turn(content: &mut Vec<AssistantContent>) {
        push_tool(content, "call_question", "ask_user");
    }

    #[tokio::test]
    async fn mixed_question_batch_emits_one_skipped_start_and_finish() {
        let (sender, mut receiver) = mpsc::channel(8);
        let execution = execution();
        let scope = StreamEventSender::top(sender);
        let state = SharedTurnState::default();
        let previews = Arc::new(Mutex::new(PreviewState::for_test(
            "run_test",
            SharedDocId::default(),
        )));
        let harness = HookTestHarness::new()
            .with_hook(TurnControlHook::for_test(
                execution.clone(),
                scope.clone(),
                Arc::clone(&state),
            ))
            .with_hook(ToolLifecycleHook::for_test(
                execution,
                scope,
                previews,
                Arc::clone(&state),
            ));
        harness.set_turn(1);

        let mut content = Vec::new();
        question_turn(&mut content);
        assert_eq!(content.len(), 1);
        let identity = ResponseIdentity::default();
        let raw = Value::Null;
        let action = harness
            .hooks()
            .on_model_turn_finished(
                harness.context(),
                ModelTurnFinished {
                    turn: 1,
                    content: &content,
                    usage: Usage::default(),
                    identity: &identity,
                    finish_reason: None,
                    max_tokens: None,
                    raw: &raw,
                },
            )
            .await;
        assert_eq!(action, ModelTurnAction::Continue);

        let kind = EffectKind::ToolCall {
            name: "apply_operations".into(),
            args: "{}".into(),
        };
        let call_id = CallId::from_wire("call_sibling");
        let action = harness
            .hooks()
            .on_dispatch(
                harness.context(),
                DispatchEvent {
                    id: EffectId::from_raw(1),
                    kind: &kind,
                    turn: 1,
                    call_id: Some(&call_id),
                    context: None,
                },
            )
            .await;
        assert!(matches!(action, DispatchAction::Deny(_)));

        let outcome = Ok(Answered::ToolResult {
            result: ToolResult::skipped("ask_user_sibling"),
        });
        let action = harness
            .hooks()
            .on_outcome(
                harness.context(),
                OutcomeEvent {
                    id: EffectId::from_raw(1),
                    kind: &kind,
                    outcome: &outcome,
                    turn: 1,
                    call_id: Some(&call_id),
                    context: None,
                },
            )
            .await;
        assert!(matches!(action, OutcomeAction::Proceed));

        let events: Vec<ModelStreamEvent> = std::iter::from_fn(|| receiver.try_recv().ok())
            .map(|result| result.ok())
            .take_while(Option::is_some)
            .flatten()
            .collect();
        assert_eq!(events.len(), 2, "unexpected events: {events:?}");
        assert!(matches!(
            events.first(),
            Some(ModelStreamEvent::ToolCallStarted { .. })
        ));
        assert!(matches!(
            events.get(1),
            Some(ModelStreamEvent::ToolCallFinished { status, .. }) if status == "skipped"
        ));
    }

    #[tokio::test]
    async fn question_batch_stops_at_next_completion_without_losing_tool_results() {
        let (sender, mut receiver) = mpsc::channel(16);
        let execution = execution();
        let scope = StreamEventSender::top(sender);
        let state = SharedTurnState::default();
        let previews = Arc::new(Mutex::new(PreviewState::for_test(
            "run_test",
            SharedDocId::default(),
        )));
        let harness = HookTestHarness::new()
            .with_hook(TurnControlHook::for_test(
                execution.clone(),
                scope.clone(),
                Arc::clone(&state),
            ))
            .with_hook(ToolLifecycleHook::for_test(
                execution,
                scope,
                previews,
                Arc::clone(&state),
            ));
        harness.set_turn(1);
        let mut content = Vec::new();
        question_turn(&mut content);
        push_tool(&mut content, "call_sibling", "apply_operations");
        let identity = ResponseIdentity::default();
        let raw = Value::Null;
        let _ = harness
            .hooks()
            .on_model_turn_finished(
                harness.context(),
                ModelTurnFinished {
                    turn: 1,
                    content: &content,
                    usage: Usage::default(),
                    identity: &identity,
                    finish_reason: None,
                    max_tokens: None,
                    raw: &raw,
                },
            )
            .await;

        let question_kind = EffectKind::ToolCall {
            name: "ask_user".into(),
            args: "{}".into(),
        };
        let question_id = CallId::from_wire("call_question");
        let _ = harness
            .hooks()
            .on_dispatch(
                harness.context(),
                DispatchEvent {
                    id: EffectId::from_raw(1),
                    kind: &question_kind,
                    turn: 1,
                    call_id: Some(&question_id),
                    context: None,
                },
            )
            .await;
        let mut tool_context = ToolContext::new();
        let _ = tool_context.insert_result(AskUserRequestedMeta {
            questions: Vec::new(),
        });
        let question_outcome = Ok(Answered::ToolResult {
            result: ToolResult::success(ToolOutput::json(json!({
                "status": "awaiting_input"
            }))),
        });
        let action = harness
            .hooks()
            .on_outcome(
                harness.context(),
                OutcomeEvent {
                    id: EffectId::from_raw(1),
                    kind: &question_kind,
                    outcome: &question_outcome,
                    turn: 1,
                    call_id: Some(&question_id),
                    context: Some(&tool_context),
                },
            )
            .await;
        assert!(matches!(action, OutcomeAction::Proceed));

        let sibling_kind = EffectKind::ToolCall {
            name: "apply_operations".into(),
            args: "{}".into(),
        };
        let sibling_id = CallId::from_wire("call_sibling");
        let action = harness
            .hooks()
            .on_dispatch(
                harness.context(),
                DispatchEvent {
                    id: EffectId::from_raw(2),
                    kind: &sibling_kind,
                    turn: 1,
                    call_id: Some(&sibling_id),
                    context: None,
                },
            )
            .await;
        assert!(matches!(action, DispatchAction::Deny(_)));
        let sibling_outcome = Ok(Answered::ToolResult {
            result: ToolResult::skipped("ask_user_sibling"),
        });
        let action = harness
            .hooks()
            .on_outcome(
                harness.context(),
                OutcomeEvent {
                    id: EffectId::from_raw(2),
                    kind: &sibling_kind,
                    outcome: &sibling_outcome,
                    turn: 1,
                    call_id: Some(&sibling_id),
                    context: None,
                },
            )
            .await;
        assert!(matches!(action, OutcomeAction::Proceed));

        harness.set_turn(2);
        let prompt = Message::user("committed tool results");
        let history: Vec<Arc<Message>> = Vec::new();
        let action = harness
            .hooks()
            .on_completion_call(
                harness.context(),
                CompletionCallEvent {
                    prompt: &prompt,
                    history: &history,
                    turn: 2,
                },
            )
            .await;
        assert!(matches!(
            action,
            CompletionCallAction::Stop(reason) if reason == ASK_USER_REASON
        ));

        let mut events = Vec::new();
        while let Ok(Ok(event)) = receiver.try_recv() {
            events.push(event);
        }
        assert_eq!(events.len(), 5, "unexpected events: {events:?}");
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, ModelStreamEvent::ToolCallStarted { .. }))
                .count(),
            2
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, ModelStreamEvent::ToolCallFinished { .. }))
                .count(),
            2
        );
        assert!(events.iter().any(|event| matches!(
            event,
            ModelStreamEvent::ToolCallFinished { status, .. } if status == "awaiting_input"
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            ModelStreamEvent::ToolCallFinished { status, .. } if status == "skipped"
        )));
    }
}
