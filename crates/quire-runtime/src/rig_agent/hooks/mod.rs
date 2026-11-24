//! Run control and tool lifecycle hooks for the native Rig runner.

use std::{collections::HashMap, sync::Arc, time::Instant};

use rig::{
    agent::{
        AgentHook, CompletionCallAction, CompletionCallEvent, DispatchAction, DispatchEvent,
        HookContext, InvalidToolCallAction, InvalidToolCallContext, InvalidToolCallReason,
        ModelTurnAction, ModelTurnFinished, ObservationAction, OutcomeAction, OutcomeEvent,
        ReasoningDelta, RequestPatch, RunStart, RunStartAction, TextDelta, ToolCallDelta,
    },
    completion::Document,
    message::AssistantContent,
};
use serde::Serialize;
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use tokio::sync::Mutex;

use crate::{
    kb::KbService, memory::ConversationMemory, model_stream::ModelStreamEvent,
    service::PendingQueueCoordinator,
};

use super::{
    AgentExecution, SharedForkHistory, StreamEventSender,
    context::AskUserRequestedMeta,
    preview_json,
    preview_stream::{PreviewState, inject_preview_ids},
    tool_output_value, update_doc_id, update_fork_history,
};

pub(super) const CANCELLED_REASON: &str = "quire:cancelled";
pub(super) const SUPERSEDED_REASON: &str = "quire:superseded";
pub(super) const ASK_USER_REASON: &str = "quire:ask_user";

#[derive(Serialize)]
struct SkippedToolOutput {
    kind: &'static str,
    reason: &'static str,
    message: &'static str,
}

#[derive(Serialize)]
struct ToolErrorOutput<'a> {
    error: &'a str,
}

#[derive(Serialize)]
struct AwaitingInputOutput {
    status: &'static str,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum RunTermination {
    #[default]
    None,
    Cancelled,
    Superseded,
    AskUser,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct ModelTurnPricing {
    pub(super) context_limit: u64,
    pub(super) cost_input: Option<f64>,
    pub(super) cost_output: Option<f64>,
    pub(super) cost_cache_read: Option<f64>,
}

impl ModelTurnPricing {
    pub(super) fn calculate_cost_rmb(
        &self,
        input_tokens: u64,
        output_tokens: u64,
        cached_tokens: u64,
    ) -> Option<f64> {
        if self.cost_input.is_none() && self.cost_output.is_none() && self.cost_cache_read.is_none()
        {
            return None;
        }
        let uncached_input = input_tokens.saturating_sub(cached_tokens);
        let input_rate = self.cost_input.unwrap_or(0.0);
        let output_rate = self.cost_output.unwrap_or(0.0);
        let cache_rate = self.cost_cache_read.or(self.cost_input).unwrap_or(0.0);
        Some(
            ((uncached_input as f64) * input_rate
                + (cached_tokens as f64) * cache_rate
                + (output_tokens as f64) * output_rate)
                / 1_000_000.0,
        )
    }
}

#[derive(Default)]
pub(super) struct TurnRunState {
    pub(super) termination: RunTermination,
    turn_has_ask_user: bool,
    started: HashMap<String, Instant>,
}

pub(super) type SharedTurnState = Arc<Mutex<TurnRunState>>;

async fn terminate(state: &SharedTurnState, termination: RunTermination) {
    state.lock().await.termination = termination;
}

#[derive(Clone)]
pub(super) struct StepMemoryCheckpoint {
    pub(super) memory: ConversationMemory,
    pub(super) log_key: String,
    pub(super) committed_history: Arc<Mutex<Vec<rig::message::Message>>>,
}

/// Owns model-call control, cancellation, queue supersession, usage and streamed text.
pub(super) struct TurnControlHook {
    execution: AgentExecution,
    scope: StreamEventSender,
    queue: Option<PendingQueueCoordinator>,
    kb_update_notice: Option<String>,
    pricing: Option<ModelTurnPricing>,
    state: SharedTurnState,
    parent_history: SharedForkHistory,
    memory: Option<StepMemoryCheckpoint>,
}

impl TurnControlHook {
    #[allow(
        clippy::too_many_arguments,
        reason = "hook carries explicit turn control, pricing, fork-history and incremental memory handles"
    )]
    pub(super) fn new(
        execution: AgentExecution,
        scope: StreamEventSender,
        queue: PendingQueueCoordinator,
        kb_update_notice: Option<String>,
        pricing: ModelTurnPricing,
        state: SharedTurnState,
        parent_history: SharedForkHistory,
        memory: Option<StepMemoryCheckpoint>,
    ) -> Self {
        Self {
            execution,
            scope,
            queue: Some(queue),
            kb_update_notice,
            pricing: Some(pricing),
            state,
            parent_history,
            memory,
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
            pricing: None,
            state,
            parent_history: super::new_shared_fork_history(),
            memory: None,
        }
    }

    pub(super) fn session_documents(&self) -> Vec<Document> {
        let mut documents = Vec::new();
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
        event: CompletionCallEvent<'_>,
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
        if !self.execution.run_id.starts_with("run_sub_") {
            let mut snapshot: Vec<rig::message::Message> = event
                .history
                .iter()
                .map(|message| message.as_ref().clone())
                .collect();
            snapshot.push(event.prompt.clone());
            update_fork_history(&self.parent_history, snapshot.clone());
            if ctx.turn() > 1
                && let Some(checkpoint) = self.memory.as_ref()
            {
                let mut committed = checkpoint.committed_history.lock().await;
                if snapshot.len() > committed.len() && snapshot.starts_with(&committed) {
                    match checkpoint
                        .memory
                        .commit(&checkpoint.log_key, &committed, snapshot.clone(), None)
                        .await
                    {
                        Ok(()) => *committed = snapshot,
                        Err(error) => {
                            tracing::warn!(%error, run_id = %self.execution.run_id, "failed to persist step to conversation memory");
                        }
                    }
                }
            }
        }
        let documents = self.session_documents();
        if documents.is_empty() {
            CompletionCallAction::continue_run()
        } else {
            CompletionCallAction::patch(RequestPatch::new().extra_context(documents))
        }
    }

    async fn on_model_turn_finished(
        &self,
        _ctx: &HookContext,
        event: ModelTurnFinished<'_>,
    ) -> ModelTurnAction {
        if self.execution.cancellation.is_cancelled() {
            terminate(&self.state, RunTermination::Cancelled).await;
            return ModelTurnAction::stop(CANCELLED_REASON);
        }
        if let Some(pricing) = self.pricing {
            let mut usage = super::context_usage(&event.usage, pricing.context_limit);
            usage.cost_rmb = pricing.calculate_cost_rmb(
                usage.input_tokens,
                usage.output_tokens,
                usage.cached_tokens,
            );
            usage.output_price_rmb = pricing.cost_output;
            if usage.input_tokens > 0 || usage.output_tokens > 0 || usage.cached_tokens > 0 {
                self.scope.send(ModelStreamEvent::Usage(usage)).await;
            }
        }
        ModelTurnAction::continue_run()
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

mod lifecycle;
#[cfg(test)]
mod tests;
