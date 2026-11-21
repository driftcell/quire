//! Pluggable model stream used by the runtime turn loop and offline tests.

use std::{collections::VecDeque, future::Future, pin::Pin, sync::Arc};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::Stream;
use quire_session_protocol::{ContextUsage, QuestionItem};
use serde_json::Value;
use tokio::{sync::Mutex, sync::mpsc, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::{
    error::{RuntimeError, RuntimeResult},
    prompt::ContextMessage,
};

#[derive(Debug, Clone, PartialEq)]
pub enum ModelStreamEvent {
    ThinkingDelta(String),
    TextDelta(String),
    TaskFocus {
        title: String,
        detail: Option<String>,
        section_id: Option<String>,
    },
    DocLinked {
        doc_id: String,
        title: String,
    },
    SessionTitleUpdated {
        title: String,
    },
    SubagentStarted {
        subagent_run_id: String,
        agent_name: String,
        display_name: Option<String>,
        task: String,
        section_id: Option<String>,
    },
    SubagentFinished {
        subagent_run_id: String,
        status: String,
        summary: String,
    },
    /// An event produced inside a subagent. The service attributes it to the subagent while
    /// keeping the parent run as its envelope.
    SubagentEvent {
        subagent_run_id: String,
        agent_name: String,
        event: Box<ModelStreamEvent>,
    },
    ToolCallStarted {
        tool_call_id: String,
        tool_name: String,
        input_preview: String,
        truncated: bool,
        input: Value,
    },
    ToolCallFinished {
        tool_call_id: String,
        tool_name: String,
        status: String,
        output_preview: String,
        truncated: bool,
        duration_ms: u64,
        input: Value,
        output: Value,
        error: Option<String>,
    },
    /// A queued user message consumed between tool results and the next model round.
    UserMessageInjected {
        message_id: String,
        content: String,
        created_at: DateTime<Utc>,
    },
    QuestionnaireRequested {
        tool_call_id: Option<String>,
        questions: Vec<QuestionItem>,
    },
    /// Real upstream token counts plus the resolved model context limit.
    Usage(ContextUsage),
}

#[derive(Debug, Clone)]
pub struct ModelTurnRequest {
    pub session_id: String,
    pub run_id: String,
    pub agent_id: String,
    pub system_prompt: String,
    pub user_message: String,
    /// Full model context assembled from session history and resume reminders.
    pub history: Vec<ContextMessage>,
    /// Bound Quire report, injected into the preamble to avoid duplicate create_report calls.
    pub doc_id: Option<String>,
    /// Session KB branch used for agent/skill lookup and branch-aware tools.
    pub kb_branch: Option<String>,
    /// Resolved model route for the primary agent.
    pub model_route: String,
    /// Tool allow-list from the primary agent definition.
    pub allowed_tools: Vec<String>,
    /// Run a single completion round with no tools (used by context compaction).
    pub disable_tools: bool,
    /// Cancels model streaming, tool execution and subagent inference.
    pub cancellation: CancellationToken,
}

pub type ModelEventStream =
    Pin<Box<dyn Stream<Item = RuntimeResult<ModelStreamEvent>> + Send + 'static>>;

#[async_trait]
pub trait ModelStreamProvider: Send + Sync {
    async fn stream_turn(&self, request: ModelTurnRequest) -> RuntimeResult<ModelEventStream>;
}

/// Aborts the producer task when the consumer drops the stream.
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Spawn a producer task and expose its events as a stream bound to the consumer's lifetime.
///
/// Dropping the stream aborts the producer. A consumer that fails midway (for example on a
/// persistence error) therefore cannot leave a provider executing model turns or tools in the
/// background after the run has already been marked as failed.
/// Bounded producer queue. Slow persistence applies backpressure instead of
/// accumulating an unbounded number of deltas and full tool payloads in memory.
const PRODUCER_CHANNEL_CAPACITY: usize = 256;

pub(crate) fn spawn_producer_stream<F, Fut>(
    cancellation: CancellationToken,
    producer: F,
) -> ModelEventStream
where
    F: FnOnce(mpsc::Sender<RuntimeResult<ModelStreamEvent>>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let (sender, mut receiver) = mpsc::channel(PRODUCER_CHANNEL_CAPACITY);
    let task = AbortOnDrop(tokio::spawn(producer(sender)));
    let stream = async_stream::stream! {
        let _task = task;
        loop {
            tokio::select! {
                _ = cancellation.cancelled() => break,
                item = receiver.recv() => {
                    match item {
                        Some(item) => yield item,
                        None => break,
                    }
                }
            }
        }
    };
    Box::pin(stream)
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

/// Every provider event variant, used by projection guardrail tests.
#[cfg(test)]
pub(crate) fn sample_model_stream_events() -> Vec<ModelStreamEvent> {
    vec![
        ModelStreamEvent::ThinkingDelta("thinking".into()),
        ModelStreamEvent::TextDelta("text".into()),
        ModelStreamEvent::TaskFocus {
            title: "title".into(),
            detail: None,
            section_id: None,
        },
        ModelStreamEvent::DocLinked {
            doc_id: "doc_1".into(),
            title: "title".into(),
        },
        ModelStreamEvent::SessionTitleUpdated {
            title: "title".into(),
        },
        ModelStreamEvent::SubagentStarted {
            subagent_run_id: "run_sub_1".into(),
            agent_name: "alpha".into(),
            display_name: None,
            task: "task".into(),
            section_id: None,
        },
        ModelStreamEvent::SubagentFinished {
            subagent_run_id: "run_sub_1".into(),
            status: "completed".into(),
            summary: "done".into(),
        },
        ModelStreamEvent::SubagentEvent {
            subagent_run_id: "run_sub_1".into(),
            agent_name: "alpha".into(),
            event: Box::new(ModelStreamEvent::TextDelta("child".into())),
        },
        ModelStreamEvent::ToolCallStarted {
            tool_call_id: "tc_1".into(),
            tool_name: "tool".into(),
            input_preview: "{}".into(),
            truncated: false,
            input: serde_json::json!({}),
        },
        ModelStreamEvent::ToolCallFinished {
            tool_call_id: "tc_1".into(),
            tool_name: "tool".into(),
            status: "completed".into(),
            output_preview: "ok".into(),
            truncated: false,
            duration_ms: 1,
            input: serde_json::json!({}),
            output: serde_json::json!({}),
            error: None,
        },
        ModelStreamEvent::UserMessageInjected {
            message_id: "msg_1".into(),
            content: "content".into(),
            created_at: chrono::Utc::now(),
        },
        ModelStreamEvent::QuestionnaireRequested {
            tool_call_id: Some("tc_ask".into()),
            questions: vec![],
        },
        ModelStreamEvent::Usage(quire_session_protocol::ContextUsage {
            input_tokens: 1,
            output_tokens: 1,
            cached_tokens: 0,
            context_used: 2,
            context_limit: 100,
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
        time::Duration,
    };

    use futures::StreamExt;

    struct DropFlag(Arc<AtomicBool>);

    impl Drop for DropFlag {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn dropping_the_stream_aborts_the_producer_task() {
        let aborted = Arc::new(AtomicBool::new(false));
        let producer_flag = Arc::clone(&aborted);
        let mut stream =
            spawn_producer_stream(CancellationToken::new(), move |sender| async move {
                let _flag = DropFlag(producer_flag);
                let _ = sender
                    .send(Ok(ModelStreamEvent::TextDelta("first".into())))
                    .await;
                tokio::time::sleep(Duration::from_secs(60)).await;
            });

        let first = stream.next().await;
        assert!(matches!(first, Some(Ok(ModelStreamEvent::TextDelta(text))) if text == "first"));
        drop(stream);

        for _ in 0..100 {
            if aborted.load(Ordering::SeqCst) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("producer task was not aborted when the stream was dropped");
    }

    #[tokio::test]
    async fn producer_events_flow_until_the_producer_completes() {
        let mut stream = spawn_producer_stream(CancellationToken::new(), |sender| async move {
            let _ = sender
                .send(Ok(ModelStreamEvent::ThinkingDelta("plan".into())))
                .await;
            let _ = sender
                .send(Ok(ModelStreamEvent::TextDelta("answer".into())))
                .await;
        });

        let first = stream.next().await;
        let second = stream.next().await;
        let end = stream.next().await;
        assert!(matches!(
            first,
            Some(Ok(ModelStreamEvent::ThinkingDelta(_)))
        ));
        assert!(matches!(second, Some(Ok(ModelStreamEvent::TextDelta(_)))));
        assert!(end.is_none());
    }

    #[tokio::test]
    async fn cancellation_stops_the_stream() {
        let token = CancellationToken::new();
        let mut stream = spawn_producer_stream(token.clone(), |sender| async move {
            let _ = sender
                .send(Ok(ModelStreamEvent::TextDelta("first".into())))
                .await;
            tokio::time::sleep(Duration::from_secs(60)).await;
        });

        let _ = stream.next().await;
        token.cancel();
        let end = stream.next().await;
        assert!(end.is_none());
    }

    #[tokio::test]
    async fn a_slow_consumer_applies_backpressure_to_the_producer() {
        let sent = Arc::new(AtomicUsize::new(0));
        let producer_sent = Arc::clone(&sent);
        let mut stream =
            spawn_producer_stream(CancellationToken::new(), move |sender| async move {
                for index in 0..(PRODUCER_CHANNEL_CAPACITY + 32) {
                    if sender
                        .send(Ok(ModelStreamEvent::TextDelta(index.to_string())))
                        .await
                        .is_err()
                    {
                        return;
                    }
                    producer_sent.fetch_add(1, Ordering::SeqCst);
                }
            });

        for _ in 0..100 {
            if sent.load(Ordering::SeqCst) >= PRODUCER_CHANNEL_CAPACITY {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            sent.load(Ordering::SeqCst),
            PRODUCER_CHANNEL_CAPACITY,
            "the producer must stop at the bounded queue capacity"
        );

        let _ = stream.next().await;
        for _ in 0..100 {
            if sent.load(Ordering::SeqCst) > PRODUCER_CHANNEL_CAPACITY {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            sent.load(Ordering::SeqCst) > PRODUCER_CHANNEL_CAPACITY,
            "consuming one frame must release one queue slot"
        );
    }
}
