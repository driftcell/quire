//! Live PostgreSQL/Redis integration tests for the independent runtime.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use base64::Engine as _;
use futures::StreamExt;
use quire_protocol::{
    BlockId, ChartRequirements, CommentAuthorType, CommentMessage, CommentStatus, CommentThread,
    DocId, EchartsBlock, Revision,
};
use quire_runtime::{
    Config, KbService, MockStreamProvider, ModelEventStream, ModelRegistry, ModelStreamEvent,
    ModelStreamProvider, ModelTurnRequest, ModelsDevImportRequest, ProviderInput, ProviderProtocol,
    RigModelStreamProvider, RuntimeError, RuntimeService, RuntimeStore, ToolCallRecord,
    bus::{InterruptCommand, RedisBus},
    control::RunRegistry,
    kb::{KbBranchStatus, KbItemKind, KbPromoteMode},
    mcp_registry::{McpServerInput, McpServerPatch},
    models::ModelInput,
    preview::ChartPreviewRecord,
    sandbox::build_sandbox_provider,
    service::{AttachmentWrite, PostMessageOutcome},
    store::SessionPatch,
};
use quire_session_protocol::{
    ActiveRunStatus, ContextUsage, MessageRole, PendingQueueSource, QuestionItem, QuestionOption,
    RunTraceSegment, RunTraceSummary, RunTraceToolItem, SendMessageRequest, SessionCommentRef,
    SessionEvent, SessionEventKind, SessionMessage,
};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::sync::{Mutex, Notify};
use tower::ServiceExt;

fn test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn take_sse_frame(buffer: &mut String) -> Option<String> {
    let (index, delimiter) = if let Some(index) = buffer.find("\r\n\r\n") {
        (index, 4)
    } else {
        (buffer.find("\n\n")?, 2)
    };
    let frame = buffer[..index].to_string();
    buffer.drain(..index + delimiter);
    Some(frame)
}

async fn next_sse_frame<S>(stream: &mut S, buffer: &mut String) -> Option<String>
where
    S: futures::Stream<Item = Result<axum::body::Bytes, reqwest::Error>> + Unpin,
{
    loop {
        if let Some(frame) = take_sse_frame(buffer) {
            return Some(frame);
        }
        let chunk = stream.next().await?;
        let chunk = chunk.ok()?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));
    }
}

async fn next_session_event<S>(stream: &mut S, buffer: &mut String) -> Option<SessionEvent>
where
    S: futures::Stream<Item = Result<axum::body::Bytes, reqwest::Error>> + Unpin,
{
    loop {
        let frame = next_sse_frame(stream, buffer).await?;
        let Some(data) = frame
            .lines()
            .find_map(|line| line.strip_prefix("data:").map(str::trim_start))
        else {
            continue;
        };
        if let Ok(event) = serde_json::from_str::<SessionEvent>(data) {
            return Some(event);
        }
    }
}
fn unique(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

fn comment_ref(thread_id: &str, comment: &str) -> SessionCommentRef {
    SessionCommentRef {
        thread_id: thread_id.into(),
        block_id: format!("b_{thread_id}"),
        end_block_id: None,
        anchor_blocks: Vec::new(),
        status: "open".into(),
        quote: None,
        end_quote: None,
        anchor_snapshot: None,
        comment: comment.into(),
    }
}

async fn mock_mcp_tools_list(axum::Json(body): axum::Json<Value>) -> axum::Json<Value> {
    let id = body.get("id").cloned().unwrap_or(Value::Null);
    let method = body
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "mock-mcp", "version": "1.0"}
        }),
        "tools/list" => json!({
            "tools": [
                {
                    "name": "alpha_tool",
                    "description": "Alpha tool",
                    "inputSchema": {"type": "object"}
                },
                {
                    "name": "beta_tool",
                    "description": "Beta tool",
                    "inputSchema": {"type": "object"}
                }
            ]
        }),
        _ => json!({}),
    };
    axum::Json(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn database_url() -> String {
    let _ = dotenvy::dotenv();
    let url = std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .unwrap_or_else(|_| "postgres://report:report@127.0.0.1:5432/report_core_test".into());
    let database = url
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default();
    assert!(
        !database.is_empty() && database != "report_core",
        "refusing to run destructive runtime tests against `{database}`; set TEST_DATABASE_URL to a disposable database (for example .../report_core_test)"
    );
    url
}

fn redis_url() -> String {
    let _ = dotenvy::dotenv();
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into())
}

async fn test_service() -> (RuntimeService, RuntimeStore) {
    test_service_with_quire_base("http://127.0.0.1:1").await
}

fn test_config(base_url: &str) -> Config {
    Config {
        database_url: database_url(),
        redis_url: redis_url(),
        bind_addr: "127.0.0.1:0".into(),
        quire_base_url: base_url.into(),
        quire_rw_token: SecretString::from("test-token"),
        quire_ro_token: SecretString::from("test-token"),
        api_token: SecretString::from("runtime-test-token"),
        model_api_key: None,
        model_provider: "mock".into(),
        model_base_url: None,
        master_key: Some(SecretString::from("test-master-key")),
        models_dev_url: "https://models.dev/api.json".into(),
        workspace_dir: std::env::temp_dir(),
        default_model: "stub/echo".into(),
        sse_replay_limit: 500,
        preview_throttle_ms: 65,
        preview_base_url: "http://127.0.0.1:5173".into(),
        preview_public_url: "http://localhost:5173".into(),
        playwright_mcp_url: None,
        sandbox: quire_runtime::sandbox::SandboxConfig::default(),
        oss: quire_runtime::oss::OssConfig::default(),
    }
}

async fn test_service_with_quire_base(base_url: &str) -> (RuntimeService, RuntimeStore) {
    test_service_with_config(test_config(base_url)).await
}

async fn test_service_with_config(config: Config) -> (RuntimeService, RuntimeStore) {
    let store = RuntimeStore::connect(&database_url(), &redis_url())
        .await
        .expect("PostgreSQL and Redis are required");
    store.migrate().await.expect("runtime migrations");
    let kb = KbService::new(store.clone());
    let models = ModelRegistry::new(
        store.clone(),
        Some(SecretString::from("test-master-key")),
        "https://models.dev/api.json",
    );
    let service = RuntimeService::new(store.clone(), kb, Arc::new(models), Arc::new(config));
    (service, store)
}

async fn install_scripted_agent(
    service: &RuntimeService,
    model_base_url: &str,
    allowed_tools: &[String],
) -> String {
    let provider_id = unique("scripted-provider");
    service
        .models()
        .upsert_provider(ProviderInput {
            provider_id: provider_id.clone(),
            name: "Scripted OpenAI".into(),
            protocol: ProviderProtocol::OpenaiCompatible,
            base_url: model_base_url.to_string(),
            api_key_ref: "test-key".into(),
            enabled: true,
        })
        .await
        .unwrap();
    let route_id = unique("scripted-model");
    service
        .models()
        .upsert_model(ModelInput {
            id: route_id.clone(),
            provider_id,
            model_id: "mock-model".into(),
            display_name: "Scripted Model".into(),
            context_limit: 32_000,
            max_output_tokens: Some(1024),
            supports_reasoning: false,
            supports_tools: true,
            supports_vision: false,
            cost_input: None,
            cost_output: None,
            cost_cache_read: None,
            enabled: true,
        })
        .await
        .unwrap();
    service
        .kb()
        .upsert_agent(
            "main",
            "",
            &route_id,
            "Scripted attachment mount agent",
            allowed_tools,
            true,
            true,
        )
        .await
        .unwrap();
    route_id
}

fn with_local_rig_provider(service: RuntimeService) -> RuntimeService {
    let sandbox_provider = build_sandbox_provider(&service.config().sandbox).unwrap();
    let provider = RigModelStreamProvider::new(
        service.kb().clone(),
        Arc::new(service.models().clone()),
        Arc::new(service.config().clone()),
        service.mcp_registry(),
        sandbox_provider,
        service.pending_queue(),
    );
    service.with_model_stream_provider(Arc::new(provider))
}

async fn start_scripted_model(turns: Vec<Vec<String>>) -> (String, tokio::task::JoinHandle<()>) {
    let state = Arc::new(ScriptedChatState {
        turns: Mutex::new(VecDeque::from(turns)),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new()
                .route("/v1/responses", axum::routing::post(mock_scripted_chat))
                .with_state(state),
        )
        .await
        .unwrap();
    });
    (format!("http://{address}/v1"), server)
}

async fn post_json(app: &axum::Router, uri: &str, body: Value) -> (axum::http::StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(uri)
                .header("authorization", "Bearer runtime-test-token")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

fn test_oss_config() -> quire_runtime::oss::OssConfig {
    quire_runtime::oss::OssConfig {
        bucket: Some("agentcore-ws-test".into()),
        region: Some("cn-hangzhou".into()),
        public_endpoint: Some("https://oss-cn-hangzhou.aliyuncs.com".into()),
        internal_endpoint: Some("https://oss-cn-hangzhou-internal.aliyuncs.com".into()),
        access_key_id: Some("LTAI_TEST_ACCESS_KEY".into()),
        access_key_secret: Some(SecretString::from("test-secret-value")),
        key_prefix: "quire/attachments".into(),
    }
}

fn find_trace_tool<'a>(
    traces: &'a [RunTraceSummary],
    tool_name: &str,
) -> Option<&'a RunTraceToolItem> {
    traces
        .iter()
        .flat_map(|trace| trace.segments.iter())
        .find_map(|segment| match segment {
            RunTraceSegment::Execution { tools, .. } => {
                tools.iter().find(|tool| tool.tool_name == tool_name)
            }
            _ => None,
        })
}

async fn wait_for_idle(store: &RuntimeStore, session_id: &str) {
    for _ in 0..200 {
        let session = store.get_session_summary(session_id).await.unwrap();
        if session.active_run_status != ActiveRunStatus::Running {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("session {session_id} never became idle");
}

async fn wait_for_running(store: &RuntimeStore, session_id: &str) {
    for _ in 0..200 {
        let session = store.get_session_summary(session_id).await.unwrap();
        if session.active_run_status == ActiveRunStatus::Running {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("session {session_id} never entered running");
}

async fn remove_branch(store: &RuntimeStore, name: &str) {
    let _ = sqlx::query("DELETE FROM kb_branches WHERE name = $1")
        .bind(name)
        .execute(store.pool())
        .await;
}

async fn parse_body(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    if text.trim().is_empty() {
        return Value::Null;
    }
    if text.trim_start().starts_with("event:") || text.contains("\ndata:") {
        for line in text.lines() {
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            if let Ok(value) = serde_json::from_str::<Value>(data) {
                return value;
            }
        }
    }
    serde_json::from_str(&text).unwrap_or(Value::String(text))
}

async fn http_json(
    app: &axum::Router,
    method: axum::http::Method,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (axum::http::StatusCode, Value) {
    let mut builder = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header("host", "localhost");
    let body = match body {
        Some(body) => {
            builder = builder.header("content-type", "application/json");
            axum::body::Body::from(serde_json::to_vec(&body).unwrap())
        }
        None => axum::body::Body::empty(),
    };
    let response = app
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    (status, parse_body(response).await)
}

async fn mcp_request(
    app: &axum::Router,
    token: &str,
    session_id: Option<&str>,
    protocol: &str,
    body: &Value,
) -> (axum::http::StatusCode, Option<String>, Value) {
    let mut builder = axum::http::Request::builder()
        .method(axum::http::Method::POST)
        .uri("/admin/mcp")
        .header("authorization", format!("Bearer {token}"))
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", protocol);
    if let Some(session_id) = session_id {
        builder = builder.header("mcp-session-id", session_id);
    }
    let response = app
        .clone()
        .oneshot(
            builder
                .body(axum::body::Body::from(serde_json::to_vec(body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let session = response
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    (status, session, parse_body(response).await)
}

struct AdminMcpClient {
    app: axum::Router,
    token: String,
    session_id: String,
    protocol_version: String,
    next_id: AtomicU64,
}

impl AdminMcpClient {
    async fn connect(app: axum::Router, token: &str) -> Self {
        let initialize = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "runtime-admin-test", "version": "1.0"}
            }
        });
        let (status, session_id, body) =
            mcp_request(&app, token, None, "2025-11-25", &initialize).await;
        assert_eq!(status, axum::http::StatusCode::OK);
        let session_id = session_id.expect("initialize must return mcp-session-id");
        let protocol_version = body
            .get("result")
            .and_then(|result| result.get("protocolVersion"))
            .and_then(Value::as_str)
            .unwrap_or("2025-11-25")
            .to_string();
        let (status, _, _) = mcp_request(
            &app,
            token,
            Some(&session_id),
            &protocol_version,
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        )
        .await;
        assert!(status.is_success());
        Self {
            app,
            token: token.to_string(),
            session_id,
            protocol_version,
            next_id: AtomicU64::new(2),
        }
    }

    async fn rpc(&self, method: &str, params: Value) -> Value {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (status, _, body) = mcp_request(
            &self.app,
            &self.token,
            Some(&self.session_id),
            &self.protocol_version,
            &json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
        )
        .await;
        assert!(status.is_success(), "MCP {method} failed with {status}");
        assert!(
            body.get("error").is_none(),
            "MCP {method} returned error: {body}"
        );
        body.get("result").cloned().unwrap_or(Value::Null)
    }

    async fn tool_names(&self) -> Vec<String> {
        let result = self.rpc("tools/list", json!({})).await;
        result
            .get("tools")
            .and_then(Value::as_array)
            .map(|tools| {
                tools
                    .iter()
                    .filter_map(|tool| tool.get("name").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    async fn call_tool(&self, name: &str, arguments: Value) -> Value {
        let result = self
            .rpc("tools/call", json!({"name": name, "arguments": arguments}))
            .await;
        assert_ne!(
            result.get("isError").and_then(Value::as_bool),
            Some(true),
            "tool {name} returned isError: {result}"
        );
        if let Some(structured) = result.get("structuredContent") {
            return structured.clone();
        }
        let text = result
            .get("content")
            .and_then(Value::as_array)
            .and_then(|content| content.iter().next())
            .and_then(|item| item.get("text"))
            .and_then(Value::as_str)
            .expect("tool result content");
        serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()))
    }
}

struct RecordingStreamProvider {
    requests: Arc<Mutex<Vec<ModelTurnRequest>>>,
    turns: Arc<Mutex<VecDeque<Vec<ModelStreamEvent>>>>,
}

impl RecordingStreamProvider {
    fn new(events: Vec<ModelStreamEvent>) -> (Self, Arc<Mutex<Vec<ModelTurnRequest>>>) {
        Self::with_turns(vec![events])
    }

    fn with_turns(turns: Vec<Vec<ModelStreamEvent>>) -> (Self, Arc<Mutex<Vec<ModelTurnRequest>>>) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                requests: requests.clone(),
                turns: Arc::new(Mutex::new(VecDeque::from(turns))),
            },
            requests,
        )
    }
}

#[async_trait::async_trait]
impl ModelStreamProvider for RecordingStreamProvider {
    async fn stream_turn(
        &self,
        request: ModelTurnRequest,
    ) -> quire_runtime::RuntimeResult<ModelEventStream> {
        self.requests.lock().await.push(request);
        let events = self
            .turns
            .lock()
            .await
            .pop_front()
            .unwrap_or_else(|| vec![ModelStreamEvent::TextDelta("mock reply".into())]);
        Ok(Box::pin(futures::stream::iter(
            events.into_iter().map(Ok).collect::<Vec<_>>(),
        )))
    }
}

struct BlockingSummaryProvider {
    released: Arc<AtomicBool>,
    notify: Arc<tokio::sync::Notify>,
}

impl BlockingSummaryProvider {
    fn new() -> Self {
        Self {
            released: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    fn release(&self) {
        self.released.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }
}

#[async_trait::async_trait]
impl ModelStreamProvider for BlockingSummaryProvider {
    async fn stream_turn(
        &self,
        _request: ModelTurnRequest,
    ) -> quire_runtime::RuntimeResult<ModelEventStream> {
        while !self.released.load(Ordering::SeqCst) {
            self.notify.notified().await;
        }
        Ok(Box::pin(futures::stream::iter(vec![Ok(
            ModelStreamEvent::TextDelta(compaction_summary_text().into()),
        )])))
    }
}

struct FailingStreamProvider;

#[async_trait::async_trait]
impl ModelStreamProvider for FailingStreamProvider {
    async fn stream_turn(
        &self,
        _request: ModelTurnRequest,
    ) -> quire_runtime::RuntimeResult<ModelEventStream> {
        Ok(Box::pin(futures::stream::iter(vec![Err(
            RuntimeError::Internal("summary provider failed".into()),
        )])))
    }
}

#[derive(Default)]
struct MockChatState {
    request_count: AtomicUsize,
    requests: Mutex<Vec<Value>>,
    started: Notify,
    release: Notify,
}

fn sse(value: Value) -> String {
    format!("data: {value}\n\n")
}

fn response_base(output: Value) -> Value {
    json!({
        "id": "resp_mock",
        "object": "response",
        "created_at": 0,
        "status": "completed",
        "model": "mock-model",
        "output": output,
        "tools": []
    })
}

fn function_item(title: &str) -> Value {
    json!({
        "type": "function_call",
        "id": "fc_mock",
        "call_id": "call-title",
        "name": "set_session_title",
        "arguments": json!({"title": title}).to_string(),
        "status": "completed"
    })
}

fn message_item(text: &str) -> Value {
    json!({
        "type": "message",
        "id": "msg_mock",
        "role": "assistant",
        "status": "completed",
        "content": [{"type": "output_text", "annotations": [], "logprobs": [], "text": text}]
    })
}

fn tool_call_stream() -> Vec<String> {
    let item = function_item("Queued title");
    vec![
        sse(json!({
            "type": "response.created",
            "sequence_number": 0,
            "response": response_base(json!([]))
        })),
        sse(json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "sequence_number": 1,
            "item": {
                "type": "function_call",
                "id": "fc_mock",
                "call_id": "call-title",
                "name": "set_session_title",
                "arguments": "",
                "status": "in_progress"
            }
        })),
        sse(json!({
            "type": "response.function_call_arguments.delta",
            "item_id": "fc_mock",
            "output_index": 0,
            "content_index": 0,
            "sequence_number": 2,
            "delta": item["arguments"]
        })),
        sse(json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "sequence_number": 3,
            "item": item
        })),
        sse(json!({
            "type": "response.completed",
            "sequence_number": 4,
            "response": response_base(json!([item]))
        })),
    ]
}

fn text_stream() -> Vec<String> {
    let text = "已完成排队消息。";
    let item = message_item(text);
    vec![
        sse(json!({
            "type": "response.created",
            "sequence_number": 0,
            "response": response_base(json!([]))
        })),
        sse(json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "sequence_number": 1,
            "item": {
                "type": "message",
                "id": "msg_mock",
                "role": "assistant",
                "status": "in_progress",
                "content": []
            }
        })),
        sse(json!({
            "type": "response.output_text.delta",
            "item_id": "msg_mock",
            "output_index": 0,
            "content_index": 0,
            "sequence_number": 2,
            "delta": text
        })),
        sse(json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "sequence_number": 3,
            "item": item
        })),
        sse(json!({
            "type": "response.completed",
            "sequence_number": 4,
            "response": response_base(json!([item]))
        })),
    ]
}

async fn mock_chat_completions(
    axum::extract::State(state): axum::extract::State<Arc<MockChatState>>,
    axum::Json(body): axum::Json<Value>,
) -> axum::response::Response {
    let index = state.request_count.fetch_add(1, Ordering::SeqCst);
    state.requests.lock().await.push(body);
    let chunks = if index == 0 {
        state.started.notify_one();
        state.release.notified().await;
        tool_call_stream()
    } else {
        text_stream()
    };
    let stream = futures::stream::iter(
        chunks
            .into_iter()
            .map(|chunk| Ok::<_, std::convert::Infallible>(axum::body::Bytes::from(chunk))),
    );
    axum::response::Response::builder()
        .header("content-type", "text/event-stream")
        .body(axum::body::Body::from_stream(stream))
        .unwrap()
}

#[derive(Default)]
struct ScriptedChatState {
    turns: Mutex<VecDeque<Vec<String>>>,
}

async fn mock_scripted_chat(
    axum::extract::State(state): axum::extract::State<Arc<ScriptedChatState>>,
    axum::Json(_body): axum::Json<Value>,
) -> axum::response::Response {
    let chunks = state
        .turns
        .lock()
        .await
        .pop_front()
        .unwrap_or_else(text_stream);
    let stream = futures::stream::iter(
        chunks
            .into_iter()
            .map(|chunk| Ok::<_, std::convert::Infallible>(axum::body::Bytes::from(chunk))),
    );
    axum::response::Response::builder()
        .header("content-type", "text/event-stream")
        .body(axum::body::Body::from_stream(stream))
        .unwrap()
}

fn tool_call_stream_for(tool_name: &str, call_id: &str, arguments: &Value) -> Vec<String> {
    let arguments = arguments.to_string();
    let item = json!({
        "type": "function_call",
        "id": format!("fc_{call_id}"),
        "call_id": call_id,
        "name": tool_name,
        "arguments": arguments,
        "status": "completed"
    });
    vec![
        sse(json!({
            "type": "response.created",
            "sequence_number": 0,
            "response": response_base(json!([]))
        })),
        sse(json!({
            "type": "response.output_item.added",
            "output_index": 0,
            "sequence_number": 1,
            "item": {
                "type": "function_call",
                "id": format!("fc_{call_id}"),
                "call_id": call_id,
                "name": tool_name,
                "arguments": "",
                "status": "in_progress"
            }
        })),
        sse(json!({
            "type": "response.function_call_arguments.delta",
            "item_id": format!("fc_{call_id}"),
            "output_index": 0,
            "content_index": 0,
            "sequence_number": 2,
            "delta": arguments
        })),
        sse(json!({
            "type": "response.output_item.done",
            "output_index": 0,
            "sequence_number": 3,
            "item": item
        })),
        sse(json!({
            "type": "response.completed",
            "sequence_number": 4,
            "response": response_base(json!([item]))
        })),
    ]
}

#[derive(Clone)]
struct QuestionnaireGateProvider {
    started: Arc<Notify>,
    release: Arc<Notify>,
}

impl QuestionnaireGateProvider {
    fn new() -> Self {
        Self {
            started: Arc::new(Notify::new()),
            release: Arc::new(Notify::new()),
        }
    }
}

#[async_trait::async_trait]
impl ModelStreamProvider for QuestionnaireGateProvider {
    async fn stream_turn(
        &self,
        _request: ModelTurnRequest,
    ) -> quire_runtime::RuntimeResult<ModelEventStream> {
        self.started.notify_one();
        self.release.notified().await;
        Ok(Box::pin(futures::stream::iter(vec![Ok(
            ModelStreamEvent::QuestionnaireRequested {
                tool_call_id: Some("ask-1".into()),
                questions: question_items(),
            },
        )])))
    }
}

#[derive(Clone)]
struct GateStreamProvider {
    released: Arc<AtomicBool>,
    notify: Arc<tokio::sync::Notify>,
}

impl GateStreamProvider {
    fn new() -> Self {
        Self {
            released: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    fn release(&self) {
        self.released.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }
}

#[async_trait::async_trait]
impl ModelStreamProvider for GateStreamProvider {
    async fn stream_turn(
        &self,
        _request: ModelTurnRequest,
    ) -> quire_runtime::RuntimeResult<ModelEventStream> {
        while !self.released.load(Ordering::SeqCst) {
            self.notify.notified().await;
        }
        Ok(Box::pin(futures::stream::iter(vec![Ok(
            ModelStreamEvent::TextDelta("gated reply".into()),
        )])))
    }
}

fn compaction_summary_text() -> &'static str {
    "## Objective & Goal\nhandoff summary\n## Confirmed Scope & Calibers\n2021 projects\n## Report State (doc_id / completed sections / source_ids)\ndoc=runtime-doc\n## Open Comments & Next Steps\ncontinue chapter 2"
}

fn question_items() -> Vec<QuestionItem> {
    vec![QuestionItem {
        id: "scope".into(),
        title: "Which scope should the summary use?".into(),
        description: Some("Pick one scope.".into()),
        multiple: false,
        required: true,
        options: vec![
            QuestionOption {
                id: "q3".into(),
                label: "Q3".into(),
                description: None,
            },
            QuestionOption {
                id: "q4".into(),
                label: "Q4".into(),
                description: None,
            },
        ],
    }]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_recovery_marks_running_runs_interrupted_and_appends_failure() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let session = service
        .create_session(&unique("recovery"), Some("runtime-doc"))
        .await
        .unwrap();
    let run = store
        .create_run(&session.session_id, None, "main", "stub/echo")
        .await
        .unwrap();
    let recovered = service.boot().await.unwrap();
    assert!(recovered >= 1);
    let run = store.get_run(&run.run_id).await.unwrap();
    assert_eq!(run.status, "interrupted");
    let detail = service.session_detail(&session.session_id).await.unwrap();
    assert_eq!(
        detail.summary.active_run_status,
        ActiveRunStatus::Interrupted
    );
    let events = store
        .list_events(&session.session_id, 0, 100)
        .await
        .unwrap();
    assert!(events.iter().any(|event| matches!(
        &event.kind,
        SessionEventKind::TurnFailed { recoverable, .. } if *recoverable
    )));
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pinning_preserves_updated_at_and_orders_pinned_sessions_first() {
    let _guard = test_lock().lock().await;
    let (service, _store) = test_service().await;
    let first = service
        .create_session(&unique("pin-first"), None)
        .await
        .unwrap();
    let second = service
        .create_session(&unique("pin-second"), None)
        .await
        .unwrap();
    let first_updated_at = first.updated_at;
    let second_updated_at = second.updated_at;

    let first_pinned = service
        .update_session(
            &first.session_id,
            SessionPatch {
                pinned: Some(true),
                ..SessionPatch::default()
            },
        )
        .await
        .unwrap();
    assert!(first_pinned.pinned_at.is_some());
    assert_eq!(
        first_pinned.updated_at, first_updated_at,
        "pinning must not move the session into today's date group"
    );

    tokio::time::sleep(Duration::from_millis(10)).await;
    let second_pinned = service
        .update_session(
            &second.session_id,
            SessionPatch {
                pinned: Some(true),
                ..SessionPatch::default()
            },
        )
        .await
        .unwrap();
    assert!(second_pinned.pinned_at.is_some());
    assert_eq!(second_pinned.updated_at, second_updated_at);
    assert!(
        second_pinned.pinned_at.unwrap() > first_pinned.pinned_at.unwrap(),
        "the most recent pin should sort first"
    );

    let sessions = service.list_sessions(200, 0).await.unwrap();
    let first_index = sessions
        .iter()
        .position(|session| session.session_id == first.session_id)
        .unwrap();
    let second_index = sessions
        .iter()
        .position(|session| session.session_id == second.session_id)
        .unwrap();
    assert!(
        second_index < first_index,
        "pinned sessions must sort before unpinned sessions and by pinned_at DESC"
    );

    let first_unpinned = service
        .update_session(
            &first.session_id,
            SessionPatch {
                pinned: Some(false),
                ..SessionPatch::default()
            },
        )
        .await
        .unwrap();
    assert!(first_unpinned.pinned_at.is_none());
    assert_eq!(
        first_unpinned.updated_at, first_updated_at,
        "unpinning must not rewrite updated_at"
    );

    let sessions = service.list_sessions(200, 0).await.unwrap();
    let first_index = sessions
        .iter()
        .position(|session| session.session_id == first.session_id)
        .unwrap();
    let second_index = sessions
        .iter()
        .position(|session| session.session_id == second.session_id)
        .unwrap();
    assert!(second_index < first_index);
    assert!(sessions[second_index].pinned_at.is_some());
    assert!(sessions[first_index].pinned_at.is_none());

    service.delete_session(&first.session_id).await.unwrap();
    service.delete_session(&second.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_message_id_is_idempotent_and_does_not_duplicate_runs() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let session = service
        .create_session(&unique("idempotent"), Some("runtime-doc"))
        .await
        .unwrap();
    let request = SendMessageRequest {
        client_msg_id: unique("client-msg"),
        content: "Draft an executive summary".into(),
        comment_refs: Vec::new(),
        attachment_ids: Vec::new(),
    };
    let first = service
        .post_message(&session.session_id, &request)
        .await
        .unwrap();
    let (first, run_id) = match first {
        PostMessageOutcome::Started { message, run_id } => (message, run_id),
        other => panic!("expected started message, got {other:?}"),
    };
    wait_for_idle(&store, &session.session_id).await;
    let idle = service.session_detail(&session.session_id).await.unwrap();
    assert_eq!(idle.summary.context_usage.context_limit, 200_000);
    let second = service
        .post_message(&session.session_id, &request)
        .await
        .unwrap();
    let second = match second {
        PostMessageOutcome::Replayed { message } => message,
        other => panic!("expected replayed message, got {other:?}"),
    };
    assert_eq!(second.message_id, first.message_id);
    let messages = store.list_messages(&session.session_id).await.unwrap();
    let user_messages: Vec<_> = messages
        .iter()
        .filter(|message| message.role == quire_session_protocol::MessageRole::User)
        .collect();
    assert_eq!(user_messages.len(), 1);
    let tool_call = service
        .record_tool_call(ToolCallRecord {
            session_id: session.session_id.clone(),
            run_id,
            tool_name: "apply_operations".into(),
            input: serde_json::json!({"ops":[]}),
            output: serde_json::json!({"committed":true}),
            status: "completed".into(),
            duration_ms: 12,
        })
        .await
        .unwrap();
    let loaded = service
        .get_tool_call(&session.session_id, &tool_call.tool_call_id)
        .await
        .unwrap();
    assert_eq!(loaded.output["committed"], true);
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structured_comment_refs_roundtrip_and_replay() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let session = service
        .create_session(&unique("structured-comment"), Some("runtime-doc"))
        .await
        .unwrap();
    let snapshot = comment_ref("ct_structured", "Revise only this paragraph");
    let request = SendMessageRequest {
        client_msg_id: unique("structured-comment-request"),
        content: "Handle the referenced comment".into(),
        comment_refs: vec![snapshot.clone()],
        attachment_ids: Vec::new(),
    };

    let first = service
        .post_message(&session.session_id, &request)
        .await
        .unwrap();
    let first = match first {
        PostMessageOutcome::Started { message, .. } => message,
        other => panic!("expected started message, got {other:?}"),
    };
    assert_eq!(first.content, "Handle the referenced comment");
    assert_eq!(first.comment_refs, vec![snapshot.clone()]);
    wait_for_idle(&store, &session.session_id).await;

    let second = service
        .post_message(&session.session_id, &request)
        .await
        .unwrap();
    let second = match second {
        PostMessageOutcome::Replayed { message } => message,
        other => panic!("expected replayed message, got {other:?}"),
    };
    assert_eq!(second.message_id, first.message_id);
    assert_eq!(second.comment_refs, vec![snapshot.clone()]);

    let stored = store
        .find_message_by_client_id(&session.session_id, &request.client_msg_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.comment_refs, vec![snapshot]);
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn post_message_broadcasts_user_message_added_before_turn_started() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let session = service
        .create_session(&unique("message-event"), Some("runtime-doc"))
        .await
        .unwrap();
    let outcome = service
        .post_message(
            &session.session_id,
            &SendMessageRequest {
                client_msg_id: unique("message-event"),
                content: "Broadcast this message".into(),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    let message = match outcome {
        PostMessageOutcome::Started { message, .. } => message,
        other => panic!("expected started message, got {other:?}"),
    };
    wait_for_idle(&store, &session.session_id).await;
    let events = store
        .list_events(&session.session_id, 0, 100)
        .await
        .unwrap();
    let first = events.first().expect("user message event");
    assert!(matches!(
        &first.kind,
        SessionEventKind::UserMessageAdded { message_id } if message_id == &message.message_id
    ));
    assert!(
        events
            .iter()
            .any(|event| matches!(&event.kind, SessionEventKind::TurnStarted))
    );
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_replay_returns_events_after_the_requested_seq() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let session = service
        .create_session(&unique("replay"), Some("runtime-doc"))
        .await
        .unwrap();
    for index in 0..3 {
        store
            .append_event(
                &session.session_id,
                "run_replay",
                "main",
                None,
                SessionEventKind::TextDelta {
                    delta: format!("chunk-{index}"),
                },
            )
            .await
            .unwrap();
    }
    let replay = store
        .list_events(&session.session_id, 1, 100)
        .await
        .unwrap();
    assert_eq!(replay.len(), 2);
    assert_eq!(replay.first().map(|event| event.seq), Some(2));
    assert_eq!(replay.last().map(|event| event.seq), Some(3));
    assert_eq!(
        store
            .count_events_after(&session.session_id, 1)
            .await
            .unwrap(),
        2
    );
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn event_stream_replays_a_small_gap_without_disconnecting() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let session = service
        .create_session(&unique("sse-gap"), Some("runtime-doc"))
        .await
        .unwrap();
    for index in 0..3 {
        store
            .append_event(
                &session.session_id,
                "run_gap",
                "main",
                None,
                SessionEventKind::TextDelta {
                    delta: format!("before-{index}"),
                },
            )
            .await
            .unwrap();
    }

    let app = quire_runtime::http::router(service.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let response = reqwest::Client::new()
        .get(format!(
            "http://{address}/api/sessions/{}/events?afterSeq=3",
            session.session_id
        ))
        .bearer_auth("runtime-test-token")
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    store
        .append_event(
            &session.session_id,
            "run_gap",
            "main",
            None,
            SessionEventKind::TextDelta {
                delta: "live-4".into(),
            },
        )
        .await
        .unwrap();
    let live_four = tokio::time::timeout(
        Duration::from_secs(5),
        next_session_event(&mut stream, &mut buffer),
    )
    .await
    .unwrap()
    .expect("live event 4");
    assert_eq!(live_four.seq, 4);

    for seq in 5..=6 {
        let event = SessionEvent::new(
            seq,
            "run_gap",
            "main",
            None,
            SessionEventKind::TextDelta {
                delta: format!("missing-{seq}"),
            },
        );
        let event_value = serde_json::to_value(&event).unwrap();
        sqlx::query(
            r#"INSERT INTO session_events
               (session_id, seq, run_id, agent_id, parent_run_id, event_type, event, created_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
        )
        .bind(&session.session_id)
        .bind(i64::try_from(seq).unwrap())
        .bind(&event.run_id)
        .bind(&event.agent_id)
        .bind(event.parent_run_id.as_deref())
        .bind(event.event_name())
        .bind(event_value)
        .bind(event.timestamp)
        .execute(store.pool())
        .await
        .unwrap();
    }
    sqlx::query("UPDATE agent_sessions SET last_event_seq = 6 WHERE session_id = $1")
        .bind(&session.session_id)
        .execute(store.pool())
        .await
        .unwrap();
    store
        .append_event(
            &session.session_id,
            "run_gap",
            "main",
            None,
            SessionEventKind::TextDelta {
                delta: "live-7".into(),
            },
        )
        .await
        .unwrap();

    let mut replayed = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(event) = next_session_event(&mut stream, &mut buffer).await {
            assert_ne!(event.event_name(), "sync_required");
            replayed.push(event.seq);
            if event.seq >= 7 {
                break;
            }
        }
    })
    .await
    .expect("stream must stay open and replay events 5-7");
    assert_eq!(replayed, vec![5, 6, 7]);

    service.delete_session(&session.session_id).await.unwrap();
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn knowledge_update_keeps_the_pinned_prompt_frozen() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let session = service
        .create_session(&unique("prompt-cache"), Some("runtime-doc"))
        .await
        .unwrap();
    let initial_version = service.kb().current_version().await.unwrap();
    store
        .set_pinned_prompt(&session.session_id, "FROZEN SYSTEM PREFIX", initial_version)
        .await
        .unwrap();
    let bumped = service
        .kb()
        .upsert_document(
            "prompt-cache-test",
            "Prompt cache test",
            "Updated runtime guidance",
        )
        .await
        .unwrap();
    assert!(bumped.version >= 1);
    let context = service
        .resume_context(&session.session_id, "Next request")
        .await
        .unwrap();
    assert_eq!(context.pinned_system_prompt, "FROZEN SYSTEM PREFIX");
    assert!(
        context
            .messages
            .iter()
            .any(|message| message.content.starts_with("<knowledge_update>"))
    );
    let (pinned, _) = store.pinned_prompt(&session.session_id).await.unwrap();
    assert_eq!(pinned.as_deref(), Some("FROZEN SYSTEM PREFIX"));
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn redis_interrupt_reaches_two_independent_runtimes() {
    let _guard = test_lock().lock().await;
    let bus = RedisBus::connect(&redis_url()).unwrap();
    let first = RunRegistry::default();
    let second = RunRegistry::default();
    let token_a = first.register("run_cross_instance").await;
    let token_b = second.register("run_cross_instance").await;
    let _a = quire_runtime::control::spawn_interrupt_listener(bus.clone(), first.clone());
    let _b = quire_runtime::control::spawn_interrupt_listener(bus.clone(), second.clone());
    tokio::time::sleep(Duration::from_millis(100)).await;
    bus.publish_interrupt(&InterruptCommand {
        session_id: "sess_cross".into(),
        run_id: "run_cross_instance".into(),
    })
    .await
    .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline
        && !(token_a.is_cancelled() && token_b.is_cancelled())
    {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(token_a.is_cancelled());
    assert!(token_b.is_cancelled());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn seed_is_idempotent_and_preserves_online_edits() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    for statement in [
        "DELETE FROM skill_resources",
        "DELETE FROM skill_definitions",
        "DELETE FROM agent_definitions",
        "DELETE FROM kb_documents",
    ] {
        sqlx::query(statement).execute(store.pool()).await.unwrap();
    }
    let workspace = tempfile::tempdir().unwrap();
    write_seed_workspace(workspace.path());
    let first = service
        .kb()
        .seed(workspace.path(), "stub/echo")
        .await
        .unwrap();
    assert_eq!(first.documents, 0);
    assert_eq!(first.agents, 2);
    assert_eq!(first.skills, 1);
    assert!(first.kb_version > 1);
    let default_agent = service
        .kb()
        .get_default_agent_on(None)
        .await
        .unwrap()
        .expect("default main agent seeded from agents/main.md");
    assert_eq!(default_agent.name, "main");
    assert!(default_agent.is_default);
    assert_eq!(
        default_agent.system_prompt,
        "Main prompt from agents/main.md"
    );
    let skills = service.kb().list_skills().await.unwrap();
    assert!(
        skills
            .iter()
            .any(|skill| skill.name == "report-review" && !skill.resources.is_empty())
    );
    service
        .kb()
        .upsert_agent("main", "", "stub/echo", "ONLINE EDIT", &[], true, true)
        .await
        .unwrap();
    let second = service.seed().await.unwrap();
    assert_eq!(second.documents, 0);
    let default_agent = service
        .kb()
        .get_default_agent_on(None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(default_agent.system_prompt, "ONLINE EDIT");
    for statement in [
        "DELETE FROM skill_resources",
        "DELETE FROM skill_definitions",
        "DELETE FROM agent_definitions",
        "DELETE FROM kb_documents",
    ] {
        sqlx::query(statement).execute(store.pool()).await.unwrap();
    }
}

fn write_seed_workspace(root: &Path) {
    let agents = root.join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(
        agents.join("main.md"),
        "---\nname: main\nmodel: stub/main\nallowed_tools: [\"*\"]\n---\nMain prompt from agents/main.md",
    )
    .unwrap();
    std::fs::write(
        agents.join("reviewer.md"),
        "---\nname: reviewer\nmodel: stub/reviewer\nallowed_tools: [lint_document, read_blocks]\n---\nReview every chapter.",
    )
    .unwrap();
    let skill = root.join("skills").join("report-review");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: report-review\ndescription: Review report structure and citations\n---\nUse the review checklist.",
    )
    .unwrap();
    std::fs::write(
        skill.join("checklist.md"),
        "- Check citations\n- Check tables",
    )
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attachments_reject_traversal_and_decode_base64() {
    let _guard = test_lock().lock().await;
    let (service, _store) = test_service().await;
    let session = service
        .create_session(&unique("attachments"), Some("runtime-doc"))
        .await
        .unwrap();
    let traversal = service
        .create_attachment(AttachmentWrite {
            session_id: &session.session_id,
            attachment_id: None,
            filename: "../escape.txt",
            mime_type: "text/plain",
            size_bytes: 5,
            content_base64: None,
            object_key: None,
        })
        .await;
    assert!(traversal.is_err());
    let invalid = service
        .create_attachment(AttachmentWrite {
            session_id: &session.session_id,
            attachment_id: None,
            filename: "note.txt",
            mime_type: "text/plain",
            size_bytes: 3,
            content_base64: Some("not-base64!!!"),
            object_key: None,
        })
        .await;
    assert!(invalid.is_err());

    let filename = format!("{}.txt", unique("attachment"));
    let encoded = base64::engine::general_purpose::STANDARD.encode(b"hello attachment");
    let meta = service
        .create_attachment(AttachmentWrite {
            session_id: &session.session_id,
            attachment_id: None,
            filename: &filename,
            mime_type: "text/plain",
            size_bytes: 16,
            content_base64: Some(&encoded),
            object_key: None,
        })
        .await
        .unwrap();
    assert_eq!(meta.filename, filename);
    let attachments_dir = std::env::temp_dir()
        .join(".quire-runtime")
        .join("attachments");
    let stored = std::fs::read_dir(&attachments_dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(&filename))
        })
        .expect("attachment file");
    assert_eq!(std::fs::read(&stored).unwrap(), b"hello attachment");
    let _ = std::fs::remove_file(stored);
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn attachments_presign_inline_and_local_mount() {
    let _guard = test_lock().lock().await;
    let mut config = test_config("http://127.0.0.1:1");
    config.sandbox = quire_runtime::sandbox::SandboxConfig {
        provider: "local".into(),
        ..Default::default()
    };
    let (service, store) = test_service_with_config(config).await;
    let app = quire_runtime::http::router(service.clone());
    let session = service
        .create_session(&unique("inline-attachment"), Some("runtime-doc"))
        .await
        .unwrap();

    let (status, presign) = post_json(
        &app,
        &format!("/api/sessions/{}/attachments/presign", session.session_id),
        json!({
            "filename": "data.csv",
            "mime_type": "text/csv",
            "size_bytes": 12
        }),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(presign["mode"], "inline");

    let encoded = base64::engine::general_purpose::STANDARD.encode(b"a,b\n1,2\n3,4");
    let (status, meta) = post_json(
        &app,
        &format!("/api/sessions/{}/attachments", session.session_id),
        json!({
            "filename": "data.csv",
            "mime_type": "text/csv",
            "size_bytes": 12,
            "content_base64": encoded
        }),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let attachment_id = meta["attachment_id"].as_str().unwrap().to_string();
    let stored = store
        .get_session_attachment(&session.session_id, &attachment_id)
        .await
        .unwrap();
    assert_eq!(stored.storage_provider, "local");
    assert!(Path::new(&stored.storage_path).is_file());

    let (model_base_url, server) = start_scripted_model(vec![
        tool_call_stream_for(
            "sandbox_mount_attachment",
            "call_local_mount",
            &json!({"attachment_id": attachment_id}),
        ),
        text_stream(),
    ])
    .await;
    install_scripted_agent(
        &service,
        &model_base_url,
        &["sandbox_mount_attachment".to_string()],
    )
    .await;
    let service = with_local_rig_provider(service);
    service.boot().await.unwrap();

    service
        .post_message(
            &session.session_id,
            &SendMessageRequest {
                client_msg_id: unique("inline-mount-message"),
                content: "Mount the attachment into the sandbox.".into(),
                comment_refs: Vec::new(),
                attachment_ids: vec![attachment_id.clone()],
            },
        )
        .await
        .unwrap();
    wait_for_idle(&store, &session.session_id).await;

    let traces = store.list_run_traces(&session.session_id).await.unwrap();
    let tool = find_trace_tool(&traces, "sandbox_mount_attachment")
        .expect("local mount tool call must be traced");
    assert_eq!(tool.status, "completed");
    let output = tool.output_preview.as_deref().unwrap_or_default();
    assert!(output.contains(&format!("/workspace/attachments/{attachment_id}/data.csv")));
    assert!(!output.contains("http"));

    service.delete_session(&session.session_id).await.unwrap();
    service.kb().delete_agent("main").await.unwrap();
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attachments_oss_presign_and_object_key_scope() {
    let _guard = test_lock().lock().await;
    let mut config = test_config("http://127.0.0.1:1");
    config.oss = test_oss_config();
    let (service, store) = test_service_with_config(config).await;
    let app = quire_runtime::http::router(service.clone());
    let session = service
        .create_session(&unique("oss-attachment"), Some("runtime-doc"))
        .await
        .unwrap();

    let (status, presign) = post_json(
        &app,
        &format!("/api/sessions/{}/attachments/presign", session.session_id),
        json!({
            "filename": "data.csv",
            "mime_type": "text/csv",
            "size_bytes": 12
        }),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(presign["mode"], "oss");
    let attachment_id = presign["attachment_id"].as_str().unwrap().to_string();
    let object_key = presign["object_key"].as_str().unwrap().to_string();
    let upload_url = presign["upload_url"].as_str().unwrap_or_default();
    assert!(
        upload_url.starts_with(
            "https://agentcore-ws-test.oss-cn-hangzhou.aliyuncs.com/quire/attachments/"
        )
    );
    assert!(upload_url.contains("x-oss-signature="));
    assert!(upload_url.contains("x-oss-credential="));
    assert!(!upload_url.contains("test-secret-value"));
    assert_eq!(
        presign["headers"]["Content-Type"].as_str(),
        Some("text/csv")
    );
    assert!(object_key.starts_with(&format!("quire/attachments/{}/", session.session_id)));

    let (wrong_status, _) = post_json(
        &app,
        &format!("/api/sessions/{}/attachments", session.session_id),
        json!({
            "attachment_id": attachment_id,
            "object_key": "quire/attachments/another_session/att_x/data.csv",
            "filename": "data.csv",
            "mime_type": "text/csv",
            "size_bytes": 12
        }),
    )
    .await;
    assert_eq!(wrong_status, axum::http::StatusCode::BAD_REQUEST);

    let (status, meta) = post_json(
        &app,
        &format!("/api/sessions/{}/attachments", session.session_id),
        json!({
            "attachment_id": attachment_id,
            "object_key": object_key,
            "filename": "data.csv",
            "mime_type": "text/csv",
            "size_bytes": 12
        }),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(meta["attachment_id"], attachment_id);
    let stored = store
        .get_session_attachment(&session.session_id, &attachment_id)
        .await
        .unwrap();
    assert_eq!(stored.storage_provider, "oss");
    assert_eq!(stored.storage_path, object_key);

    let (plain_service, _plain_store) = test_service().await;
    let plain_app = quire_runtime::http::router(plain_service.clone());
    let plain_session = plain_service
        .create_session(&unique("plain-oss-rejected"), Some("runtime-doc"))
        .await
        .unwrap();
    let (status, _) = post_json(
        &plain_app,
        &format!("/api/sessions/{}/attachments", plain_session.session_id),
        json!({
            "object_key": format!(
                "quire/attachments/{}/att_fake/data.csv",
                plain_session.session_id
            ),
            "filename": "data.csv",
            "mime_type": "text/csv",
            "size_bytes": 12
        }),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);

    plain_service
        .delete_session(&plain_session.session_id)
        .await
        .unwrap();
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sandbox_mount_attachment_scopes_sessions_and_redacts_traces() {
    let _guard = test_lock().lock().await;
    let mut config = test_config("http://127.0.0.1:1");
    config.oss = test_oss_config();
    config.sandbox = quire_runtime::sandbox::SandboxConfig {
        provider: "local".into(),
        ..Default::default()
    };
    let (service, store) = test_service_with_config(config).await;
    let app = quire_runtime::http::router(service.clone());
    let owner = service
        .create_session(&unique("oss-mount-owner"), Some("runtime-doc"))
        .await
        .unwrap();
    let intruder = service
        .create_session(&unique("oss-mount-intruder"), Some("runtime-doc"))
        .await
        .unwrap();

    let (status, presign) = post_json(
        &app,
        &format!("/api/sessions/{}/attachments/presign", owner.session_id),
        json!({
            "filename": "data.csv",
            "mime_type": "text/csv",
            "size_bytes": 12
        }),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    let attachment_id = presign["attachment_id"].as_str().unwrap().to_string();
    let object_key = presign["object_key"].as_str().unwrap().to_string();
    let (status, _) = post_json(
        &app,
        &format!("/api/sessions/{}/attachments", owner.session_id),
        json!({
            "attachment_id": attachment_id,
            "object_key": object_key,
            "filename": "data.csv",
            "mime_type": "text/csv",
            "size_bytes": 12
        }),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);

    let (model_base_url, server) = start_scripted_model(vec![
        tool_call_stream_for(
            "sandbox_mount_attachment",
            "call_owner",
            &json!({"attachment_id": attachment_id}),
        ),
        text_stream(),
        tool_call_stream_for(
            "sandbox_mount_attachment",
            "call_intruder",
            &json!({"attachment_id": attachment_id}),
        ),
        text_stream(),
    ])
    .await;
    install_scripted_agent(
        &service,
        &model_base_url,
        &["sandbox_mount_attachment".to_string()],
    )
    .await;
    let service = with_local_rig_provider(service);
    service.boot().await.unwrap();

    service
        .post_message(
            &owner.session_id,
            &SendMessageRequest {
                client_msg_id: unique("owner-mount-message"),
                content: "Mount my attachment.".into(),
                comment_refs: Vec::new(),
                attachment_ids: vec![attachment_id.clone()],
            },
        )
        .await
        .unwrap();
    wait_for_idle(&store, &owner.session_id).await;
    let owner_traces = store.list_run_traces(&owner.session_id).await.unwrap();
    let owner_tool = find_trace_tool(&owner_traces, "sandbox_mount_attachment")
        .expect("owner mount must be traced");
    assert_eq!(owner_tool.status, "completed");
    assert!(
        owner_tool
            .output_preview
            .as_deref()
            .unwrap_or_default()
            .contains(&format!("/workspace/attachments/{attachment_id}/data.csv"))
    );
    let owner_trace_text = serde_json::to_string(&owner_traces).unwrap();
    assert!(!owner_trace_text.contains("x-oss-signature"));
    assert!(!owner_trace_text.contains("x-oss-credential"));
    assert!(!owner_trace_text.contains("agentcore-ws-test"));
    assert!(!owner_trace_text.contains("https://"));

    service
        .post_message(
            &intruder.session_id,
            &SendMessageRequest {
                client_msg_id: unique("intruder-mount-message"),
                content: "Mount another session's attachment.".into(),
                comment_refs: Vec::new(),
                attachment_ids: vec![attachment_id.clone()],
            },
        )
        .await
        .unwrap();
    wait_for_idle(&store, &intruder.session_id).await;
    let intruder_traces = store.list_run_traces(&intruder.session_id).await.unwrap();
    let intruder_tool = find_trace_tool(&intruder_traces, "sandbox_mount_attachment")
        .expect("cross-session mount attempt must be traced");
    assert_eq!(intruder_tool.status, "failed");
    let intruder_output = intruder_tool.output_preview.as_deref().unwrap_or_default();
    assert!(intruder_output.contains("not found"));
    assert!(!intruder_output.contains("x-oss-signature"));
    let intruder_trace_text = serde_json::to_string(&intruder_traces).unwrap();
    assert!(!intruder_trace_text.contains("x-oss-signature"));
    assert!(!intruder_trace_text.contains("agentcore-ws-test"));
    assert!(!intruder_trace_text.contains("https://"));

    let detail = store
        .get_tool_call(&intruder.session_id, &intruder_tool.tool_call_id)
        .await
        .unwrap();
    let serialized = serde_json::to_string(&detail).unwrap();
    assert!(!serialized.contains("x-oss-signature"));
    assert!(!serialized.contains("agentcore-ws-test"));

    service.delete_session(&intruder.session_id).await.unwrap();
    service.delete_session(&owner.session_id).await.unwrap();
    service.kb().delete_agent("main").await.unwrap();
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_bearer_auth_protects_api_routes() {
    let _guard = test_lock().lock().await;
    let (service, _store) = test_service().await;
    let provider_id = unique("auth-provider");
    service
        .models()
        .upsert_provider(ProviderInput {
            provider_id: provider_id.clone(),
            name: "Auth Provider".into(),
            protocol: ProviderProtocol::OpenaiCompatible,
            base_url: "https://example.invalid".into(),
            api_key_ref: "sk-plaintext-must-not-leak".into(),
            enabled: true,
        })
        .await
        .unwrap();
    let cleanup_service = service.clone();
    let app = quire_runtime::http::router(service);
    let unauthorized = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/sessions")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), axum::http::StatusCode::UNAUTHORIZED);
    let health = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/health")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(health.status(), axum::http::StatusCode::OK);
    let wrong_token = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/sessions")
                .header("authorization", "Bearer wrong-token")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wrong_token.status(), axum::http::StatusCode::UNAUTHORIZED);
    let authorized = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/sessions")
                .header("authorization", "Bearer runtime-test-token")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(authorized.status(), axum::http::StatusCode::OK);
    let providers = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/models/providers")
                .header("authorization", "Bearer runtime-test-token")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(providers.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(providers.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(!text.contains("sk-plaintext-must-not-leak"));
    assert!(text.contains("enc:***"));
    cleanup_service
        .models()
        .delete_provider(&provider_id)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn models_registry_imports_catalog_and_routes_context_limit() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let provider_id = unique("provider");
    let provider = service
        .models()
        .upsert_provider(ProviderInput {
            provider_id: provider_id.clone(),
            name: "DeepSeek".into(),
            protocol: ProviderProtocol::OpenaiCompatible,
            base_url: "https://api.deepseek.com".into(),
            api_key_ref: "sk-raw-secret-value".into(),
            enabled: true,
        })
        .await
        .unwrap();
    assert!(provider.api_key_masked.starts_with("enc:"));
    assert!(!provider.api_key_masked.contains("sk-raw-secret-value"));

    let catalog = serde_json::json!({
        "deepseek": {
            "id": "deepseek",
            "name": "DeepSeek",
            "models": {
                "deepseek-chat": {
                    "id": "deepseek-chat",
                    "name": "DeepSeek Chat",
                    "reasoning": true,
                    "tool_call": true,
                    "attachment": false,
                    "limit": {"context": 64000, "output": 4096},
                    "cost": {"input": 0.14, "output": 0.28, "cache_read": 0.014}
                },
                "deepseek-coder": {
                    "id": "deepseek-coder",
                    "name": "DeepSeek Coder",
                    "tool_call": true
                }
            }
        }
    });
    let imported = service
        .models()
        .import_models_dev(ModelsDevImportRequest {
            provider_id: provider_id.clone(),
            provider_filter: Some("deepseek".into()),
            model_ids: None,
            payload: Some(catalog),
            url: None,
        })
        .await
        .unwrap();
    assert_eq!(imported.imported, 2);
    let route = imported
        .models
        .iter()
        .find(|model| model.model_id == "deepseek-chat")
        .unwrap();
    assert_eq!(route.context_limit, 64000);
    assert_eq!(route.max_output_tokens, Some(4096));
    assert_eq!(route.cost_cache_read, Some(0.014));
    let fallback = imported
        .models
        .iter()
        .find(|model| model.model_id == "deepseek-coder")
        .unwrap();
    assert_eq!(fallback.context_limit, 128_000);
    assert_eq!(fallback.max_output_tokens, None);
    let resolved = service.models().resolve(&route.id).await.unwrap();
    assert_eq!(resolved.protocol, ProviderProtocol::OpenaiCompatible);
    assert_eq!(resolved.base_url, "https://api.deepseek.com");
    assert_eq!(resolved.context_limit, 64000);
    assert_eq!(resolved.model_id, "deepseek-chat");

    service
        .kb()
        .upsert_agent("main", "", &route.id, "Runtime test agent", &[], true, true)
        .await
        .unwrap();
    let session = service
        .create_session(&unique("model-route"), Some("runtime-doc"))
        .await
        .unwrap();
    let request = SendMessageRequest {
        client_msg_id: unique("model-msg"),
        content: "Use the registered model".into(),
        comment_refs: Vec::new(),
        attachment_ids: Vec::new(),
    };
    service
        .post_message(&session.session_id, &request)
        .await
        .unwrap();
    wait_for_idle(&store, &session.session_id).await;
    let detail = service.session_detail(&session.session_id).await.unwrap();
    assert_eq!(detail.summary.context_usage.context_limit, 64000);
    service.delete_session(&session.session_id).await.unwrap();
    service.kb().delete_agent("main").await.unwrap();
    service
        .models()
        .delete_provider(&provider_id)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn knowledge_update_is_injected_once_and_synced_version_advances() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let session = service
        .create_session(&unique("prompt-once"), Some("runtime-doc"))
        .await
        .unwrap();
    let initial_version = service.kb().current_version().await.unwrap();
    store
        .set_pinned_prompt(&session.session_id, "FROZEN PREFIX", initial_version)
        .await
        .unwrap();
    let document_id = unique("kb-update");
    service
        .kb()
        .upsert_document(&document_id, "Update", "New guidance")
        .await
        .unwrap();
    let bumped_version = service.kb().current_version().await.unwrap();
    let first = service
        .resume_context(&session.session_id, "First resume")
        .await
        .unwrap();
    assert_eq!(first.pinned_system_prompt, "FROZEN PREFIX");
    assert!(
        first
            .messages
            .iter()
            .any(|message| message.content.starts_with("<knowledge_update>"))
    );

    let request = SendMessageRequest {
        client_msg_id: unique("kb-msg"),
        content: "Run one turn".into(),
        comment_refs: Vec::new(),
        attachment_ids: Vec::new(),
    };
    service
        .post_message(&session.session_id, &request)
        .await
        .unwrap();
    wait_for_idle(&store, &session.session_id).await;
    let (pinned, synced) = store.pinned_prompt(&session.session_id).await.unwrap();
    assert_eq!(pinned.as_deref(), Some("FROZEN PREFIX"));
    assert_eq!(synced, bumped_version);

    let second = service
        .resume_context(&session.session_id, "Second resume")
        .await
        .unwrap();
    assert_eq!(second.pinned_system_prompt, "FROZEN PREFIX");
    assert!(
        !second
            .messages
            .iter()
            .any(|message| message.content.starts_with("<knowledge_update>"))
    );
    service.kb().delete_document(&document_id).await.unwrap();
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compaction_reuses_prefix_and_preserves_pending_state() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let (provider, requests) = RecordingStreamProvider::new(vec![ModelStreamEvent::TextDelta(
        compaction_summary_text().into(),
    )]);
    let service = service.with_model_stream_provider(Arc::new(provider));
    let session = service
        .create_session(&unique("compact"), Some("runtime-doc"))
        .await
        .unwrap();
    let old_prompt = "OLD PINNED PROMPT";
    store
        .set_pinned_prompt(&session.session_id, old_prompt, 0)
        .await
        .unwrap();
    store
        .insert_context_message(
            &session.session_id,
            MessageRole::User,
            "old question",
            "compact:old-user",
            None,
        )
        .await
        .unwrap();
    store
        .insert_context_message(
            &session.session_id,
            MessageRole::Assistant,
            "old answer",
            "compact:old-assistant",
            None,
        )
        .await
        .unwrap();
    store
        .enqueue_pending_message(
            &session.session_id,
            "compact:queue",
            "queued follow-up",
            &[],
            PendingQueueSource::User,
            None,
            &[],
        )
        .await
        .unwrap();
    store
        .create_question(&session.session_id, "run_pending", None, &question_items())
        .await
        .unwrap();
    store
        .update_context_usage(
            &session.session_id,
            &ContextUsage {
                context_used: 180_000,
                context_limit: 200_000,
                ..ContextUsage::default()
            },
        )
        .await
        .unwrap();

    let detail = service.compact(&session.session_id).await.unwrap();
    assert!(detail.summary.compacted_at.is_some());
    assert!(
        detail
            .pending_queue
            .iter()
            .any(|item| item.content == "queued follow-up")
    );
    assert!(detail.pending_question.is_some());
    let marker = detail
        .messages
        .iter()
        .find(|message| message.role == MessageRole::Compaction)
        .expect("compaction marker");
    assert!(marker.content.contains("handoff summary"));

    let captured = requests.lock().await;
    assert_eq!(captured.len(), 1);
    assert_eq!(captured[0].system_prompt, old_prompt);
    assert!(captured[0].disable_tools);
    assert_eq!(
        captured[0]
            .history
            .iter()
            .find(|message| message.content == "old answer")
            .map(|message| message.content.as_str()),
        Some("old answer")
    );
    drop(captured);

    let (pinned, synced) = store.pinned_prompt(&session.session_id).await.unwrap();
    assert_ne!(pinned.as_deref(), Some(old_prompt));
    assert_eq!(synced, service.kb().current_version().await.unwrap());
    let usage = detail.summary.context_usage.clone();
    assert!(usage.context_used > 0 && usage.context_used < 180_000);
    let events = store
        .list_events(&session.session_id, 0, 500)
        .await
        .unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event.kind, SessionEventKind::CompactionStarted))
    );
    assert!(events.iter().any(|event| matches!(
        event.kind,
        SessionEventKind::CompactionFinished { success: true }
    )));

    let again = service.compact(&session.session_id).await.unwrap();
    assert_eq!(requests.lock().await.len(), 1);
    assert_eq!(again.messages.len(), detail.messages.len());

    store
        .insert_context_message(
            &session.session_id,
            MessageRole::User,
            "after anchor",
            "compact:after",
            None,
        )
        .await
        .unwrap();
    let resumed = service
        .resume_context(&session.session_id, "next")
        .await
        .unwrap();
    assert!(
        resumed
            .messages
            .first()
            .is_some_and(|message| message.content.contains("handoff summary"))
    );
    assert!(
        resumed
            .messages
            .iter()
            .any(|message| message.content == "after anchor")
    );
    assert!(
        !resumed
            .messages
            .iter()
            .any(|message| message.content == "old question")
    );
    assert!(
        resumed
            .messages
            .iter()
            .any(|message| message.content.contains("<report_state>"))
    );
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compaction_failure_leaves_anchor_prompt_and_usage_untouched() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let service = service.with_model_stream_provider(Arc::new(FailingStreamProvider));
    let session = service
        .create_session(&unique("compact-fail"), Some("runtime-doc"))
        .await
        .unwrap();
    store
        .set_pinned_prompt(&session.session_id, "OLD PINNED PROMPT", 0)
        .await
        .unwrap();
    store
        .insert_context_message(
            &session.session_id,
            MessageRole::User,
            "keep me",
            "compact-fail:user",
            None,
        )
        .await
        .unwrap();
    store
        .update_context_usage(
            &session.session_id,
            &ContextUsage {
                context_used: 190_000,
                context_limit: 200_000,
                ..ContextUsage::default()
            },
        )
        .await
        .unwrap();
    let before = service.session_detail(&session.session_id).await.unwrap();

    assert!(service.compact(&session.session_id).await.is_err());
    let after = service.session_detail(&session.session_id).await.unwrap();
    let state = store.compaction_state(&session.session_id).await.unwrap();
    assert!(state.summary.is_none());
    assert!(state.up_to_message_id.is_none());
    assert!(!after.summary.compacting);
    assert_eq!(
        store
            .pinned_prompt(&session.session_id)
            .await
            .unwrap()
            .0
            .as_deref(),
        Some("OLD PINNED PROMPT")
    );
    assert_eq!(
        after.summary.context_usage.context_used,
        before.summary.context_usage.context_used
    );
    assert_eq!(after.messages.len(), before.messages.len());
    assert!(
        !after
            .messages
            .iter()
            .any(|message| message.role == MessageRole::Compaction)
    );
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compaction_conflicts_while_a_run_is_active() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let session = service
        .create_session(&unique("compact-running"), None)
        .await
        .unwrap();
    let run = store
        .create_run(&session.session_id, None, "main", "stub/echo")
        .await
        .unwrap();

    let error = service.compact(&session.session_id).await.unwrap_err();
    assert!(matches!(error, RuntimeError::CompactionConflict(_)));
    store
        .finish_run(&run.run_id, "completed", None)
        .await
        .unwrap();
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compacting_state_survives_a_client_refresh_until_finished() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let provider = Arc::new(BlockingSummaryProvider::new());
    let release = Arc::clone(&provider);
    let service = service.with_model_stream_provider(provider);
    let session = service
        .create_session(&unique("compact-state"), Some("runtime-doc"))
        .await
        .unwrap();
    store
        .insert_context_message(
            &session.session_id,
            MessageRole::User,
            "summarize this",
            "compact-state:user",
            None,
        )
        .await
        .unwrap();

    let compact_session = session.session_id.clone();
    let task_service = service.clone();
    let handle = tokio::spawn(async move { task_service.compact(&compact_session).await });

    let mut saw_compacting = false;
    for _ in 0..100 {
        let detail = store.get_session_detail(&session.session_id).await.unwrap();
        if detail.summary.compacting {
            saw_compacting = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(saw_compacting, "compaction state must be persisted");

    release.release();
    let result = handle.await.unwrap();
    assert!(result.is_ok());
    let detail = store.get_session_detail(&session.session_id).await.unwrap();
    assert!(!detail.summary.compacting);
    assert!(
        detail
            .messages
            .iter()
            .any(|message| message.role == MessageRole::Compaction)
    );
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn automatic_compaction_excludes_the_new_user_message_from_the_anchor() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let (provider, requests) = RecordingStreamProvider::with_turns(vec![
        vec![ModelStreamEvent::TextDelta(
            compaction_summary_text().into(),
        )],
        vec![ModelStreamEvent::TextDelta("normal reply".into())],
    ]);
    let service = service.with_model_stream_provider(Arc::new(provider));
    let session = service
        .create_session(&unique("compact-auto"), Some("runtime-doc"))
        .await
        .unwrap();
    store
        .set_pinned_prompt(&session.session_id, "OLD PINNED PROMPT", 0)
        .await
        .unwrap();
    store
        .insert_context_message(
            &session.session_id,
            MessageRole::User,
            "old question",
            "compact-auto:user",
            None,
        )
        .await
        .unwrap();
    let anchor = store
        .insert_context_message(
            &session.session_id,
            MessageRole::Assistant,
            "old answer",
            "compact-auto:assistant",
            None,
        )
        .await
        .unwrap();
    store
        .update_context_usage(
            &session.session_id,
            &ContextUsage {
                context_used: 180_000,
                context_limit: 200_000,
                ..ContextUsage::default()
            },
        )
        .await
        .unwrap();

    let new_content = "new request after auto compaction";
    service
        .post_message(
            &session.session_id,
            &SendMessageRequest {
                client_msg_id: unique("compact-auto:new"),
                content: new_content.into(),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    wait_for_idle(&store, &session.session_id).await;

    let captured = requests.lock().await;
    assert!(captured.len() >= 2);
    assert!(captured[0].disable_tools);
    assert!(
        !captured[0]
            .history
            .iter()
            .any(|message| message.content == new_content)
    );
    assert_eq!(captured[1].user_message, new_content);
    drop(captured);

    let state = store.compaction_state(&session.session_id).await.unwrap();
    assert_eq!(
        state.up_to_message_id.as_deref(),
        Some(anchor.message_id.as_str())
    );
    let detail = service.session_detail(&session.session_id).await.unwrap();
    let new_message = detail
        .messages
        .iter()
        .find(|message| message.role == MessageRole::User && message.content == new_content)
        .expect("new user message");
    assert_ne!(
        state.up_to_message_id.as_deref(),
        Some(new_message.message_id.as_str())
    );
    let marker_index = detail
        .messages
        .iter()
        .position(|message| message.role == MessageRole::Compaction)
        .expect("compaction marker");
    let new_index = detail
        .messages
        .iter()
        .position(|message| message.message_id == new_message.message_id)
        .expect("new message position");
    assert!(marker_index < new_index);
    assert!(detail.summary.context_usage.context_used < 180_000);
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streaming_tokens_are_coalesced_and_flushed_before_tool_and_turn_events() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let mut events = Vec::new();
    for _ in 0..30 {
        events.push(ModelStreamEvent::ThinkingDelta("t".into()));
    }
    for _ in 0..50 {
        events.push(ModelStreamEvent::TextDelta("x".into()));
    }
    events.push(ModelStreamEvent::ToolCallStarted {
        tool_call_id: "tc_stream".into(),
        tool_name: "describe_task".into(),
        input_preview: "{\"title\":\"Review\"}".into(),
        truncated: false,
        input: json!({"title": "Review"}),
    });
    let service =
        service.with_model_stream_provider(Arc::new(MockStreamProvider::new(vec![events])));
    let session = service
        .create_session(&unique("coalesce"), Some("runtime-doc"))
        .await
        .unwrap();
    let request = SendMessageRequest {
        client_msg_id: unique("coalesce-msg"),
        content: "Stream fifty fragments".into(),
        comment_refs: Vec::new(),
        attachment_ids: Vec::new(),
    };
    service
        .post_message(&session.session_id, &request)
        .await
        .unwrap();
    wait_for_idle(&store, &session.session_id).await;

    let events = store
        .list_events(&session.session_id, 0, 500)
        .await
        .unwrap();
    let thinking_count = events
        .iter()
        .filter(|event| matches!(event.kind, SessionEventKind::ThinkingDelta { .. }))
        .count();
    let text_deltas: Vec<&str> = events
        .iter()
        .filter_map(|event| match &event.kind {
            SessionEventKind::TextDelta { delta } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    let text_count = text_deltas.len();
    assert_eq!(thinking_count, 1);
    assert_eq!(text_count, 1);
    assert_eq!(text_deltas.concat(), "x".repeat(50));
    assert!(thinking_count + text_count < 80);

    let last_text_seq = events
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                SessionEventKind::ThinkingDelta { .. } | SessionEventKind::TextDelta { .. }
            )
        })
        .map(|event| event.seq)
        .max()
        .unwrap();
    let tool_seq = events
        .iter()
        .find(|event| matches!(event.kind, SessionEventKind::ToolCallStarted { .. }))
        .map(|event| event.seq)
        .unwrap();
    let finished_seq = events
        .iter()
        .find(|event| matches!(event.kind, SessionEventKind::TurnFinished))
        .map(|event| event.seq)
        .unwrap();
    assert!(last_text_seq < tool_seq, "text must flush before tool call");
    assert!(
        tool_seq < finished_seq,
        "tool event must precede turn finish"
    );
    let detail = service.session_detail(&session.session_id).await.unwrap();
    assert_eq!(
        detail
            .messages
            .iter()
            .find(|message| message.role == quire_session_protocol::MessageRole::Assistant)
            .map(|message| message.content.as_str()),
        Some("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx")
    );
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn branch_edits_are_isolated_and_sessions_use_the_selected_branch() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let base_doc_id = unique("branch-base");
    service
        .kb()
        .upsert_document(&base_doc_id, "Branch base", "BASE DOCUMENT")
        .await
        .unwrap();
    let branch_name = unique("draft");
    let branch = service
        .kb()
        .create_branch(&branch_name, "Branch isolation test", None)
        .await
        .unwrap();
    assert_eq!(branch.status, KbBranchStatus::Draft);
    assert_eq!(branch.base_version, branch.head_version);

    let branch_doc_id = unique("branch-only");
    service
        .kb()
        .upsert_document_on(
            &branch_name,
            &branch_doc_id,
            "Branch-only document",
            "BRANCH ONLY BODY",
        )
        .await
        .unwrap();
    let active_docs = service.kb().list_documents().await.unwrap();
    assert!(
        !active_docs
            .iter()
            .any(|document| document.doc_id == branch_doc_id),
        "draft edits must not leak into the active branch"
    );
    let branch_docs = service
        .kb()
        .list_documents_on(Some(&branch_name))
        .await
        .unwrap();
    assert!(
        branch_docs
            .iter()
            .any(|document| document.doc_id == branch_doc_id
                && document.content == "BRANCH ONLY BODY")
    );

    let app = quire_runtime::http::router(service.clone());
    let (status, session) = http_json(
        &app,
        axum::http::Method::POST,
        "/api/sessions",
        "runtime-test-token",
        Some(json!({
            "title": unique("branch-session"),
            "doc_id": "runtime-doc",
            "kb_branch": branch_name
        })),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(session["kb_branch"], branch_name);
    let session_id = session["session_id"]
        .as_str()
        .expect("session id")
        .to_string();
    let context = service
        .resume_context(&session_id, "Read the selected branch")
        .await
        .unwrap();
    assert!(
        context.pinned_system_prompt.contains("BRANCH ONLY BODY"),
        "a session bound to the draft must read branch content"
    );
    let active_prompt = service.kb().system_prompt().await.unwrap();
    assert!(!active_prompt.contains("BRANCH ONLY BODY"));

    service.delete_session(&session_id).await.unwrap();
    remove_branch(&store, &branch_name).await;
    service.kb().delete_document(&base_doc_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disjoint_changes_auto_merge_on_promotion_and_invalidate_caches() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let doc_a = unique("merge-a");
    let doc_b = unique("merge-b");
    service
        .kb()
        .upsert_document(&doc_a, "Merge A", "A BASE")
        .await
        .unwrap();
    service
        .kb()
        .upsert_document(&doc_b, "Merge B", "B BASE")
        .await
        .unwrap();
    let branch_name = unique("merge-draft");
    service
        .kb()
        .create_branch(&branch_name, "Disjoint merge test", None)
        .await
        .unwrap();
    service
        .kb()
        .upsert_document_on(&branch_name, &doc_a, "Merge A", "A BRANCH")
        .await
        .unwrap();
    service
        .kb()
        .upsert_document(&doc_b, "Merge B", "B ACTIVE")
        .await
        .unwrap();

    let mut invalidation = store.bus().subscribe_kb_invalidation().await.unwrap();
    let before = service.kb().system_prompt().await.unwrap();
    assert!(before.contains("B ACTIVE"));
    assert!(!before.contains("A BRANCH"));

    let report = service.kb().promote_branch(&branch_name).await.unwrap();
    assert_eq!(report.mode, KbPromoteMode::Merged);
    assert!(report.version > report.previous_version);
    assert!(report.applied_items >= 1);
    let documents = service.kb().list_documents().await.unwrap();
    let merged_a = documents
        .iter()
        .find(|document| document.doc_id == doc_a)
        .unwrap();
    let merged_b = documents
        .iter()
        .find(|document| document.doc_id == doc_b)
        .unwrap();
    assert_eq!(merged_a.content, "A BRANCH");
    assert_eq!(merged_b.content, "B ACTIVE");

    let prompt = service.kb().system_prompt().await.unwrap();
    assert!(prompt.contains("A BRANCH"));
    assert!(prompt.contains("B ACTIVE"));
    let published = tokio::time::timeout(Duration::from_secs(5), invalidation.next_invalidation())
        .await
        .unwrap()
        .expect("promotion must publish a KB invalidation");
    assert_eq!(published.version, report.version);

    let branch = service
        .kb()
        .get_branch(&branch_name)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(branch.status, KbBranchStatus::Archived);
    let snapshots: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::bigint FROM kb_snapshots WHERE version = $1")
            .bind(i64::try_from(report.snapshot_version).unwrap())
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(snapshots, 1);

    service.kb().delete_document(&doc_a).await.unwrap();
    service.kb().delete_document(&doc_b).await.unwrap();
    remove_branch(&store, &branch_name).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conflicting_promotion_returns_three_way_content_then_rollback_restores_previous_release() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let doc_id = unique("conflict-doc");
    service
        .kb()
        .upsert_document(&doc_id, "Conflict doc", "BASE")
        .await
        .unwrap();
    let branch_name = unique("conflict-draft");
    let branch = service
        .kb()
        .create_branch(&branch_name, "Conflict test", None)
        .await
        .unwrap();
    let base_version = branch.base_version;
    service
        .kb()
        .upsert_document_on(&branch_name, &doc_id, "Conflict doc", "BRANCH")
        .await
        .unwrap();
    service
        .kb()
        .upsert_document(&doc_id, "Conflict doc", "ACTIVE")
        .await
        .unwrap();

    let conflict = match service.kb().promote_branch(&branch_name).await {
        Err(RuntimeError::KbConflict(report)) => report,
        other => panic!("expected a structured conflict, got {other:?}"),
    };
    assert_eq!(conflict.branch, branch_name);
    assert_eq!(conflict.base_version, base_version);
    assert!(!conflict.conflicts.is_empty());
    let entry = conflict
        .conflicts
        .iter()
        .find(|entry| entry.item_kind == KbItemKind::Doc && entry.item_key == doc_id)
        .expect("conflict for edited document");
    let content = |value: &Option<Value>| {
        value
            .as_ref()
            .and_then(|value| value.get("content"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    assert_eq!(content(&entry.base_content).as_deref(), Some("BASE"));
    assert_eq!(content(&entry.active_content).as_deref(), Some("ACTIVE"));
    assert_eq!(content(&entry.branch_content).as_deref(), Some("BRANCH"));

    service
        .kb()
        .upsert_document_on(&branch_name, &doc_id, "Conflict doc", "RESOLVED")
        .await
        .unwrap();
    let promoted = service.kb().promote_branch(&branch_name).await.unwrap();
    assert_eq!(promoted.mode, KbPromoteMode::Merged);
    let active_documents = service.kb().list_documents().await.unwrap();
    assert_eq!(
        active_documents
            .iter()
            .find(|document| document.doc_id == doc_id)
            .map(|document| document.content.as_str()),
        Some("RESOLVED")
    );

    let rollback = service.kb().rollback_active(None).await.unwrap();
    assert_eq!(rollback.previous_version, promoted.version);
    assert!(rollback.restored_from_version >= base_version);
    assert!(rollback.restored_from_version < promoted.version);
    assert!(rollback.version > promoted.version);
    let rolled_back = service.kb().list_documents().await.unwrap();
    assert_eq!(
        rolled_back
            .iter()
            .find(|document| document.doc_id == doc_id)
            .map(|document| document.content.as_str()),
        Some("ACTIVE"),
        "one-click rollback restores the active state immediately before promotion"
    );

    let rollback_to_base = service
        .kb()
        .rollback_active(Some(base_version))
        .await
        .unwrap();
    assert_eq!(rollback_to_base.restored_from_version, base_version);
    let restored_base = service.kb().list_documents().await.unwrap();
    assert_eq!(
        restored_base
            .iter()
            .find(|document| document.doc_id == doc_id)
            .map(|document| document.content.as_str()),
        Some("BASE")
    );

    service.kb().delete_document(&doc_id).await.unwrap();
    remove_branch(&store, &branch_name).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_mcp_exposes_branch_model_and_session_tools_end_to_end() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    service
        .kb()
        .upsert_agent(
            "main",
            "",
            "stub/echo",
            "Runtime admin MCP test",
            &[],
            true,
            true,
        )
        .await
        .unwrap();
    let branch_name = unique("mcp-draft");
    let app = quire_runtime::http::router(service.clone());

    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "runtime-admin-test", "version": "1.0"}
        }
    });
    let (status, _, _) = mcp_request(&app, "wrong-token", None, "2025-11-25", &initialize).await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
    let (status, _) = http_json(
        &app,
        axum::http::Method::GET,
        "/api/kb/branches",
        "wrong-token",
        None,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::UNAUTHORIZED);
    let (status, branches) = http_json(
        &app,
        axum::http::Method::GET,
        "/api/kb/branches",
        "runtime-test-token",
        None,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert!(
        branches
            .as_array()
            .is_some_and(|branches| branches.iter().any(|branch| branch["status"] == "active"))
    );

    let (status, created) = http_json(
        &app,
        axum::http::Method::POST,
        "/api/kb/branches",
        "runtime-test-token",
        Some(json!({
            "name": branch_name,
            "description": "Admin MCP end-to-end branch"
        })),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(created["status"], "draft");
    let base_version = created["base_version"].as_u64().expect("base version");
    let (status, diff) = http_json(
        &app,
        axum::http::Method::GET,
        &format!("/api/kb/branches/{branch_name}/diff"),
        "runtime-test-token",
        None,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(diff["branch"], branch_name);

    let client = AdminMcpClient::connect(app, "runtime-test-token").await;
    let tool_names = client.tool_names().await;
    for expected in [
        "kb_list_branches",
        "kb_create_branch",
        "kb_diff_branch",
        "kb_promote_branch",
        "kb_rollback_active",
        "kb_list_items",
        "kb_read_item",
        "kb_upsert_doc",
        "kb_upsert_agent",
        "kb_upsert_skill",
        "models_list_providers",
        "models_upsert_provider",
        "models_list",
        "models_import_models_dev",
        "mcp_list_servers",
        "mcp_upsert_server",
        "mcp_sync_tools",
        "mcp_list_tools",
        "mcp_set_tool_enabled",
        "sessions_list",
        "sessions_inspect",
    ] {
        assert!(
            tool_names.iter().any(|name| name == expected),
            "missing admin MCP tool {expected}"
        );
    }

    let doc_id = unique("mcp-doc");
    let document = client
        .call_tool(
            "kb_upsert_doc",
            json!({
                "branch": branch_name,
                "doc_id": doc_id,
                "name": "MCP branch document",
                "content": "MCP BRANCH CONTENT"
            }),
        )
        .await;
    assert_eq!(document["document"]["content"], "MCP BRANCH CONTENT");
    let items = client
        .call_tool(
            "kb_list_items",
            json!({"branch": branch_name, "item_kind": "doc"}),
        )
        .await;
    assert!(
        items["items"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["item_key"] == doc_id))
    );
    let read = client
        .call_tool(
            "kb_read_item",
            json!({"branch": branch_name, "item_kind": "doc", "item_key": doc_id}),
        )
        .await;
    assert_eq!(read["found"], true);
    assert_eq!(read["content"]["content"], "MCP BRANCH CONTENT");
    let branch_diff = client
        .call_tool("kb_diff_branch", json!({"name": branch_name}))
        .await;
    assert!(
        branch_diff["diff"]["changes"]
            .as_array()
            .is_some_and(|changes| changes
                .iter()
                .any(|change| change["item_key"] == doc_id && change["status"] == "added"))
    );

    let session = service
        .create_session_on_branch(
            &unique("mcp-session"),
            Some("runtime-doc"),
            Some(&branch_name),
        )
        .await
        .unwrap();
    service
        .post_message(
            &session.session_id,
            &SendMessageRequest {
                client_msg_id: unique("mcp-message"),
                content: "Run the admin MCP observation check".into(),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    wait_for_idle(&store, &session.session_id).await;
    let sessions = client.call_tool("sessions_list", json!({})).await;
    assert!(sessions["sessions"].as_array().is_some_and(|sessions| {
        sessions
            .iter()
            .any(|entry| entry["session_id"] == session.session_id)
    }));
    let inspected = client
        .call_tool(
            "sessions_inspect",
            json!({"session_id": session.session_id}),
        )
        .await;
    assert_eq!(inspected["session"]["kb_branch"], branch_name);
    assert_eq!(inspected["latest_run"]["status"], "completed");
    assert_eq!(inspected["context_usage"]["context_limit"], 200_000);

    let provider_id = unique("mcp-provider");
    let provider = client
        .call_tool(
            "models_upsert_provider",
            json!({
                "provider_id": provider_id,
                "name": "MCP Provider",
                "protocol": "openai_compatible",
                "base_url": "https://example.invalid",
                "api_key_ref": "sk-admin-mcp-secret"
            }),
        )
        .await;
    assert!(!provider.to_string().contains("sk-admin-mcp-secret"));
    assert!(
        provider["provider"]["api_key_masked"]
            .as_str()
            .is_some_and(|masked| masked.starts_with("enc:"))
    );
    let providers = client.call_tool("models_list_providers", json!({})).await;
    assert!(providers["providers"].as_array().is_some_and(|providers| {
        providers
            .iter()
            .any(|provider| provider["provider_id"] == provider_id)
    }));
    let imported = client
        .call_tool(
            "models_import_models_dev",
            json!({
                "provider_id": provider_id,
                "provider_filter": "e2e",
                "payload": {
                    "e2e": {
                        "id": "e2e",
                        "name": "E2E Catalog",
                        "models": {
                            "e2e-chat": {
                                "id": "e2e-chat",
                                "name": "E2E Chat",
                                "limit": {"context": 32000, "output": 2048}
                            }
                        }
                    }
                }
            }),
        )
        .await;
    assert_eq!(imported["imported"], 1);
    let route_id = format!("{provider_id}/e2e-chat");
    let models = client.call_tool("models_list", json!({})).await;
    assert!(
        models["models"]
            .as_array()
            .is_some_and(|models| models.iter().any(|model| model["id"] == route_id))
    );

    let promoted = client
        .call_tool("kb_promote_branch", json!({"name": branch_name}))
        .await;
    assert_eq!(promoted["report"]["mode"], "fast_forward");
    let active_documents = service.kb().list_documents().await.unwrap();
    assert!(
        active_documents
            .iter()
            .any(|document| document.doc_id == doc_id && document.content == "MCP BRANCH CONTENT")
    );
    let listed = client.call_tool("kb_list_branches", json!({})).await;
    assert!(listed["branches"].as_array().is_some_and(|branches| {
        branches
            .iter()
            .any(|branch| branch["name"] == branch_name && branch["status"] == "archived")
    }));
    let rolled_back = client
        .call_tool("kb_rollback_active", json!({"version": base_version}))
        .await;
    assert_eq!(rolled_back["report"]["restored_from_version"], base_version);
    let rolled = service.kb().list_documents().await.unwrap();
    assert!(!rolled.iter().any(|document| document.doc_id == doc_id));

    service.delete_session(&session.session_id).await.unwrap();
    service.kb().delete_agent("main").await.unwrap();
    service
        .models()
        .delete_provider(&provider_id)
        .await
        .unwrap();
    service.kb().delete_document(&doc_id).await.unwrap();
    remove_branch(&store, &branch_name).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rig_turn_context_and_task_events_reach_the_persisted_session_contract() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let branch = unique("rig-context");
    service
        .kb()
        .create_branch(&branch, "Rig context test", None)
        .await
        .unwrap();
    service
        .kb()
        .upsert_agent_on(
            &branch,
            "main",
            "",
            "stub/echo",
            "BRANCH AGENT PROMPT",
            &["describe_task".into(), "apply_operations".into()],
            true,
            true,
        )
        .await
        .unwrap();
    let session = service
        .create_session_on_branch(
            &unique("rig-context-session"),
            Some("runtime-doc"),
            Some(&branch),
        )
        .await
        .unwrap();
    let initial_version = service.kb().current_version().await.unwrap();
    store
        .set_pinned_prompt(&session.session_id, "PINNED FOR RIG", initial_version)
        .await
        .unwrap();
    let bump_doc = unique("rig-context-kb");
    service
        .kb()
        .upsert_document(&bump_doc, "Rig context", "KB advanced for resume")
        .await
        .unwrap();

    let tool_call_id = unique("tc-context");
    let tool_input = json!({
        "doc_id": "runtime-doc",
        "expected_revision": 1,
        "summary": "context test",
        "ops": []
    });
    let events = vec![
        ModelStreamEvent::TextDelta("main answer".into()),
        ModelStreamEvent::TaskFocus {
            title: "Review chapter".into(),
            detail: Some("checking citations".into()),
            section_id: Some("ch1".into()),
        },
        ModelStreamEvent::SubagentStarted {
            subagent_run_id: "run_sub_ctx".into(),
            agent_name: "reviewer".into(),
            display_name: None,
            task: "review chapter".into(),
            section_id: Some("ch1".into()),
        },
        ModelStreamEvent::SubagentEvent {
            subagent_run_id: "run_sub_ctx".into(),
            agent_name: "reviewer".into(),
            event: Box::new(ModelStreamEvent::TextDelta("subagent answer".into())),
        },
        ModelStreamEvent::ToolCallStarted {
            tool_call_id: tool_call_id.clone(),
            tool_name: "apply_operations".into(),
            input_preview: tool_input.to_string(),
            truncated: false,
            input: tool_input.clone(),
        },
        ModelStreamEvent::ToolCallFinished {
            tool_call_id: tool_call_id.clone(),
            tool_name: "apply_operations".into(),
            status: "completed".into(),
            output_preview: "{\"committed\":true}".into(),
            truncated: false,
            duration_ms: 12,
            input: tool_input,
            output: json!({"committed": true}),
            error: None,
        },
        ModelStreamEvent::SubagentFinished {
            subagent_run_id: "run_sub_ctx".into(),
            status: "completed".into(),
            summary: "subagent answer".into(),
        },
        ModelStreamEvent::Usage(ContextUsage {
            input_tokens: 111,
            output_tokens: 22,
            cached_tokens: 33,
            context_used: 133,
            context_limit: 123_456,
        }),
    ];
    let (provider, requests) = RecordingStreamProvider::new(events);
    let service = service.with_model_stream_provider(Arc::new(provider));
    let started = service
        .post_message(
            &session.session_id,
            &SendMessageRequest {
                client_msg_id: unique("rig-context-message"),
                content: "Use the full rig context".into(),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    let run_id = match started {
        PostMessageOutcome::Started { run_id, .. } => run_id,
        other => panic!("expected started message, got {other:?}"),
    };
    wait_for_idle(&store, &session.session_id).await;

    let requests = requests.lock().await;
    let request = requests.first().expect("recorded model turn request");
    assert_eq!(request.doc_id.as_deref(), Some("runtime-doc"));
    assert_eq!(request.kb_branch.as_deref(), Some(branch.as_str()));
    assert_eq!(request.model_route, "stub/echo");
    assert_eq!(
        request.allowed_tools,
        vec!["describe_task".to_string(), "apply_operations".to_string()]
    );
    assert!(
        request
            .history
            .iter()
            .any(|message| message.role == MessageRole::System
                && message.content.starts_with("<knowledge_update>"))
    );
    assert_eq!(
        request
            .history
            .last()
            .map(|message| (message.role, message.content.as_str())),
        Some((MessageRole::User, "Use the full rig context"))
    );
    drop(requests);

    let detail = service.session_detail(&session.session_id).await.unwrap();
    assert_eq!(detail.summary.context_usage.input_tokens, 111);
    assert_eq!(detail.summary.context_usage.output_tokens, 22);
    assert_eq!(detail.summary.context_usage.cached_tokens, 33);
    assert_eq!(detail.summary.context_usage.context_used, 133);
    assert_eq!(detail.summary.context_usage.context_limit, 123_456);
    let tool_call = service
        .get_tool_call(&session.session_id, &tool_call_id)
        .await
        .unwrap();
    assert_eq!(tool_call.input["summary"], "context test");
    assert_eq!(tool_call.output["committed"], true);
    assert_eq!(tool_call.status, "completed");

    let events = store
        .list_events(&session.session_id, 0, 200)
        .await
        .unwrap();
    let seq = |predicate: &dyn Fn(&quire_session_protocol::SessionEvent) -> bool| {
        events
            .iter()
            .find(|event| predicate(event))
            .map(|event| event.seq)
            .expect("expected event")
    };
    let task_focus = seq(&|event| matches!(event.kind, SessionEventKind::TaskFocus { .. }));
    let subagent_started =
        seq(&|event| matches!(event.kind, SessionEventKind::SubagentStarted { .. }));
    let subagent_text = seq(&|event| {
        event.agent_id == "reviewer" && matches!(event.kind, SessionEventKind::TextDelta { .. })
    });
    let tool_started = seq(&|event| matches!(event.kind, SessionEventKind::ToolCallStarted { .. }));
    let tool_finished =
        seq(&|event| matches!(event.kind, SessionEventKind::ToolCallFinished { .. }));
    let subagent_finished =
        seq(&|event| matches!(event.kind, SessionEventKind::SubagentFinished { .. }));
    let context_usage = seq(&|event| matches!(event.kind, SessionEventKind::ContextUsage { .. }));
    let turn_finished = seq(&|event| matches!(event.kind, SessionEventKind::TurnFinished));
    assert!(task_focus < subagent_started);
    assert!(subagent_started < subagent_text);
    assert!(subagent_text < tool_started);
    assert!(tool_started < tool_finished);
    assert!(tool_finished < subagent_finished);
    assert!(subagent_finished < context_usage);
    assert!(context_usage < turn_finished);
    let subagent_event = events
        .iter()
        .find(|event| event.agent_id == "reviewer")
        .expect("subagent event");
    assert_eq!(
        subagent_event.parent_run_id.as_deref(),
        Some(run_id.as_str())
    );

    service.delete_session(&session.session_id).await.unwrap();
    remove_branch(&store, &branch).await;
    service.kb().delete_document(&bump_doc).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ask_user_suspends_until_answers_resume_the_run() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let (provider, requests) = RecordingStreamProvider::with_turns(vec![
        vec![ModelStreamEvent::QuestionnaireRequested {
            tool_call_id: Some("tc_ask".into()),
            questions: question_items(),
        }],
        vec![
            ModelStreamEvent::TextDelta("resumed after answer".into()),
            ModelStreamEvent::Usage(ContextUsage {
                input_tokens: 10,
                output_tokens: 5,
                cached_tokens: 2,
                context_used: 15,
                context_limit: 64_000,
            }),
        ],
    ]);
    let service = service.with_model_stream_provider(Arc::new(provider));
    let session = service
        .create_session(&unique("ask-user-answer"), Some("runtime-doc"))
        .await
        .unwrap();
    service
        .post_message(
            &session.session_id,
            &SendMessageRequest {
                client_msg_id: unique("ask-user-answer-start"),
                content: "Start the questionnaire".into(),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    wait_for_idle(&store, &session.session_id).await;

    let waiting = service.session_detail(&session.session_id).await.unwrap();
    assert_eq!(
        waiting.summary.active_run_status,
        ActiveRunStatus::AwaitingInput
    );
    let pending = waiting.pending_question.clone().expect("pending question");
    assert_eq!(pending.questions.len(), 1);
    assert_eq!(pending.tool_call_id.as_deref(), Some("tc_ask"));

    let app = quire_runtime::http::router(service.clone());
    let (status, body) = http_json(
        &app,
        axum::http::Method::POST,
        &format!(
            "/api/sessions/{}/questions/{}/answer",
            session.session_id, pending.question_id
        ),
        "runtime-test-token",
        Some(json!({
            "answers": [{
                "question_id": "scope",
                "selected_option_ids": ["q3"],
                "skipped": false
            }],
            "skipped": false
        })),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(body["pending_question"], Value::Null);
    wait_for_idle(&store, &session.session_id).await;

    let resumed = service.session_detail(&session.session_id).await.unwrap();
    assert_eq!(resumed.summary.active_run_status, ActiveRunStatus::Idle);
    assert!(resumed.pending_question.is_none());
    let resolved = resumed
        .resolved_questions
        .first()
        .expect("resolved question should be hydrated after reload");
    assert_eq!(resolved.status, "answered");
    assert_eq!(resolved.question.tool_call_id.as_deref(), Some("tc_ask"));
    assert_eq!(
        resolved
            .answers
            .first()
            .and_then(|answer| answer.selected_option_ids.first())
            .map(String::as_str),
        Some("q3")
    );
    assert!(resumed.messages.iter().any(|message| {
        message.role == MessageRole::Assistant && message.content.contains("resumed after answer")
    }));
    let question_status: String =
        sqlx::query_scalar("SELECT status FROM agent_questions WHERE question_id = $1")
            .bind(&pending.question_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(question_status, "answered");
    let events = store
        .list_events(&session.session_id, 0, 200)
        .await
        .unwrap();
    assert!(events.iter().any(|event| matches!(
        &event.kind,
        SessionEventKind::QuestionnaireAnswered { skipped_all: false, answers, .. }
            if answers.first().is_some_and(|answer| answer.selected_option_ids == vec!["q3"])
    )));
    assert!(
        events
            .iter()
            .any(|event| matches!(&event.kind, SessionEventKind::QuestionnaireRequested { .. }))
    );
    let requests = requests.lock().await;
    assert_eq!(requests.len(), 2);
    assert!(requests.get(1).is_some_and(|request| {
        request
            .history
            .iter()
            .any(|message| message.content.contains("q3"))
    }));
    drop(requests);
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ask_user_skip_marks_skipped_and_continues() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let (provider, requests) = RecordingStreamProvider::with_turns(vec![
        vec![ModelStreamEvent::QuestionnaireRequested {
            tool_call_id: Some("tc_skip".into()),
            questions: question_items(),
        }],
        vec![
            ModelStreamEvent::TextDelta("continued after skip".into()),
            ModelStreamEvent::Usage(ContextUsage {
                input_tokens: 8,
                output_tokens: 4,
                cached_tokens: 0,
                context_used: 12,
                context_limit: 64_000,
            }),
        ],
    ]);
    let service = service.with_model_stream_provider(Arc::new(provider));
    let session = service
        .create_session(&unique("ask-user-skip"), Some("runtime-doc"))
        .await
        .unwrap();
    service
        .post_message(
            &session.session_id,
            &SendMessageRequest {
                client_msg_id: unique("ask-user-skip-start"),
                content: "Ask me something optional".into(),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    wait_for_idle(&store, &session.session_id).await;
    let pending = service
        .session_detail(&session.session_id)
        .await
        .unwrap()
        .pending_question
        .expect("pending question");

    let app = quire_runtime::http::router(service.clone());
    let (status, _) = http_json(
        &app,
        axum::http::Method::POST,
        &format!(
            "/api/sessions/{}/questions/{}/answer",
            session.session_id, pending.question_id
        ),
        "runtime-test-token",
        Some(json!({"answers": [], "skipped": true})),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    wait_for_idle(&store, &session.session_id).await;

    let detail = service.session_detail(&session.session_id).await.unwrap();
    assert!(detail.pending_question.is_none());
    assert!(detail.messages.iter().any(|message| {
        message.role == MessageRole::Assistant && message.content.contains("continued after skip")
    }));
    let question_status: String =
        sqlx::query_scalar("SELECT status FROM agent_questions WHERE question_id = $1")
            .bind(&pending.question_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(question_status, "skipped");
    let events = store
        .list_events(&session.session_id, 0, 200)
        .await
        .unwrap();
    assert!(events.iter().any(|event| matches!(
        &event.kind,
        SessionEventKind::QuestionnaireAnswered {
            skipped_all: true,
            ..
        }
    )));
    let requests = requests.lock().await;
    assert_eq!(requests.len(), 2);
    assert!(requests.get(1).is_some_and(|request| {
        request
            .history
            .iter()
            .any(|message| message.content.contains("skipped questionnaire"))
    }));
    drop(requests);
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn new_message_dismisses_pending_question_and_starts_a_new_turn() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let (provider, requests) = RecordingStreamProvider::with_turns(vec![
        vec![ModelStreamEvent::QuestionnaireRequested {
            tool_call_id: Some("tc_dismiss".into()),
            questions: question_items(),
        }],
        vec![ModelStreamEvent::TextDelta("reply to new message".into())],
    ]);
    let service = service.with_model_stream_provider(Arc::new(provider));
    let session = service
        .create_session(&unique("ask-user-dismiss"), Some("runtime-doc"))
        .await
        .unwrap();
    service
        .post_message(
            &session.session_id,
            &SendMessageRequest {
                client_msg_id: unique("ask-user-dismiss-start"),
                content: "Ask a question first".into(),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    wait_for_idle(&store, &session.session_id).await;
    let pending = service
        .session_detail(&session.session_id)
        .await
        .unwrap()
        .pending_question
        .expect("pending question");

    let app = quire_runtime::http::router(service.clone());
    let (status, body) = http_json(
        &app,
        axum::http::Method::POST,
        &format!("/api/sessions/{}/messages", session.session_id),
        "runtime-test-token",
        Some(json!({
            "client_msg_id": unique("ask-user-dismiss-next"),
            "content": "Ignore the card and answer this instead"
        })),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert!(body["run_id"].as_str().is_some());
    wait_for_idle(&store, &session.session_id).await;

    let detail = service.session_detail(&session.session_id).await.unwrap();
    assert!(detail.pending_question.is_none());
    assert!(detail.messages.iter().any(|message| {
        message.role == MessageRole::Assistant && message.content.contains("reply to new message")
    }));
    let question_status: String =
        sqlx::query_scalar("SELECT status FROM agent_questions WHERE question_id = $1")
            .bind(&pending.question_id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(question_status, "dismissed");
    let events = store
        .list_events(&session.session_id, 0, 200)
        .await
        .unwrap();
    assert!(events.iter().any(|event| matches!(
        &event.kind,
        SessionEventKind::QuestionnaireDismissed { reason, .. }
            if reason == "superseded_by_user_message"
    )));
    let started_runs: Vec<&str> = events
        .iter()
        .filter(|event| matches!(&event.kind, SessionEventKind::TurnStarted))
        .map(|event| event.run_id.as_str())
        .collect();
    assert_eq!(started_runs.len(), 2);
    assert_ne!(started_runs.first(), started_runs.get(1));
    let requests = requests.lock().await;
    assert_eq!(requests.len(), 2);
    assert!(requests.get(1).is_some_and(|request| {
        request
            .history
            .iter()
            .any(|message| message.content.contains("Ignore the card"))
    }));
    drop(requests);
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn seed_registers_quire_report_mcp_without_overwriting_admin_state() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    service.mcp().delete_server("quire-report").await.unwrap();
    service.seed().await.unwrap();
    let server = service
        .mcp()
        .get_server("quire-report")
        .await
        .unwrap()
        .expect("seeded quire-report server");
    assert!(server.enabled);
    assert_eq!(server.url, service.config().quire_base_url);
    assert_eq!(
        server.auth_token_masked.as_deref(),
        Some("env:QUIRE_RW_TOKEN")
    );

    service
        .mcp()
        .patch_server(
            "quire-report",
            McpServerPatch {
                enabled: Some(false),
                ..McpServerPatch::default()
            },
        )
        .await
        .unwrap();
    service.seed().await.unwrap();
    let disabled = service
        .mcp()
        .get_server("quire-report")
        .await
        .unwrap()
        .expect("seeded quire-report server");
    assert!(
        !disabled.enabled,
        "seed must not overwrite an admin-disabled server"
    );
    service.mcp().delete_server("quire-report").await.unwrap();
    let _ = sqlx::query("DELETE FROM mcp_servers WHERE server_id = 'quire-report'")
        .execute(store.pool())
        .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_tool_switch_filters_allow_listed_tools_and_invalidates_cache() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    for server in service.mcp().list_servers().await.unwrap() {
        service
            .mcp()
            .delete_server(&server.server_id)
            .await
            .unwrap();
    }
    let server_id = unique("mcp-registry");
    service
        .mcp()
        .upsert_server(McpServerInput {
            server_id: server_id.clone(),
            name: "Registry test".into(),
            url: "https://example.invalid/mcp".into(),
            auth_token_ref: None,
            enabled: true,
        })
        .await
        .unwrap();
    for tool_name in ["danger_tool", "safe_tool"] {
        sqlx::query(
            r#"INSERT INTO mcp_tools (server_id, tool_name, description, enabled)
               VALUES ($1, $2, $3, TRUE)"#,
        )
        .bind(&server_id)
        .bind(tool_name)
        .bind(format!("{tool_name} description"))
        .execute(store.pool())
        .await
        .unwrap();
    }

    let allowed = vec!["danger_tool".to_string()];
    let before = service.mcp().tools_for_agent(&allowed).await.unwrap();
    assert_eq!(before.len(), 1);
    assert!(
        before[0]
            .tool_names
            .iter()
            .any(|name| name == "danger_tool")
    );

    let mut invalidation = store.bus().subscribe_mcp_invalidation().await.unwrap();
    service
        .mcp()
        .set_tool_enabled(&server_id, "danger_tool", false)
        .await
        .unwrap();
    let published = tokio::time::timeout(Duration::from_secs(5), invalidation.next_invalidation())
        .await
        .unwrap()
        .expect("tool toggle must publish MCP invalidation");
    assert!(published.version > 0);
    assert!(
        service
            .mcp()
            .tools_for_agent(&allowed)
            .await
            .unwrap()
            .is_empty(),
        "globally disabled tools must be filtered even when agent-allowed"
    );
    let wildcard = service
        .mcp()
        .tools_for_agent(&["*".to_string()])
        .await
        .unwrap();
    let wildcard_server_ids: Vec<&str> = wildcard
        .iter()
        .map(|binding| binding.server_id.as_str())
        .collect();
    assert_eq!(
        wildcard.len(),
        1,
        "unexpected servers: {wildcard_server_ids:?}"
    );
    assert_eq!(wildcard[0].tool_names, vec!["safe_tool".to_string()]);

    let prefix_wildcard = service
        .mcp()
        .tools_for_agent(&["safe_*".to_string()])
        .await
        .unwrap();
    assert_eq!(prefix_wildcard[0].tool_names, vec!["safe_tool".to_string()]);

    let scoped_wildcard = format!("{server_id}:safe_*");
    let scoped = service
        .mcp()
        .tools_for_agent(&[scoped_wildcard])
        .await
        .unwrap();
    assert_eq!(scoped[0].tool_names, vec!["safe_tool".to_string()]);

    let wrong_server = service
        .mcp()
        .tools_for_agent(&["other-server:safe_*".to_string()])
        .await
        .unwrap();
    assert!(
        wrong_server.is_empty(),
        "scoped wildcards must not cross server boundaries"
    );

    service
        .mcp()
        .set_tool_enabled(&server_id, "danger_tool", true)
        .await
        .unwrap();
    let restored = service
        .mcp()
        .tools_for_agent(&["*".to_string()])
        .await
        .unwrap();
    assert_eq!(
        restored[0].tool_names,
        vec!["danger_tool".to_string(), "safe_tool".to_string()]
    );
    service.mcp().delete_server(&server_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_auth_token_is_masked_in_http_and_admin_receipts() {
    let _guard = test_lock().lock().await;
    let (service, _store) = test_service().await;
    let server_id = unique("mcp-secret");
    let secret = "sk-mcp-registry-secret-value";
    let app = quire_runtime::http::router(service.clone());
    let (status, created) = http_json(
        &app,
        axum::http::Method::POST,
        "/api/mcp/servers",
        "runtime-test-token",
        Some(json!({
            "server_id": server_id,
            "name": "Secret registry test",
            "url": "https://example.invalid/mcp",
            "auth_token_ref": secret,
            "enabled": true
        })),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::OK);
    assert_eq!(created["auth_token_masked"], "enc:***");
    assert!(!created.to_string().contains(secret));

    let client = AdminMcpClient::connect(app, "runtime-test-token").await;
    let listed = client.call_tool("mcp_list_servers", json!({})).await;
    assert!(!listed.to_string().contains(secret));
    assert!(
        listed["servers"]
            .as_array()
            .is_some_and(|servers| servers.iter().any(|server| {
                server["server_id"] == server_id && server["auth_token_masked"] == "enc:***"
            }))
    );
    service.mcp().delete_server(&server_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_sync_discovers_tools_and_preserves_admin_switches() {
    let _guard = test_lock().lock().await;
    let (service, _store) = test_service().await;
    let app = axum::Router::new().route("/mcp", axum::routing::post(mock_mcp_tools_list));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let server_id = unique("mcp-sync");
    service
        .mcp()
        .upsert_server(McpServerInput {
            server_id: server_id.clone(),
            name: "Sync test".into(),
            url: format!("http://{address}"),
            auth_token_ref: None,
            enabled: true,
        })
        .await
        .unwrap();
    let first = service.mcp().sync_server_tools(&server_id).await.unwrap();
    assert_eq!(first.discovered, 2);
    assert_eq!(first.created, 2);
    assert_eq!(first.updated, 0);
    service
        .mcp()
        .set_tool_enabled(&server_id, "alpha_tool", false)
        .await
        .unwrap();

    let second = service.mcp().sync_server_tools(&server_id).await.unwrap();
    assert_eq!(second.discovered, 2);
    assert_eq!(second.created, 0);
    assert_eq!(second.updated, 2);
    let alpha = second
        .tools
        .iter()
        .find(|tool| tool.tool_name == "alpha_tool")
        .expect("alpha tool");
    assert!(
        !alpha.enabled,
        "sync must preserve a previously disabled tool"
    );
    let beta = second
        .tools
        .iter()
        .find(|tool| tool.tool_name == "beta_tool")
        .expect("beta tool");
    assert!(beta.enabled);
    service.mcp().delete_server(&server_id).await.unwrap();
    server.abort();
}

fn proxy_comment_thread(status: CommentStatus) -> CommentThread {
    let now = chrono::Utc::now();
    CommentThread {
        thread_id: "ct_proxy".into(),
        doc_id: DocId::parse("report-test").unwrap(),
        block_id: BlockId::parse("b_proxy").unwrap(),
        end_block_id: None,
        anchor_blocks: Vec::new(),
        section_id: None,
        anchor_snapshot: Some("Proxy anchor".into()),
        quote: None,
        end_quote: None,
        status,
        created_rev: Revision::new(1).unwrap(),
        addressed_rev: (status == CommentStatus::Addressed).then(|| Revision::new(2).unwrap()),
        anchor_detached: false,
        messages: vec![CommentMessage {
            message_id: "cm_proxy".into(),
            thread_id: "ct_proxy".into(),
            author_type: CommentAuthorType::Human,
            author: "reviewer".into(),
            body: "Please tighten this paragraph.".into(),
            created_at: now,
        }],
        created_at: now,
        updated_at: now,
    }
}

async fn mock_comment_create() -> axum::Json<CommentThread> {
    axum::Json(proxy_comment_thread(CommentStatus::Open))
}

async fn mock_comment_reply() -> axum::Json<CommentThread> {
    axum::Json(proxy_comment_thread(CommentStatus::Addressed))
}

fn proxy_comment_reply_thread(status: CommentStatus, body: &str) -> CommentThread {
    let mut thread = proxy_comment_thread(status);
    let now = chrono::Utc::now();
    thread.updated_at = now;
    thread.messages.push(CommentMessage {
        message_id: "cm_proxy_reply".into(),
        thread_id: thread.thread_id.clone(),
        author_type: CommentAuthorType::Human,
        author: "reviewer".into(),
        body: body.into(),
        created_at: now,
    });
    thread
}

async fn mock_comment_reply_addressed() -> axum::Json<CommentThread> {
    axum::Json(proxy_comment_reply_thread(
        CommentStatus::Addressed,
        "Use a different chart",
    ))
}

async fn mock_comment_reply_open() -> axum::Json<CommentThread> {
    axum::Json(proxy_comment_reply_thread(
        CommentStatus::Open,
        "More context while the agent is working",
    ))
}

async fn mock_comment_patch() -> axum::Json<CommentThread> {
    axum::Json(proxy_comment_thread(CommentStatus::Archived))
}

async fn mock_comment_delete(
    axum::extract::State(deleted): axum::extract::State<Arc<Mutex<Vec<String>>>>,
    axum::extract::Path((doc_id, thread_id)): axum::extract::Path<(String, String)>,
) -> axum::http::StatusCode {
    deleted.lock().await.push(format!("{doc_id}:{thread_id}"));
    axum::http::StatusCode::NO_CONTENT
}

#[tokio::test]
async fn session_comment_proxy_requires_a_bound_report_and_forwards_rw_token() {
    let _guard = test_lock().lock().await;
    let upstream = axum::Router::new()
        .route(
            "/api/docs/{doc_id}/comments",
            axum::routing::post(mock_comment_create),
        )
        .route(
            "/api/docs/{doc_id}/comments/{thread_id}/replies",
            axum::routing::post(mock_comment_reply),
        )
        .route(
            "/api/docs/{doc_id}/comments/{thread_id}",
            axum::routing::patch(mock_comment_patch),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, upstream).await.unwrap();
    });

    let (service, _store) = test_service_with_quire_base(&format!("http://{address}")).await;
    let session = service
        .create_session(&unique("comment-proxy"), Some("report-test"))
        .await
        .unwrap();
    let unbound = service
        .create_session(&unique("comment-proxy-unbound"), None)
        .await
        .unwrap();
    let app = quire_runtime::http::router(service);

    let created = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/api/sessions/{}/comments", session.session_id))
                .header("authorization", "Bearer runtime-test-token")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({"block_id": "b_proxy", "body": "Tighten"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(created.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(created.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let thread: CommentThread = serde_json::from_slice(&body).unwrap();
    assert_eq!(thread.status, CommentStatus::Open);

    let replied = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/sessions/{}/comments/ct_proxy/replies",
                    session.session_id
                ))
                .header("authorization", "Bearer runtime-test-token")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(json!({"body": "Done"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replied.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(replied.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let thread: CommentThread = serde_json::from_slice(&body).unwrap();
    assert_eq!(thread.status, CommentStatus::Addressed);

    let archived = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("PATCH")
                .uri(format!(
                    "/api/sessions/{}/comments/ct_proxy",
                    session.session_id
                ))
                .header("authorization", "Bearer runtime-test-token")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({"status": "archived"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(archived.status(), axum::http::StatusCode::OK);

    let unbound_response = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/api/sessions/{}/comments", unbound.session_id))
                .header("authorization", "Bearer runtime-test-token")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({"block_id": "b_proxy", "body": "Tighten"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        unbound_response.status(),
        axum::http::StatusCode::BAD_REQUEST
    );
    let body = axum::body::to_bytes(unbound_response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert!(
        String::from_utf8_lossy(&body).contains("not bound to a report"),
        "unbound session error must explain the missing report binding"
    );
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tool_boundary_injects_pending_message_into_same_run() {
    let _guard = test_lock().lock().await;
    let mock_state = Arc::new(MockChatState::default());
    let server_state = Arc::clone(&mock_state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            axum::Router::new()
                .route("/v1/responses", axum::routing::post(mock_chat_completions))
                .with_state(server_state),
        )
        .await
        .unwrap();
    });

    let (service, store) = test_service().await;
    let provider_id = unique("mock-model-provider");
    service
        .models()
        .upsert_provider(ProviderInput {
            provider_id: provider_id.clone(),
            name: "Mock OpenAI".into(),
            protocol: ProviderProtocol::OpenaiCompatible,
            base_url: format!("http://{address}/v1"),
            api_key_ref: "test-key".into(),
            enabled: true,
        })
        .await
        .unwrap();
    let route_id = unique("mock-model-route");
    service
        .models()
        .upsert_model(ModelInput {
            id: route_id.clone(),
            provider_id: provider_id.clone(),
            model_id: "mock-model".into(),
            display_name: "Mock Model".into(),
            context_limit: 32_000,
            max_output_tokens: Some(1024),
            supports_reasoning: false,
            supports_tools: true,
            supports_vision: false,
            cost_input: None,
            cost_output: None,
            cost_cache_read: None,
            enabled: true,
        })
        .await
        .unwrap();
    service
        .kb()
        .upsert_agent(
            "main",
            "",
            &route_id,
            "Mock tool-boundary agent",
            &["set_session_title".to_string()],
            true,
            true,
        )
        .await
        .unwrap();
    let sandbox_provider = build_sandbox_provider(&service.config().sandbox).unwrap();
    let provider = RigModelStreamProvider::new(
        service.kb().clone(),
        Arc::new(service.models().clone()),
        Arc::new(service.config().clone()),
        service.mcp_registry(),
        sandbox_provider,
        service.pending_queue(),
    );
    let service = service.with_model_stream_provider(Arc::new(provider));
    service.boot().await.unwrap();
    let session = service
        .create_session(&unique("tool-boundary-queue"), None)
        .await
        .unwrap();

    let first = service
        .post_message(
            &session.session_id,
            &SendMessageRequest {
                client_msg_id: unique("tool-boundary-first"),
                content: "先更新标题，再继续回答。".into(),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(first, PostMessageOutcome::Started { .. }));
    let started =
        tokio::time::timeout(Duration::from_secs(10), mock_state.started.notified()).await;
    if started.is_err() {
        let events = store
            .list_events(&session.session_id, 0, 100)
            .await
            .unwrap();
        let detail = service.session_detail(&session.session_id).await.unwrap();
        panic!("mock model never received the first request: events={events:?} detail={detail:?}");
    }

    let queued = service
        .post_message(
            &session.session_id,
            &SendMessageRequest {
                client_msg_id: unique("tool-boundary-queued"),
                content: "排队指令：请补充风险提示。".into(),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(queued, PostMessageOutcome::Queued { .. }));
    mock_state.release.notify_one();

    let mut finished = false;
    for _ in 0..500 {
        let detail = service.session_detail(&session.session_id).await.unwrap();
        if detail.pending_queue.is_empty()
            && detail.summary.active_run_status == ActiveRunStatus::Idle
        {
            finished = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(finished, "tool-boundary turn did not finish");
    let detail = service.session_detail(&session.session_id).await.unwrap();
    assert!(detail.pending_queue.is_empty());

    let requests = mock_state.requests.lock().await;
    assert!(
        requests.len() >= 2,
        "expected a second LLM request after the tool result"
    );
    let second_messages = requests[1]["messages"]
        .as_array()
        .or_else(|| requests[1]["input"].as_array())
        .unwrap_or_else(|| panic!("second request messages missing: {}", requests[1]));
    let tool_index = second_messages
        .iter()
        .rposition(|message| message["type"] == "function_call_output")
        .unwrap_or_else(|| {
            panic!("second request must contain a tool result: {second_messages:?}")
        });
    let injected = second_messages
        .get(tool_index + 1)
        .expect("queued message must follow the tool result");
    assert_eq!(injected["role"], "user");
    assert_eq!(injected["type"], "message");
    assert!(
        injected["content"]
            .as_array()
            .is_some_and(|parts| parts.iter().any(|part| {
                part["text"]
                    .as_str()
                    .is_some_and(|text| text.contains("排队指令：请补充风险提示。"))
            })),
        "queued message missing from second request: {injected:?}"
    );

    let events = store
        .list_events(&session.session_id, 0, 1_000)
        .await
        .unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event.kind, SessionEventKind::PendingQueueUpdated { .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event.kind, SessionEventKind::UserMessageInjected { .. }))
    );

    let injected_message = detail
        .messages
        .iter()
        .find(|message| message.content == "排队指令：请补充风险提示。")
        .expect("injected user message must be persisted");
    let injected_run_id = injected_message
        .run_id
        .as_deref()
        .expect("inline message must belong to the active run");
    let traces = store.list_run_traces(&session.session_id).await.unwrap();
    let trace = traces
        .iter()
        .find(|trace| trace.run_id == injected_run_id)
        .expect("injected run trace");
    assert_eq!(trace.segments.len(), 3);
    let Some(RunTraceSegment::Execution { tools, .. }) = trace.segments.first() else {
        panic!(
            "tool execution must precede the injected message: {:?}",
            trace.segments
        )
    };
    assert!(
        tools
            .iter()
            .any(|tool| tool.tool_name == "set_session_title")
    );
    let Some(RunTraceSegment::UserMessage {
        message_id,
        content,
        ..
    }) = trace.segments.get(1)
    else {
        panic!("injected user message must follow the tool execution")
    };
    assert_eq!(message_id, &injected_message.message_id);
    assert_eq!(content, "排队指令：请补充风险提示。");
    let Some(RunTraceSegment::Text { text }) = trace.segments.get(2) else {
        panic!("the next model round must follow the injected message")
    };
    assert_eq!(text, "已完成排队消息。");

    service.delete_session(&session.session_id).await.unwrap();
    service.kb().delete_agent("main").await.unwrap();
    service
        .models()
        .delete_provider(&provider_id)
        .await
        .unwrap();
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn idle_completion_wakes_pending_queue_without_interval_wait() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let gate = GateStreamProvider::new();
    let service = service.with_model_stream_provider(Arc::new(gate.clone()));
    service.boot().await.unwrap();
    let session = service
        .create_session(&unique("queue-notify"), Some("runtime-doc"))
        .await
        .unwrap();

    let first = service
        .post_message(
            &session.session_id,
            &SendMessageRequest {
                client_msg_id: unique("queue-notify-first"),
                content: "First gated answer".into(),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(first, PostMessageOutcome::Started { .. }));
    wait_for_running(&store, &session.session_id).await;

    let queued = service
        .post_message(
            &session.session_id,
            &SendMessageRequest {
                client_msg_id: unique("queue-notify-second"),
                content: "Immediate follow-up".into(),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(queued, PostMessageOutcome::Queued { .. }));

    let started = Instant::now();
    gate.release();
    let mut second_turn_started = false;
    for _ in 0..300 {
        let detail = service.session_detail(&session.session_id).await.unwrap();
        let messages = store.list_messages(&session.session_id).await.unwrap();
        let has_follow_up = messages.iter().any(|message| {
            message.role == MessageRole::User && message.content == "Immediate follow-up"
        });
        if has_follow_up
            && detail.pending_queue.is_empty()
            && detail.summary.active_run_status == ActiveRunStatus::Idle
        {
            second_turn_started = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        second_turn_started,
        "queued follow-up did not start after completion"
    );
    assert!(
        started.elapsed() < Duration::from_millis(1_500),
        "queued follow-up waited for the 2s fallback poll: {:?}",
        started.elapsed()
    );
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ask_user_suspension_keeps_pending_queue_untouched() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let provider = QuestionnaireGateProvider::new();
    let service = service.with_model_stream_provider(Arc::new(provider.clone()));
    service.boot().await.unwrap();
    let session = service
        .create_session(&unique("queue-suspended"), Some("runtime-doc"))
        .await
        .unwrap();

    let first = service
        .post_message(
            &session.session_id,
            &SendMessageRequest {
                client_msg_id: unique("queue-suspended-first"),
                content: "Ask me a question".into(),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(first, PostMessageOutcome::Started { .. }));
    tokio::time::timeout(Duration::from_secs(10), provider.started.notified())
        .await
        .expect("questionnaire model never started");
    let queued = service
        .post_message(
            &session.session_id,
            &SendMessageRequest {
                client_msg_id: unique("queue-suspended-second"),
                content: "This must wait for the answer".into(),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(queued, PostMessageOutcome::Queued { .. }));
    provider.release.notify_one();

    let mut awaiting = false;
    for _ in 0..300 {
        let detail = service.session_detail(&session.session_id).await.unwrap();
        if detail.summary.active_run_status == ActiveRunStatus::AwaitingInput {
            awaiting = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(awaiting, "run did not enter AwaitingInput");
    tokio::time::sleep(Duration::from_millis(250)).await;

    let detail = service.session_detail(&session.session_id).await.unwrap();
    assert!(
        detail.pending_queue.len() == 1,
        "suspended queue must not be consumed: {:?}",
        detail.pending_queue
    );
    let messages = store.list_messages(&session.session_id).await.unwrap();
    assert!(
        !messages.iter().any(|message| {
            message.role == MessageRole::User && message.content == "This must wait for the answer"
        }),
        "queued message was consumed while awaiting user input"
    );
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn running_session_queues_messages_and_drains_after_turn() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let gate = GateStreamProvider::new();
    let service = service.with_model_stream_provider(Arc::new(gate.clone()));
    service.boot().await.unwrap();
    let session = service
        .create_session(&unique("queue-session"), Some("runtime-doc"))
        .await
        .unwrap();

    let first = service
        .post_message(
            &session.session_id,
            &SendMessageRequest {
                client_msg_id: unique("queue-first"),
                content: "First turn".into(),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(first, PostMessageOutcome::Started { .. }));
    wait_for_running(&store, &session.session_id).await;

    let second = service
        .post_message(
            &session.session_id,
            &SendMessageRequest {
                client_msg_id: unique("queue-second"),
                content: "Second turn".into(),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    let second_item = match second {
        PostMessageOutcome::Queued { item } => item,
        other => panic!("expected queued message, got {other:?}"),
    };
    assert_eq!(second_item.source, PendingQueueSource::User);

    let third = service
        .post_message(
            &session.session_id,
            &SendMessageRequest {
                client_msg_id: unique("queue-third"),
                content: "Third turn".into(),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    let third_item = match third {
        PostMessageOutcome::Queued { item } => item,
        other => panic!("expected second queued message, got {other:?}"),
    };
    assert_eq!(
        service
            .list_pending_queue(&session.session_id)
            .await
            .unwrap()
            .len(),
        2
    );

    let edited_ref = comment_ref("ct_queue_edit", "Edited queued comment");
    let patched = service
        .update_pending_message(
            &session.session_id,
            &second_item.queue_id,
            "Edited second turn",
            std::slice::from_ref(&edited_ref),
        )
        .await
        .unwrap();
    assert_eq!(patched.content, "Edited second turn");
    assert_eq!(patched.comment_refs, vec![edited_ref]);
    assert!(
        service
            .delete_pending_message(&session.session_id, &third_item.queue_id)
            .await
            .unwrap()
    );
    assert_eq!(
        service
            .list_pending_queue(&session.session_id)
            .await
            .unwrap()
            .len(),
        1
    );

    gate.release();
    let mut stable_idle = 0;
    for _ in 0..300 {
        let detail = service.session_detail(&session.session_id).await.unwrap();
        if detail.pending_queue.is_empty()
            && detail.summary.active_run_status == ActiveRunStatus::Idle
        {
            stable_idle += 1;
            if stable_idle >= 5 {
                break;
            }
        } else {
            stable_idle = 0;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let detail = service.session_detail(&session.session_id).await.unwrap();
    assert!(detail.pending_queue.is_empty());
    assert_eq!(detail.summary.active_run_status, ActiveRunStatus::Idle);
    let messages = store.list_messages(&session.session_id).await.unwrap();
    let user_contents: Vec<String> = messages
        .iter()
        .filter(|message| message.role == MessageRole::User)
        .map(|message| message.content.clone())
        .collect();
    assert!(user_contents.iter().any(|content| content == "First turn"));
    assert!(
        user_contents
            .iter()
            .any(|content| content == "Edited second turn")
    );
    assert!(!user_contents.iter().any(|content| content == "Third turn"));
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn comment_creation_auto_triggers_a_turn() {
    let _guard = test_lock().lock().await;
    let upstream = axum::Router::new().route(
        "/api/docs/{doc_id}/comments",
        axum::routing::post(mock_comment_create),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, upstream).await.unwrap();
    });

    let (service, store) = test_service_with_quire_base(&format!("http://{address}")).await;
    let session = service
        .create_session(&unique("comment-trigger"), Some("report-test"))
        .await
        .unwrap();
    let app = quire_runtime::http::router(service);
    let created = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/api/sessions/{}/comments", session.session_id))
                .header("authorization", "Bearer runtime-test-token")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({"block_id": "b_proxy", "body": "Tighten this paragraph"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(created.status(), axum::http::StatusCode::OK);
    wait_for_idle(&store, &session.session_id).await;
    let messages = store.list_messages(&session.session_id).await.unwrap();
    assert!(
        messages.iter().any(|message| {
            message.role == MessageRole::User
                && message.content.is_empty()
                && message.comment_refs.iter().any(|comment_ref| {
                    comment_ref.thread_id == "ct_proxy"
                        && comment_ref.comment == "Please tighten this paragraph."
                })
        }),
        "comment creation must auto-submit a structured comment reference"
    );
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn addressed_comment_reply_auto_triggers_agent() {
    let _guard = test_lock().lock().await;
    let upstream = axum::Router::new().route(
        "/api/docs/{doc_id}/comments/{thread_id}/replies",
        axum::routing::post(mock_comment_reply_addressed),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, upstream).await.unwrap();
    });

    let (service, store) = test_service_with_quire_base(&format!("http://{address}")).await;
    let session = service
        .create_session(&unique("comment-follow-up"), Some("report-test"))
        .await
        .unwrap();
    let app = quire_runtime::http::router(service.clone());
    let replied = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/sessions/{}/comments/ct_proxy/replies",
                    session.session_id
                ))
                .header("authorization", "Bearer runtime-test-token")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({"body": "Use a different chart"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replied.status(), axum::http::StatusCode::OK);
    wait_for_idle(&store, &session.session_id).await;

    let messages = store.list_messages(&session.session_id).await.unwrap();
    assert!(
        messages.iter().any(|message| {
            message.role == MessageRole::User
                && message.content.is_empty()
                && message.comment_refs.iter().any(|comment_ref| {
                    comment_ref.thread_id == "ct_proxy"
                        && comment_ref.comment == "Use a different chart"
                })
        }),
        "replying to an addressed comment must re-submit it to the agent"
    );
    service.delete_session(&session.session_id).await.unwrap();
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn open_comment_reply_also_triggers_agent() {
    let _guard = test_lock().lock().await;
    let upstream = axum::Router::new().route(
        "/api/docs/{doc_id}/comments/{thread_id}/replies",
        axum::routing::post(mock_comment_reply_open),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, upstream).await.unwrap();
    });

    let (service, store) = test_service_with_quire_base(&format!("http://{address}")).await;
    let session = service
        .create_session(&unique("comment-open-reply"), Some("report-test"))
        .await
        .unwrap();
    let app = quire_runtime::http::router(service.clone());
    let replied = app
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!(
                    "/api/sessions/{}/comments/ct_proxy/replies",
                    session.session_id
                ))
                .header("authorization", "Bearer runtime-test-token")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({"body": "More context while the agent is working"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(replied.status(), axum::http::StatusCode::OK);
    wait_for_idle(&store, &session.session_id).await;

    let messages = store.list_messages(&session.session_id).await.unwrap();
    assert!(
        messages.iter().any(|message| {
            message.role == MessageRole::User
                && message.content.is_empty()
                && message.comment_refs.iter().any(|comment_ref| {
                    comment_ref.thread_id == "ct_proxy"
                        && comment_ref.comment == "More context while the agent is working"
                })
        }),
        "follow-up replies on open threads must also reach the agent"
    );
    service.delete_session(&session.session_id).await.unwrap();
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn running_session_queues_comment_and_queue_delete_removes_thread() {
    let _guard = test_lock().lock().await;
    let deleted = Arc::new(Mutex::new(Vec::<String>::new()));
    let upstream = axum::Router::new()
        .route(
            "/api/docs/{doc_id}/comments",
            axum::routing::post(mock_comment_create),
        )
        .route(
            "/api/docs/{doc_id}/comments/{thread_id}",
            axum::routing::delete(mock_comment_delete),
        )
        .with_state(deleted.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, upstream).await.unwrap();
    });

    let gate = GateStreamProvider::new();
    let (service, store) = test_service_with_quire_base(&format!("http://{address}")).await;
    let service = service.with_model_stream_provider(Arc::new(gate.clone()));
    let session = service
        .create_session(&unique("comment-queue"), Some("report-test"))
        .await
        .unwrap();
    let started = service
        .post_message(
            &session.session_id,
            &SendMessageRequest {
                client_msg_id: unique("comment-queue-first"),
                content: "Start the long turn".into(),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(started, PostMessageOutcome::Started { .. }));
    wait_for_running(&store, &session.session_id).await;

    let app = quire_runtime::http::router(service.clone());
    let created = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri(format!("/api/sessions/{}/comments", session.session_id))
                .header("authorization", "Bearer runtime-test-token")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({"block_id": "b_proxy", "body": "Tighten while busy"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(created.status(), axum::http::StatusCode::OK);

    let queue = service
        .list_pending_queue(&session.session_id)
        .await
        .unwrap();
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].source, PendingQueueSource::Comment);
    assert_eq!(queue[0].thread_id.as_deref(), Some("ct_proxy"));
    let queue_id = queue[0].queue_id.clone();

    let removed = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("DELETE")
                .uri(format!(
                    "/api/sessions/{}/queue/{}",
                    session.session_id, queue_id
                ))
                .header("authorization", "Bearer runtime-test-token")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(removed.status(), axum::http::StatusCode::NO_CONTENT);
    assert_eq!(
        deleted.lock().await.as_slice(),
        ["report-test:ct_proxy".to_string()]
    );
    assert!(
        service
            .list_pending_queue(&session.session_id)
            .await
            .unwrap()
            .is_empty()
    );

    gate.release();
    wait_for_idle(&store, &session.session_id).await;
    let messages = store.list_messages(&session.session_id).await.unwrap();
    assert!(
        !messages
            .iter()
            .any(|message| !message.comment_refs.is_empty()),
        "deleting a queued comment must not leave an agent trigger behind"
    );
    service.delete_session(&session.session_id).await.unwrap();
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn consecutive_queued_comments_merge_into_one_message() {
    let _guard = test_lock().lock().await;
    let gate = GateStreamProvider::new();
    let (service, store) = test_service().await;
    let service = service.with_model_stream_provider(Arc::new(gate.clone()));
    service.boot().await.unwrap();
    let session = service
        .create_session(&unique("comment-merge"), Some("report-test"))
        .await
        .unwrap();
    let started = service
        .post_message(
            &session.session_id,
            &SendMessageRequest {
                client_msg_id: unique("comment-merge-first"),
                content: "Start the long turn".into(),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(started, PostMessageOutcome::Started { .. }));
    wait_for_running(&store, &session.session_id).await;

    for (thread_id, comment) in [
        ("ct_a", "First queued comment"),
        ("ct_b", "Second queued comment"),
    ] {
        let outcome = service
            .post_comment_message(&session.session_id, comment_ref(thread_id, comment))
            .await
            .unwrap();
        let item = match outcome {
            PostMessageOutcome::Queued { item } => item,
            other => panic!("expected queued comment, got {other:?}"),
        };
        assert_eq!(item.source, PendingQueueSource::Comment);
    }
    assert_eq!(
        service
            .list_pending_queue(&session.session_id)
            .await
            .unwrap()
            .len(),
        2
    );

    gate.release();
    for _ in 0..300 {
        let detail = service.session_detail(&session.session_id).await.unwrap();
        if detail.pending_queue.is_empty()
            && detail.summary.active_run_status == ActiveRunStatus::Idle
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let messages = store.list_messages(&session.session_id).await.unwrap();
    let merged: Vec<&SessionMessage> = messages
        .iter()
        .filter(|message| {
            message.role == MessageRole::User
                && message
                    .comment_refs
                    .iter()
                    .any(|comment_ref| comment_ref.thread_id == "ct_a")
                && message
                    .comment_refs
                    .iter()
                    .any(|comment_ref| comment_ref.thread_id == "ct_b")
        })
        .collect();
    assert_eq!(merged.len(), 1, "consecutive comments must be merged");
    let merged = merged.first();
    assert_eq!(merged.map(|message| message.comment_refs.len()), Some(2));
    assert_eq!(
        merged.and_then(|message| message
            .comment_refs
            .first()
            .map(|item| item.comment.as_str())),
        Some("First queued comment")
    );
    assert_eq!(
        merged.and_then(|message| message
            .comment_refs
            .get(1)
            .map(|item| item.comment.as_str())),
        Some("Second queued comment")
    );
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn repeated_comment_follow_ups_update_the_pending_item() {
    let _guard = test_lock().lock().await;
    let gate = GateStreamProvider::new();
    let (service, store) = test_service().await;
    let service = service.with_model_stream_provider(Arc::new(gate.clone()));
    service.boot().await.unwrap();
    let session = service
        .create_session(&unique("comment-follow-up-dedupe"), Some("report-test"))
        .await
        .unwrap();
    let started = service
        .post_message(
            &session.session_id,
            &SendMessageRequest {
                client_msg_id: unique("comment-follow-up-dedupe-first"),
                content: "Start the long turn".into(),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(started, PostMessageOutcome::Started { .. }));
    wait_for_running(&store, &session.session_id).await;

    let first = service
        .post_comment_message(&session.session_id, comment_ref("ct_same", "First note"))
        .await
        .unwrap();
    let first_item = match first {
        PostMessageOutcome::Queued { item } => item,
        other => panic!("expected queued comment, got {other:?}"),
    };
    let second = service
        .post_comment_reply_message(
            &session.session_id,
            "msg_reply_two",
            comment_ref("ct_same", "Latest note"),
        )
        .await
        .unwrap();
    let second_item = match second {
        PostMessageOutcome::Queued { item } => item,
        other => panic!("expected updated comment queue item, got {other:?}"),
    };
    assert_eq!(first_item.queue_id, second_item.queue_id);
    let queue = service
        .list_pending_queue(&session.session_id)
        .await
        .unwrap();
    assert_eq!(queue.len(), 1, "a follow-up must update, not duplicate");
    assert!(queue[0].content.is_empty());
    assert_eq!(queue[0].comment_refs.len(), 1);
    assert_eq!(queue[0].comment_refs[0].comment, "Latest note");

    gate.release();
    for _ in 0..300 {
        let detail = service.session_detail(&session.session_id).await.unwrap();
        if detail.pending_queue.is_empty()
            && detail.summary.active_run_status == ActiveRunStatus::Idle
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test]
async fn chart_preview_endpoint_accepts_ro_token_and_hides_expired_previews() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let preview_id = unique("chart-preview");
    let record = ChartPreviewRecord {
        preview_id: preview_id.clone(),
        chart: EchartsBlock {
            title: Some("Preview".into()),
            height: 300,
            option: json!({"series": [{"type": "bar", "data": [1, 2]}]}),
            option_patch: None,
            fallback_summary: "Two bars.".into(),
            numbering: None,
            note: None,
        },
        requirements: ChartRequirements::default(),
        tables: vec![],
        title: Some("Preview".into()),
        note: None,
        based_on_preview_id: None,
    };
    store.bus().store_chart_preview(&record).await.unwrap();
    let app = quire_runtime::http::router(service);
    let authorized = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(format!("/api/chart-previews/{preview_id}"))
                .header("authorization", "Bearer test-token")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(authorized.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(authorized.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["preview_id"], preview_id);
    assert_eq!(body["chart"]["option"]["series"][0]["type"], "bar");

    let unauthorized = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(format!("/api/chart-previews/{preview_id}"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), axum::http::StatusCode::UNAUTHORIZED);

    let missing = app
        .oneshot(
            axum::http::Request::builder()
                .uri(format!("/api/chart-previews/{}", unique("missing-chart")))
                .header("authorization", "Bearer test-token")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), axum::http::StatusCode::NOT_FOUND);
    store.bus().delete_chart_preview(&preview_id).await.unwrap();
}

#[tokio::test]
async fn session_detail_returns_an_atomic_active_run_snapshot() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let session = service
        .create_session(&unique("active-trace"), None)
        .await
        .unwrap();
    let run = store
        .create_run(&session.session_id, None, "main", "stub/echo")
        .await
        .unwrap();
    let session_id = session.session_id.as_str();

    let turn_started = store
        .append_event(
            session_id,
            &run.run_id,
            "main",
            None,
            SessionEventKind::TurnStarted,
        )
        .await
        .unwrap();
    store
        .append_event(
            session_id,
            &run.run_id,
            "main",
            None,
            SessionEventKind::ThinkingDelta {
                delta: "先核对统计口径。".into(),
            },
        )
        .await
        .unwrap();
    store
        .append_event(
            session_id,
            &run.run_id,
            "main",
            None,
            SessionEventKind::ToolCallStarted {
                tool_call_id: "main-done".into(),
                tool_name: "read_blocks".into(),
                input_preview: "{}".into(),
                truncated: false,
            },
        )
        .await
        .unwrap();
    store
        .append_event(
            session_id,
            &run.run_id,
            "main",
            None,
            SessionEventKind::ToolCallFinished {
                tool_call_id: "main-done".into(),
                status: "completed".into(),
                output_preview: "{}".into(),
                truncated: false,
                duration_ms: 1_200,
            },
        )
        .await
        .unwrap();
    store
        .append_event(
            session_id,
            &run.run_id,
            "main",
            None,
            SessionEventKind::ToolCallStarted {
                tool_call_id: "main-live".into(),
                tool_name: "preview_echarts".into(),
                input_preview: "{}".into(),
                truncated: false,
            },
        )
        .await
        .unwrap();
    let subagent_started = store
        .append_event(
            session_id,
            "run-sub",
            "analyst",
            Some(&run.run_id),
            SessionEventKind::SubagentStarted {
                subagent_run_id: "run-sub".into(),
                agent_name: "analyst".into(),
                display_name: Some("量化分析师".into()),
                task: "核对数据".into(),
                section_id: None,
            },
        )
        .await
        .unwrap();
    store
        .append_event(
            session_id,
            "run-sub",
            "analyst",
            Some(&run.run_id),
            SessionEventKind::TextDelta {
                delta: "子代理输出片段。".into(),
            },
        )
        .await
        .unwrap();
    store
        .append_event(
            session_id,
            "run-sub",
            "analyst",
            Some(&run.run_id),
            SessionEventKind::ToolCallStarted {
                tool_call_id: "sub-live".into(),
                tool_name: "read_blocks".into(),
                input_preview: "{}".into(),
                truncated: false,
            },
        )
        .await
        .unwrap();
    let subagent_thinking = store
        .append_event(
            session_id,
            "run-sub",
            "analyst",
            Some(&run.run_id),
            SessionEventKind::ThinkingDelta {
                delta: "子代理正在思考。".into(),
            },
        )
        .await
        .unwrap();
    let live_thinking = store
        .append_event(
            session_id,
            &run.run_id,
            "main",
            None,
            SessionEventKind::ThinkingDelta {
                delta: "正在核对最新数据。".into(),
            },
        )
        .await
        .unwrap();

    let detail = store.get_session_detail(session_id).await.unwrap();
    let event_count = store.list_events(session_id, 0, 1_000).await.unwrap().len();
    assert_eq!(detail.summary.last_event_seq, event_count as u64);
    let trace = detail
        .active_run_trace
        .as_ref()
        .expect("running session must expose its active trace");
    assert_eq!(trace.run_id, run.run_id);
    assert_eq!(trace.status, "running");
    assert_eq!(trace.started_at, Some(turn_started.timestamp));

    let execution = trace
        .segments
        .iter()
        .find_map(|segment| match segment {
            RunTraceSegment::Execution { tools, .. } => Some(tools),
            _ => None,
        })
        .expect("active trace must contain execution tools");
    let done = execution
        .iter()
        .find(|tool| tool.tool_call_id == "main-done")
        .expect("completed tool must be retained");
    assert_eq!(done.status, "completed");
    assert_eq!(done.duration_ms, Some(1_200));
    let running = execution
        .iter()
        .find(|tool| tool.tool_call_id == "main-live")
        .expect("running tool must be retained");
    assert_eq!(running.status, "running");

    let Some(RunTraceSegment::Thinking {
        text,
        duration_ms,
        started_at,
    }) = trace.segments.last()
    else {
        panic!("active thinking must be the trailing segment");
    };
    assert_eq!(text, "正在核对最新数据。");
    assert_eq!(*duration_ms, None);
    assert_eq!(*started_at, Some(live_thinking.timestamp));

    assert_eq!(detail.active_subagent_traces.len(), 1);
    let subagent = &detail.active_subagent_traces[0];
    assert_eq!(subagent.subagent_run_id, "run-sub");
    assert_eq!(subagent.status, "running");
    assert_eq!(subagent.started_at, Some(subagent_started.timestamp));
    assert_eq!(subagent.text, "子代理输出片段。");
    assert_eq!(subagent.thinking, "子代理正在思考。");
    assert_eq!(
        subagent.thinking_started_at,
        Some(subagent_thinking.timestamp)
    );
    assert!(
        subagent
            .tools
            .iter()
            .any(|tool| tool.tool_call_id == "sub-live" && tool.status == "running")
    );

    store
        .finish_run(&run.run_id, "completed", None)
        .await
        .unwrap();
    service.delete_session(session_id).await.unwrap();
}
