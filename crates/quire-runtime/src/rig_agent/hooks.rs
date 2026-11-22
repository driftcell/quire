//! Responsibility-focused AgentHook implementations for the native Rig runner.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use rig::{
    agent::{
        AgentHook, CompletionCallAction, CompletionCallEvent, DispatchAction, DispatchEvent,
        HookContext, InvalidToolCallAction, InvalidToolCallContext, InvalidToolCallReason,
        ModelTurnAction, ModelTurnFinished, ObservationAction, OutcomeAction, OutcomeEvent,
        ReasoningDelta, RequestPatch, TextDelta, ToolCallDelta,
    },
    completion::{Document, Usage},
    message::AssistantContent,
};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::{kb::KbService, model_stream::ModelStreamEvent, service::PendingQueueCoordinator};

use super::{
    AgentExecution, EventScope,
    context::{AskUserRequestedMeta, CreatedReportMeta},
    preview_json,
    preview_stream::{PreviewState, inject_preview_ids},
    runtime_preamble, tool_output_value,
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

pub(super) type Termination = Arc<Mutex<RunTermination>>;

async fn terminate(termination: &Termination, reason: RunTermination) {
    *termination.lock().await = reason;
}

/// Streams accepted provisional text/reasoning to Quire's ordered event pipeline.
pub(super) struct StreamEventHook {
    execution: AgentExecution,
    scope: EventScope,
    termination: Termination,
}

impl StreamEventHook {
    pub(super) fn new(
        execution: AgentExecution,
        scope: EventScope,
        termination: Termination,
    ) -> Self {
        Self {
            execution,
            scope,
            termination,
        }
    }
}

impl AgentHook for StreamEventHook {
    async fn on_text_delta(&self, _ctx: &HookContext, event: TextDelta<'_>) -> ObservationAction {
        if self.execution.cancellation.is_cancelled() {
            terminate(&self.termination, RunTermination::Cancelled).await;
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
            terminate(&self.termination, RunTermination::Cancelled).await;
            return ObservationAction::stop(CANCELLED_REASON);
        }
        self.scope
            .send(ModelStreamEvent::ThinkingDelta(event.delta.to_string()))
            .await;
        ObservationAction::continue_run()
    }
}

/// Owns ask_user's terminal semantics and position-independent sibling skipping.
pub(super) struct QuestionHook {
    execution: AgentExecution,
    scope: EventScope,
    batch_has_question: Arc<AtomicBool>,
    termination: Termination,
}

impl QuestionHook {
    pub(super) fn new(
        execution: AgentExecution,
        scope: EventScope,
        batch_has_question: Arc<AtomicBool>,
        termination: Termination,
    ) -> Self {
        Self {
            execution,
            scope,
            batch_has_question,
            termination,
        }
    }

    fn call_id(&self, event: &DispatchEvent<'_>) -> Option<String> {
        event
            .call_id
            .map(|id| format!("{}:{}", self.execution.run_id, id.wire()))
    }

    fn arguments(event: &DispatchEvent<'_>) -> Value {
        event
            .tool_args()
            .and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or(Value::Null)
    }
}

impl AgentHook for QuestionHook {
    async fn on_model_turn_finished(
        &self,
        _ctx: &HookContext,
        event: ModelTurnFinished<'_>,
    ) -> ModelTurnAction {
        let has_question = event.content.iter().any(|content| {
            matches!(content, AssistantContent::ToolCall(call) if call.function.name.as_str() == "ask_user")
        });
        self.batch_has_question
            .store(has_question, Ordering::Release);
        ModelTurnAction::continue_run()
    }

    async fn on_dispatch(&self, _ctx: &HookContext, event: DispatchEvent<'_>) -> DispatchAction {
        if !self.batch_has_question.load(Ordering::Acquire) {
            return DispatchAction::proceed();
        }
        let Some(tool_name) = event.tool_name() else {
            return DispatchAction::proceed();
        };
        if tool_name == "ask_user" {
            if let (Some(tool_call_id), arguments) = (self.call_id(&event), Self::arguments(&event))
            {
                let (input_preview, truncated) = preview_json(&arguments);
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
            return DispatchAction::proceed();
        }

        let Some(tool_call_id) = self.call_id(&event) else {
            return DispatchAction::skip("skipped because ask_user ends this turn");
        };
        let arguments = Self::arguments(&event);
        let (input_preview, truncated) = preview_json(&arguments);
        self.scope
            .send(ModelStreamEvent::ToolCallStarted {
                tool_call_id: tool_call_id.clone(),
                tool_name: tool_name.to_string(),
                input_preview,
                truncated,
                input: arguments.clone(),
            })
            .await;
        let output = json!({
            "kind": "skipped",
            "reason": "ask_user_sibling",
            "message": "not executed because ask_user ends this turn; replan after the structured answer"
        });
        let (output_preview, output_truncated) = preview_json(&output);
        self.scope
            .send(ModelStreamEvent::ToolCallFinished {
                tool_call_id,
                tool_name: tool_name.to_string(),
                status: "skipped".into(),
                output_preview,
                truncated: output_truncated,
                duration_ms: 0,
                input: arguments,
                output: output.clone(),
                error: None,
            })
            .await;
        DispatchAction::skip(output.to_string())
    }

    async fn on_outcome(&self, _ctx: &HookContext, event: OutcomeEvent<'_>) -> OutcomeAction {
        if event.tool_name() != Some("ask_user") {
            return OutcomeAction::proceed();
        }
        let Some(context) = event.tool_context() else {
            return OutcomeAction::proceed();
        };
        let requested = context.result::<AskUserRequestedMeta>().ok().flatten();
        let Some(requested) = requested else {
            return OutcomeAction::proceed();
        };
        let Some(call_id) = event.call_id else {
            return OutcomeAction::stop("ask_user did not carry a tool call id");
        };
        let tool_call_id = format!("{}:{}", self.execution.run_id, call_id.wire());
        let arguments = event
            .tool_args()
            .and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or(Value::Null);
        self.scope
            .send(ModelStreamEvent::QuestionnaireRequested {
                tool_call_id: Some(tool_call_id.clone()),
                questions: requested.questions,
            })
            .await;
        let output = json!({"status": "awaiting_input"});
        let (output_preview, truncated) = preview_json(&output);
        self.scope
            .send(ModelStreamEvent::ToolCallFinished {
                tool_call_id,
                tool_name: "ask_user".into(),
                status: "awaiting_input".into(),
                output_preview,
                truncated,
                duration_ms: 0,
                input: arguments,
                output,
                error: None,
            })
            .await;
        terminate(&self.termination, RunTermination::AskUser).await;
        OutcomeAction::stop(ASK_USER_REASON)
    }
}

/// Controls cancellation, safe-boundary supersession and per-turn request context.
pub(super) struct RunControlHook {
    execution: AgentExecution,
    queue: PendingQueueCoordinator,
    doc_id: Arc<Mutex<Option<String>>>,
    extra_context: Vec<Document>,
    termination: Termination,
}

impl RunControlHook {
    pub(super) fn new(
        execution: AgentExecution,
        queue: PendingQueueCoordinator,
        doc_id: Arc<Mutex<Option<String>>>,
        extra_context: Vec<Document>,
        termination: Termination,
    ) -> Self {
        Self {
            execution,
            queue,
            doc_id,
            extra_context,
            termination,
        }
    }
}

impl AgentHook for RunControlHook {
    async fn on_completion_call(
        &self,
        ctx: &HookContext,
        _event: CompletionCallEvent<'_>,
    ) -> CompletionCallAction {
        if self.execution.cancellation.is_cancelled() {
            terminate(&self.termination, RunTermination::Cancelled).await;
            return CompletionCallAction::stop(CANCELLED_REASON);
        }
        if ctx.turn() > 1
            && !self.execution.run_id.starts_with("run_sub_")
            && self
                .queue
                .has_pending(&self.execution.session_id)
                .await
                .unwrap_or(false)
        {
            terminate(&self.termination, RunTermination::Superseded).await;
            return CompletionCallAction::stop(SUPERSEDED_REASON);
        }
        let mut execution = self.execution.clone();
        execution.doc_id = self.doc_id.lock().await.clone();
        CompletionCallAction::patch(
            RequestPatch::new()
                .preamble(runtime_preamble(&execution))
                .extra_context(self.extra_context.clone()),
        )
    }

    async fn on_dispatch(&self, _ctx: &HookContext, _event: DispatchEvent<'_>) -> DispatchAction {
        if self.execution.cancellation.is_cancelled() {
            terminate(&self.termination, RunTermination::Cancelled).await;
            DispatchAction::stop(CANCELLED_REASON)
        } else {
            DispatchAction::proceed()
        }
    }

    async fn on_tool_call_delta(
        &self,
        _ctx: &HookContext,
        _event: ToolCallDelta<'_>,
    ) -> ObservationAction {
        if self.execution.cancellation.is_cancelled() {
            terminate(&self.termination, RunTermination::Cancelled).await;
            ObservationAction::stop(CANCELLED_REASON)
        } else {
            ObservationAction::continue_run()
        }
    }
}

/// Converts streamed apply_operations arguments to previews and rewrites final arguments.
pub(super) struct PreviewHook {
    previews: Arc<Mutex<PreviewState>>,
}

impl PreviewHook {
    pub(super) fn new(previews: Arc<Mutex<PreviewState>>) -> Self {
        Self { previews }
    }
}

impl AgentHook for PreviewHook {
    async fn on_completion_call(
        &self,
        _ctx: &HookContext,
        _event: CompletionCallEvent<'_>,
    ) -> CompletionCallAction {
        self.previews.lock().await.clear_streams();
        CompletionCallAction::continue_run()
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
        if event.tool_name() != Some("apply_operations") {
            return DispatchAction::proceed();
        }
        let Some(call_id) = event.call_id else {
            return DispatchAction::proceed();
        };
        let Some(mut arguments) = event
            .tool_args()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        else {
            return DispatchAction::proceed();
        };
        let mut previews = self.previews.lock().await;
        previews
            .finish_tool_call(ctx.turn(), &call_id.wire(), "apply_operations", &arguments)
            .await;
        if let Some(ids) = previews.take_preview_ids(&call_id.wire()) {
            inject_preview_ids(&mut arguments, &ids);
            return DispatchAction::rewrite_tool_args(event.kind, arguments);
        }
        DispatchAction::proceed()
    }
}

/// Records normal tool execution. ask_user and skipped siblings are owned by QuestionHook.
pub(super) struct ToolAuditHook {
    execution: AgentExecution,
    scope: EventScope,
    kb: KbService,
    doc_id: Arc<Mutex<Option<String>>>,
    started: Arc<Mutex<HashMap<String, Instant>>>,
}

impl ToolAuditHook {
    pub(super) fn new(
        execution: AgentExecution,
        scope: EventScope,
        kb: KbService,
        doc_id: Arc<Mutex<Option<String>>>,
    ) -> Self {
        Self {
            execution,
            scope,
            kb,
            doc_id,
            started: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn tool_call_id(&self, event: &OutcomeEvent<'_>) -> Option<String> {
        event
            .call_id
            .map(|id| format!("{}:{}", self.execution.run_id, id.wire()))
    }
}

impl AgentHook for ToolAuditHook {
    async fn on_dispatch(&self, _ctx: &HookContext, event: DispatchEvent<'_>) -> DispatchAction {
        if event.tool_name() == Some("ask_user") {
            return DispatchAction::proceed();
        }
        let (Some(tool_name), Some(call_id), Some(raw_arguments)) =
            (event.tool_name(), event.call_id, event.tool_args())
        else {
            return DispatchAction::proceed();
        };
        let arguments = serde_json::from_str(raw_arguments).unwrap_or(Value::Null);
        let tool_call_id = format!("{}:{}", self.execution.run_id, call_id.wire());
        let (input_preview, truncated) = preview_json(&arguments);
        self.started
            .lock()
            .await
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
        DispatchAction::proceed()
    }

    async fn on_outcome(&self, _ctx: &HookContext, event: OutcomeEvent<'_>) -> OutcomeAction {
        let (Some(tool_name), Some(tool_call_id)) = (event.tool_name(), self.tool_call_id(&event))
        else {
            return OutcomeAction::proceed();
        };
        let arguments = event
            .tool_args()
            .and_then(|raw| serde_json::from_str(raw).ok())
            .unwrap_or(Value::Null);
        let duration_ms = self
            .started
            .lock()
            .await
            .remove(&tool_call_id)
            .and_then(|started| u64::try_from(started.elapsed().as_millis()).ok())
            .unwrap_or(0);
        let (status, output, error) = if let Some(result) = event.tool_result() {
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
            (status, tool_output_value(result.output()), error)
        } else {
            let message = event
                .outcome
                .as_ref()
                .err()
                .map(|report| report.message.clone())
                .unwrap_or_else(|| "tool dispatch failed".into());
            ("failed", json!({"error": message.clone()}), Some(message))
        };

        if status == "completed" && tool_name == "create_report" {
            let created = event
                .tool_context()
                .and_then(|context| context.result::<CreatedReportMeta>().ok().flatten())
                .or_else(|| {
                    let doc_id = output.get("doc_id")?.as_str()?.to_string();
                    let title = output
                        .get("title")
                        .and_then(Value::as_str)
                        .or_else(|| arguments.get("title").and_then(Value::as_str))
                        .unwrap_or("未命名报告")
                        .to_string();
                    Some(CreatedReportMeta { doc_id, title })
                });
            if let Some(created) = created {
                match self
                    .kb
                    .store()
                    .bind_doc(&self.execution.session_id, &created.doc_id)
                    .await
                {
                    Ok(()) => {
                        *self.doc_id.lock().await = Some(created.doc_id.clone());
                        self.scope
                            .send(ModelStreamEvent::DocLinked {
                                doc_id: created.doc_id,
                                title: created.title,
                            })
                            .await;
                    }
                    Err(error) => {
                        tracing::warn!(%error, session_id = %self.execution.session_id, "failed to bind lazily created report");
                    }
                }
            }
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
        OutcomeAction::proceed()
    }
}

/// Classifies malformed arguments separately from unknown/disallowed tools.
pub(super) struct InvalidToolCallHook;

impl AgentHook for InvalidToolCallHook {
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

/// Keeps the last accepted model call's context occupancy, not aggregate run usage.
pub(super) struct UsageHook {
    last_usage: Arc<Mutex<Option<Usage>>>,
}

impl UsageHook {
    pub(super) fn new(last_usage: Arc<Mutex<Option<Usage>>>) -> Self {
        Self { last_usage }
    }
}

impl AgentHook for UsageHook {
    async fn on_model_turn_finished(
        &self,
        _ctx: &HookContext,
        event: ModelTurnFinished<'_>,
    ) -> ModelTurnAction {
        *self.last_usage.lock().await = Some(event.usage);
        ModelTurnAction::continue_run()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::{
        effect::{EffectId, EffectKind},
        message::CallId,
        test_utils::HookTestHarness,
    };
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    fn execution() -> AgentExecution {
        AgentExecution {
            agent_id: "main".into(),
            model_route: "test/model".into(),
            allowed_tools: vec!["ask_user".into(), "apply_operations".into()],
            disable_tools: false,
            system_prompt: "test".into(),
            kb_branch: Some("main".into()),
            doc_id: None,
            session_id: "sess_test".into(),
            run_id: "run_test".into(),
            cancellation: CancellationToken::new(),
        }
    }

    #[tokio::test]
    async fn question_hook_skips_siblings_before_preview_or_execution() {
        let (sender, mut receiver) = mpsc::channel(8);
        let execution = execution();
        let scope = EventScope::top(sender, execution.cancellation.clone());
        let has_question = Arc::new(AtomicBool::new(true));
        let termination = Arc::new(Mutex::new(RunTermination::None));
        let harness = HookTestHarness::new().with_hook(QuestionHook::new(
            execution,
            scope,
            has_question,
            termination,
        ));
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

        let mut events = Vec::new();
        while let Ok(Ok(event)) = receiver.try_recv() {
            events.push(event);
        }
        assert!(matches!(
            events.first(),
            Some(ModelStreamEvent::ToolCallStarted { .. })
        ));
        assert!(matches!(
            events.get(1),
            Some(ModelStreamEvent::ToolCallFinished { status, .. }) if status == "skipped"
        ));
        assert_eq!(events.len(), 2);
    }
}
