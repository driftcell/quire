//! Pluggable model stream used by the runtime turn loop and offline tests.

use std::{collections::VecDeque, pin::Pin, sync::Arc};

use async_trait::async_trait;
use futures::Stream;
use tokio::sync::Mutex;

use crate::error::{RuntimeError, RuntimeResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelStreamEvent {
    ThinkingDelta(String),
    TextDelta(String),
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
}

#[derive(Debug, Clone)]
pub struct ModelTurnRequest {
    pub session_id: String,
    pub run_id: String,
    pub agent_id: String,
    pub system_prompt: String,
    pub user_message: String,
}

pub type ModelEventStream =
    Pin<Box<dyn Stream<Item = RuntimeResult<ModelStreamEvent>> + Send + 'static>>;

#[async_trait]
pub trait ModelStreamProvider: Send + Sync {
    async fn stream_turn(&self, request: ModelTurnRequest) -> RuntimeResult<ModelEventStream>;
}

/// Deterministic local provider used when no external model is configured.
#[derive(Debug, Default)]
pub struct StubStreamProvider;

#[async_trait]
impl ModelStreamProvider for StubStreamProvider {
    async fn stream_turn(&self, request: ModelTurnRequest) -> RuntimeResult<ModelEventStream> {
        let response = format!(
            "Runtime accepted the request for `{}`. Connect a rig model provider to stream the full answer.",
            request.session_id
        );
        Ok(Box::pin(futures::stream::iter(vec![
            Ok(ModelStreamEvent::ThinkingDelta(
                "Preparing the next report action.".into(),
            )),
            Ok(ModelStreamEvent::TextDelta(response)),
        ])))
    }
}

/// Scriptable provider for offline coalescing/tool-order integration tests.
pub struct MockStreamProvider {
    turns: Arc<Mutex<VecDeque<Vec<ModelStreamEvent>>>>,
}

impl MockStreamProvider {
    pub fn new(turns: Vec<Vec<ModelStreamEvent>>) -> Self {
        Self {
            turns: Arc::new(Mutex::new(VecDeque::from(turns))),
        }
    }
}

#[async_trait]
impl ModelStreamProvider for MockStreamProvider {
    async fn stream_turn(&self, request: ModelTurnRequest) -> RuntimeResult<ModelEventStream> {
        let events = {
            let mut turns = self.turns.lock().await;
            turns.pop_front()
        }
        .unwrap_or_else(|| {
            vec![ModelStreamEvent::TextDelta(format!(
                "mock reply for {}",
                request.session_id
            ))]
        });
        Ok(Box::pin(futures::stream::iter(
            events.into_iter().map(Ok).collect::<Vec<_>>(),
        )))
    }
}

/// Fail fast if a provider is asked to stream without a model route.
pub fn missing_provider_error(route: &str) -> RuntimeError {
    RuntimeError::InvalidRequest(format!("no model stream provider configured for `{route}`"))
}
