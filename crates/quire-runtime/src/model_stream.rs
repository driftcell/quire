//! Pluggable model stream used by the runtime turn loop and offline tests.

use std::{collections::VecDeque, future::Future, pin::Pin, sync::Arc};

use futures::Stream;
use quire_session_protocol::{ContextUsage, QuestionItem, TaskTodoItem};
use serde_json::Value;
use tokio::{sync::Mutex, sync::mpsc, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::error::RuntimeResult;

#[derive(Debug, Clone, PartialEq)]
pub enum ModelStreamEvent {
    ThinkingDelta(String),
    TextDelta(String),
    TaskFocus {
        title: String,
        detail: Option<String>,
        section_id: Option<String>,
        todos: Vec<TaskTodoItem>,
    },
    TodosUpdated {
        todos: Vec<TaskTodoItem>,
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
    QuestionnaireRequested {
        tool_call_id: Option<String>,
        questions: Vec<QuestionItem>,
    },
    /// Real upstream token counts plus the resolved model context limit.
    Usage(ContextUsage),
    /// A queued user message takes over at the next safe model boundary.
    Superseded,
}

#[derive(Debug, Clone)]
pub struct ModelTurnRequest {
    pub session_id: String,
    pub run_id: String,
    pub agent_id: String,
    pub system_prompt: String,
    pub user_message: String,
    /// Transient KB update notice appended after the frozen system prefix.
    pub kb_update_notice: Option<String>,
    /// Initially bound Quire report; later bindings are shared within the run.
    pub doc_id: Option<String>,
    /// Session KB branch used for agent/skill lookup and branch-aware tools.
    pub kb_branch: Option<String>,
    /// Resolved model route for the primary agent.
    pub model_route: String,
    /// Tool allow-list from the primary agent definition.
    pub allowed_tools: Vec<String>,
    /// Cancels model streaming, tool execution and subagent inference.
    pub cancellation: CancellationToken,
    /// Whether the canonical memory already ends with this request's user prompt.
    pub prompt_recorded: bool,
    /// Whether this run commits its canonical suffix back to the loaded memory log.
    pub persist_history: bool,
}

pub type ModelEventStream =
    Pin<Box<dyn Stream<Item = RuntimeResult<ModelStreamEvent>> + Send + 'static>>;

pub trait ModelStreamProvider: Send + Sync {
    fn stream_turn(
        &self,
        request: ModelTurnRequest,
    ) -> Pin<Box<dyn Future<Output = RuntimeResult<ModelEventStream>> + Send + '_>>;

    /// Whether the provider commits canonical history and UI receipts itself.
    fn commits_canonical_history(&self) -> bool {
        false
    }
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

pub(crate) fn spawn_producer_stream<F, Fut>(producer: F) -> ModelEventStream
where
    F: FnOnce(mpsc::Sender<RuntimeResult<ModelStreamEvent>>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let (sender, mut receiver) = mpsc::channel(PRODUCER_CHANNEL_CAPACITY);
    let task = AbortOnDrop(tokio::spawn(producer(sender)));
    let stream = async_stream::stream! {
        let _task = task;
        while let Some(item) = receiver.recv().await {
            yield item;
        }
    };
    Box::pin(stream)
}

/// Deterministic local provider used when no external model is configured.
#[derive(Debug, Default)]
pub struct StubStreamProvider;

impl ModelStreamProvider for StubStreamProvider {
    fn stream_turn(
        &self,
        request: ModelTurnRequest,
    ) -> Pin<Box<dyn Future<Output = RuntimeResult<ModelEventStream>> + Send + '_>> {
        Box::pin(async move {
            let response = format!(
                "Runtime accepted the request for `{}`. Connect a rig model provider to stream the full answer.",
                request.session_id
            );
            let stream: ModelEventStream = Box::pin(futures::stream::iter(vec![
                Ok(ModelStreamEvent::ThinkingDelta(
                    "Preparing the next report action.".into(),
                )),
                Ok(ModelStreamEvent::TextDelta(response)),
            ]));
            Ok(stream)
        })
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

impl ModelStreamProvider for MockStreamProvider {
    fn stream_turn(
        &self,
        request: ModelTurnRequest,
    ) -> Pin<Box<dyn Future<Output = RuntimeResult<ModelEventStream>> + Send + '_>> {
        Box::pin(async move {
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
            let stream: ModelEventStream = Box::pin(futures::stream::iter(
                events.into_iter().map(Ok).collect::<Vec<_>>(),
            ));
            Ok(stream)
        })
    }
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
            todos: Vec::new(),
        },
        ModelStreamEvent::TodosUpdated { todos: Vec::new() },
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
            cost_rmb: None,
            output_price_rmb: None,
        }),
        ModelStreamEvent::Superseded,
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
        let mut stream = spawn_producer_stream(move |sender| async move {
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
        let mut stream = spawn_producer_stream(|sender| async move {
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
    async fn cancellation_waits_for_producer_cleanup_and_channel_close() {
        let token = CancellationToken::new();
        let producer_token = token.clone();
        let cleaned = Arc::new(AtomicBool::new(false));
        let producer_cleaned = Arc::clone(&cleaned);
        let mut stream = spawn_producer_stream(|sender| async move {
            let _ = sender
                .send(Ok(ModelStreamEvent::TextDelta("first".into())))
                .await;
            producer_token.cancelled().await;
            let _ = sender
                .send(Ok(ModelStreamEvent::TextDelta("cleanup".into())))
                .await;
            producer_cleaned.store(true, Ordering::SeqCst);
        });

        let _ = stream.next().await;
        token.cancel();
        let cleanup = stream.next().await;
        let end = stream.next().await;
        assert!(
            matches!(cleanup, Some(Ok(ModelStreamEvent::TextDelta(text))) if text == "cleanup")
        );
        assert!(end.is_none());
        assert!(cleaned.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn a_slow_consumer_applies_backpressure_to_the_producer() {
        let sent = Arc::new(AtomicUsize::new(0));
        let producer_sent = Arc::clone(&sent);
        let mut stream = spawn_producer_stream(move |sender| async move {
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
