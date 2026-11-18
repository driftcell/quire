//! rig-core tool adaptation, MCP HTTP transport and streamed preview throttling.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use futures::StreamExt;
use quire_protocol::{
    BlockId, EphemeralPayload, EphemeralPreviewEvent, PartialBlockPreview, PreviewAnchor,
    PreviewStatus,
};
use quire_session_protocol::{ContextUsage, MessageRole};
use rig_core::{
    completion::{
        AssistantContent, CompletionRequest, CompletionResponse, Message, ToolDefinition, Usage,
        message::ToolResult as MessageToolResult,
    },
    message::{AssistantMessage, ToolCall, ToolName},
    streaming::{Item, PartKind, StreamEvent, parse_partial_arguments},
    tool::{DynamicTool, ToolExecutionError, ToolOutput, ToolResult as ExecutedToolResult},
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    sync::{Mutex, mpsc},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;

use crate::{
    config::Config,
    error::{RuntimeError, RuntimeResult},
    kb::KbService,
    model_stream::{ModelEventStream, ModelStreamEvent, ModelStreamProvider, ModelTurnRequest},
    models::ModelRegistry,
    prompt::ContextMessage,
};

pub const BUILTIN_TOOLS: &[&str] = &[
    "load_skill",
    "read_skill_resource",
    "describe_task",
    "spawn_subagents",
];

#[derive(Debug, Clone, PartialEq)]
pub struct McpToolDescriptor {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

impl McpToolDescriptor {
    fn from_rmcp(tool: &rmcp::model::Tool) -> Option<Self> {
        Some(Self {
            name: tool.name.to_string(),
            description: tool.description.as_deref().unwrap_or_default().to_string(),
            input_schema: Value::Object((*tool.input_schema).clone()),
        })
    }
}

#[derive(Clone)]
pub struct McpHttpClient {
    http: reqwest::Client,
    base_url: String,
    token: SecretString,
    x_session_id: String,
    agent_id: String,
    mcp_session_id: Arc<Mutex<Option<String>>>,
    next_id: Arc<AtomicU64>,
}

impl McpHttpClient {
    pub fn new(
        base_url: impl Into<String>,
        token: SecretString,
        x_session_id: impl Into<String>,
        agent_id: impl Into<String>,
    ) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token,
            x_session_id: x_session_id.into(),
            agent_id: agent_id.into(),
            mcp_session_id: Arc::new(Mutex::new(None)),
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    async fn ensure_initialized(&self) -> RuntimeResult<()> {
        if self.mcp_session_id.lock().await.is_some() {
            return Ok(());
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let response = self
            .http
            .post(format!("{}/mcp", self.base_url))
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.token.expose_secret()),
            )
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-11-25")
            .header("x-session-id", &self.x_session_id)
            .header("x-actor", &self.agent_id)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": {"name": "quire-runtime", "version": env!("CARGO_PKG_VERSION")}
                }
            }))
            .send()
            .await?;
        let session_id = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let body = parse_rpc_body(response).await?;
        if body.get("error").is_some() {
            return Err(RuntimeError::Internal(format!(
                "MCP initialize failed: {body}"
            )));
        }
        if let Some(session_id) = session_id {
            *self.mcp_session_id.lock().await = Some(session_id.clone());
            let id = self.next_id.fetch_add(1, Ordering::SeqCst);
            let _ = self
                .http
                .post(format!("{}/mcp", self.base_url))
                .header(
                    reqwest::header::AUTHORIZATION,
                    format!("Bearer {}", self.token.expose_secret()),
                )
                .header("accept", "application/json, text/event-stream")
                .header("mcp-protocol-version", "2025-11-25")
                .header("x-session-id", &self.x_session_id)
                .header("x-actor", &self.agent_id)
                .header("mcp-session-id", session_id)
                .json(&json!({"jsonrpc":"2.0","id":id,"method":"notifications/initialized"}))
                .send()
                .await;
        }
        Ok(())
    }

    async fn rpc(&self, method: &str, params: Value) -> RuntimeResult<Value> {
        self.ensure_initialized().await?;
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let mut request = self
            .http
            .post(format!("{}/mcp", self.base_url))
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.token.expose_secret()),
            )
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-11-25")
            .header("x-session-id", &self.x_session_id)
            .header("x-actor", &self.agent_id);
        if let Some(session_id) = self.mcp_session_id.lock().await.clone() {
            request = request.header("mcp-session-id", session_id);
        }
        let response = request
            .json(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params,
            }))
            .send()
            .await?;
        let body = parse_rpc_body(response).await?;
        if let Some(error) = body.get("error") {
            return Err(RuntimeError::Internal(format!(
                "MCP {method} failed: {error}"
            )));
        }
        Ok(body.get("result").cloned().unwrap_or(Value::Null))
    }

    pub async fn list_tools(&self) -> RuntimeResult<Vec<McpToolDescriptor>> {
        let result = self.rpc("tools/list", json!({})).await?;
        let tools_value = result.get("tools").cloned().unwrap_or_else(|| json!([]));
        let tools: Vec<rmcp::model::Tool> = serde_json::from_value(tools_value)?;
        Ok(tools
            .iter()
            .filter_map(McpToolDescriptor::from_rmcp)
            .collect())
    }

    pub async fn call_tool(&self, name: &str, arguments: Value) -> RuntimeResult<Value> {
        let result = self
            .rpc("tools/call", json!({"name": name, "arguments": arguments}))
            .await?;
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            return Err(RuntimeError::Internal(format!(
                "MCP tool `{name}` returned isError: {result}"
            )));
        }
        if let Some(structured) = result.get("structuredContent") {
            return Ok(structured.clone());
        }
        if let Some(text) = result
            .get("content")
            .and_then(Value::as_array)
            .and_then(|content| content.first())
            .and_then(|item| item.get("text"))
            .and_then(Value::as_str)
        {
            return Ok(serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.into())));
        }
        Ok(result)
    }
}

async fn parse_rpc_body(response: reqwest::Response) -> RuntimeResult<Value> {
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let text = response.text().await?;
    if content_type.contains("text/event-stream") {
        let mut found = None;
        for line in text.lines() {
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            if let Ok(value) = serde_json::from_str::<Value>(data)
                && (value.get("result").is_some() || value.get("error").is_some())
            {
                found = Some(value);
            }
        }
        found.ok_or_else(|| RuntimeError::Internal("MCP SSE response had no result".into()))
    } else if text.trim().is_empty() {
        Ok(Value::Null)
    } else {
        Ok(serde_json::from_str(&text)?)
    }
}

/// Adapt MCP descriptors into rig's dynamic tool registry, preserving allow-list policy.
pub async fn build_rig_tools(
    client: McpHttpClient,
    allowed_tools: &[String],
) -> RuntimeResult<Vec<DynamicTool>> {
    let descriptors = client.list_tools().await?;
    Ok(descriptors
        .into_iter()
        .filter(|descriptor| allowed_tools.iter().any(|name| name == &descriptor.name))
        .filter_map(|descriptor| {
            let name = ToolName::new(descriptor.name.clone()).ok()?;
            let tool_name = descriptor.name.clone();
            let call_client = client.clone();
            Some(DynamicTool::new(
                name,
                descriptor.description,
                descriptor.input_schema,
                move |arguments| {
                    let call_client = call_client.clone();
                    let tool_name = tool_name.clone();
                    Box::pin(async move {
                        call_client
                            .call_tool(&tool_name, arguments)
                            .await
                            .map(ToolOutput::json)
                            .map_err(|error| ToolExecutionError::other(error.to_string()))
                    })
                },
            ))
        })
        .collect())
}

/// Register the runtime's built-in tools on the same rig dynamic registry.
pub fn build_builtin_tools(kb: KbService) -> RuntimeResult<Vec<DynamicTool>> {
    let mut tools = Vec::new();

    let load_kb = kb.clone();
    tools.push(DynamicTool::new(
        ToolName::new("load_skill").map_err(|error| RuntimeError::Internal(error.to_string()))?,
        "Load a skill definition from the runtime knowledge base.",
        json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "required": ["name"],
            "additionalProperties": false
        }),
        move |arguments| {
            let kb = load_kb.clone();
            Box::pin(async move {
                let name = arguments
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ToolExecutionError::invalid_args("`name` is required"))?;
                let skill = kb
                    .get_skill(name)
                    .await
                    .map_err(|error| ToolExecutionError::other(error.to_string()))?
                    .ok_or_else(|| ToolExecutionError::not_found(format!("skill `{name}`")))?;
                serde_json::to_value(skill)
                    .map(ToolOutput::json)
                    .map_err(|error| ToolExecutionError::other(error.to_string()))
            })
        },
    ));

    let resource_kb = kb.clone();
    tools.push(DynamicTool::new(
        ToolName::new("read_skill_resource")
            .map_err(|error| RuntimeError::Internal(error.to_string()))?,
        "Read one resource file bundled with a skill.",
        json!({
            "type": "object",
            "properties": {
                "skill": {"type": "string"},
                "path": {"type": "string"}
            },
            "required": ["skill", "path"],
            "additionalProperties": false
        }),
        move |arguments| {
            let kb = resource_kb.clone();
            Box::pin(async move {
                let skill = arguments
                    .get("skill")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ToolExecutionError::invalid_args("`skill` is required"))?;
                let path = arguments
                    .get("path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ToolExecutionError::invalid_args("`path` is required"))?;
                let definition = kb
                    .get_skill(skill)
                    .await
                    .map_err(|error| ToolExecutionError::other(error.to_string()))?
                    .ok_or_else(|| ToolExecutionError::not_found(format!("skill `{skill}`")))?;
                let resource = definition
                    .resources
                    .iter()
                    .find(|resource| resource.path == path)
                    .ok_or_else(|| {
                        ToolExecutionError::not_found(format!("resource `{path}` in `{skill}`"))
                    })?;
                Ok(ToolOutput::json(json!({
                    "skill": skill,
                    "path": path,
                    "content": resource.content
                })))
            })
        },
    ));

    tools.push(DynamicTool::new(
        ToolName::new("describe_task")
            .map_err(|error| RuntimeError::Internal(error.to_string()))?,
        "Publish the current task focus to the frontend event stream.",
        json!({
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "detail": {"type": "string", "nullable": true},
                "section_id": {"type": "string", "nullable": true}
            },
            "required": ["title"],
            "additionalProperties": false
        }),
        |arguments| {
            Box::pin(async move {
                let title = arguments
                    .get("title")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ToolExecutionError::invalid_args("`title` is required"))?;
                Ok(ToolOutput::json(json!({
                    "title": title,
                    "detail": arguments.get("detail").cloned().unwrap_or(Value::Null),
                    "section_id": arguments.get("section_id").cloned().unwrap_or(Value::Null)
                })))
            })
        },
    ));

    tools.push(DynamicTool::new(
        ToolName::new("spawn_subagents")
            .map_err(|error| RuntimeError::Internal(error.to_string()))?,
        "Run multiple subagents concurrently with a JoinSet.",
        json!({
            "type": "object",
            "properties": {
                "tasks": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "agent_name": {"type": "string"},
                            "model": {"type": "string", "nullable": true},
                            "task": {"type": "string"},
                            "section_id": {"type": "string", "nullable": true}
                        },
                        "required": ["agent_name", "task"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["tasks"],
            "additionalProperties": false
        }),
        |arguments| {
            Box::pin(async move {
                let tasks: Vec<SubagentTask> = serde_json::from_value(
                    arguments
                        .get("tasks")
                        .cloned()
                        .ok_or_else(|| ToolExecutionError::invalid_args("`tasks` is required"))?,
                )
                .map_err(|error| ToolExecutionError::invalid_args(error.to_string()))?;
                let outcomes = spawn_subagents(tasks, |task| async move {
                    SubagentOutcome {
                        agent_name: task.agent_name,
                        status: "completed".into(),
                        summary: task.task,
                    }
                })
                .await
                .map_err(|error| ToolExecutionError::other(error.to_string()))?;
                Ok(ToolOutput::json(json!({"outcomes": outcomes})))
            })
        },
    ));

    Ok(tools)
}

/// Register built-in and allow-listed MCP tools together.
pub async fn build_agent_tools(
    kb: KbService,
    client: McpHttpClient,
    allowed_tools: &[String],
) -> RuntimeResult<Vec<DynamicTool>> {
    let mut tools = build_builtin_tools(kb)?;
    let remote_allow: Vec<String> = allowed_tools
        .iter()
        .filter(|name| !BUILTIN_TOOLS.contains(&name.as_str()))
        .cloned()
        .collect();
    tools.extend(build_rig_tools(client, &remote_allow).await?);
    Ok(tools)
}

/// Throttle for streamed tool-argument preview frames.
#[derive(Debug)]
pub struct PreviewThrottle {
    interval: Duration,
    last_emit: Option<Instant>,
}

impl PreviewThrottle {
    pub fn new(milliseconds: u64) -> Self {
        Self {
            interval: Duration::from_millis(milliseconds.clamp(50, 80)),
            last_emit: None,
        }
    }

    pub fn should_emit(&mut self) -> bool {
        self.should_emit_at(Instant::now())
    }

    fn should_emit_at(&mut self, now: Instant) -> bool {
        if self
            .last_emit
            .is_some_and(|last| now.duration_since(last) < self.interval)
        {
            return false;
        }
        self.last_emit = Some(now);
        true
    }
}

/// Sends partial tool-argument previews to Quire's ephemeral preview endpoint.
pub struct ToolPreviewStreamer {
    http: reqwest::Client,
    base_url: String,
    token: SecretString,
    doc_id: String,
    preview_id: String,
    actor: String,
    block_id: String,
    anchor: PreviewAnchor,
    seq: u64,
    throttle: PreviewThrottle,
}

impl ToolPreviewStreamer {
    pub fn new(
        base_url: impl Into<String>,
        token: SecretString,
        doc_id: impl Into<String>,
        preview_id: impl Into<String>,
        actor: impl Into<String>,
        block_id: impl Into<String>,
        throttle_ms: u64,
    ) -> Self {
        let block_id = block_id.into();
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token,
            doc_id: doc_id.into(),
            preview_id: preview_id.into(),
            actor: actor.into(),
            anchor: PreviewAnchor::ReplaceBlock {
                block_id: quire_protocol::BlockId::pending(),
            },
            block_id,
            seq: 0,
            throttle: PreviewThrottle::new(throttle_ms),
        }
    }

    /// Override where the previewed block appears. Replace anchors are rewritten to the
    /// streamer's block ID.
    pub fn with_anchor(mut self, anchor: PreviewAnchor) -> Self {
        self.anchor = anchor;
        self
    }

    pub fn preview_id(&self) -> &str {
        &self.preview_id
    }

    pub async fn push_delta(&mut self, text: &str) -> RuntimeResult<bool> {
        let block_id = quire_protocol::BlockId::parse(self.block_id.clone())
            .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
        self.push_block(PartialBlockPreview {
            block_id: Some(block_id),
            block_type: "paragraph".into(),
            title: None,
            level: None,
            text: Some(text.to_string()),
        })
        .await
    }

    /// Push one partial block frame. The streamer remains responsible for throttling and
    /// monotonic sequence numbers.
    pub async fn push_block(&mut self, block: PartialBlockPreview) -> RuntimeResult<bool> {
        if !self.throttle.should_emit() {
            return Ok(false);
        }
        self.seq = self.seq.saturating_add(1);
        let block_id = quire_protocol::BlockId::parse(self.block_id.clone())
            .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?;
        let anchor = match self.anchor.clone() {
            PreviewAnchor::ReplaceBlock { .. } => PreviewAnchor::ReplaceBlock {
                block_id: block_id.clone(),
            },
            PreviewAnchor::AfterBlock { .. } => PreviewAnchor::AfterBlock {
                block_id: block_id.clone(),
            },
            PreviewAnchor::BeforeBlock { .. } => PreviewAnchor::BeforeBlock {
                block_id: block_id.clone(),
            },
            other => other,
        };
        let payload = EphemeralPayload::Preview {
            event: EphemeralPreviewEvent {
                doc_id: quire_protocol::DocId::parse(self.doc_id.clone())
                    .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?,
                preview_id: self.preview_id.clone(),
                actor: self.actor.clone(),
                anchor,
                status: PreviewStatus::Streaming,
                seq: Some(self.seq),
                ttl_ms: None,
                block,
                received_at: chrono::Utc::now(),
            },
        };
        let response = self
            .http
            .post(format!(
                "{}/api/docs/{}/ephemeral",
                self.base_url, self.doc_id
            ))
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.token.expose_secret()),
            )
            .json(&payload)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(RuntimeError::Internal(format!(
                "preview publish failed with {}",
                response.status()
            )));
        }
        Ok(true)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentTask {
    pub agent_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentOutcome {
    pub agent_name: String,
    pub status: String,
    pub summary: String,
}

/// Run subagent closures concurrently with a bounded JoinSet.
pub async fn spawn_subagents<F, Fut>(
    tasks: Vec<SubagentTask>,
    runner: F,
) -> RuntimeResult<Vec<SubagentOutcome>>
where
    F: Fn(SubagentTask) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = SubagentOutcome> + Send + 'static,
{
    let mut joinset = tokio::task::JoinSet::new();
    for task in tasks {
        joinset.spawn(runner(task));
    }
    let mut outcomes = Vec::new();
    while let Some(result) = joinset.join_next().await {
        outcomes
            .push(result.map_err(|error| {
                RuntimeError::Internal(format!("subagent join error: {error}"))
            })?);
    }
    Ok(outcomes)
}

const PREVIEW_TOOLS: &[&str] = &["apply_operations", "apply_draft"];

#[derive(Clone)]
pub struct RigModelStreamProvider {
    core: Arc<RigModelCore>,
}

impl RigModelStreamProvider {
    pub fn new(kb: KbService, models: Arc<ModelRegistry>, config: Arc<Config>) -> Self {
        Self {
            core: Arc::new(RigModelCore { kb, models, config }),
        }
    }
}

#[async_trait]
impl ModelStreamProvider for RigModelStreamProvider {
    async fn stream_turn(&self, request: ModelTurnRequest) -> RuntimeResult<ModelEventStream> {
        let core = self.core.clone();
        let cancellation = request.cancellation.clone();
        let (sender, mut receiver) = mpsc::unbounded_channel::<RuntimeResult<ModelStreamEvent>>();
        tokio::spawn(async move {
            if let Err(error) = core.run_main_turn(request, sender.clone()).await {
                let _ = sender.send(Err(error));
            }
        });
        let stream = async_stream::stream! {
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
        Ok(Box::pin(stream))
    }
}

struct RigModelCore {
    kb: KbService,
    models: Arc<ModelRegistry>,
    config: Arc<Config>,
}

#[derive(Clone)]
struct AgentExecution {
    agent_id: String,
    model_route: String,
    allowed_tools: Vec<String>,
    system_prompt: String,
    kb_branch: Option<String>,
    doc_id: Option<String>,
    session_id: String,
    run_id: String,
    cancellation: CancellationToken,
}

#[derive(Clone)]
struct SubagentScope {
    subagent_run_id: String,
    agent_name: String,
}

#[derive(Clone)]
struct EventScope {
    sender: mpsc::UnboundedSender<RuntimeResult<ModelStreamEvent>>,
    subagent: Option<SubagentScope>,
    cancellation: CancellationToken,
}

impl EventScope {
    fn top(
        sender: mpsc::UnboundedSender<RuntimeResult<ModelStreamEvent>>,
        token: CancellationToken,
    ) -> Self {
        Self {
            sender,
            subagent: None,
            cancellation: token,
        }
    }

    fn child(&self, subagent_run_id: String, agent_name: String) -> Self {
        Self {
            sender: self.sender.clone(),
            subagent: Some(SubagentScope {
                subagent_run_id,
                agent_name,
            }),
            cancellation: self.cancellation.clone(),
        }
    }

    fn send(&self, event: ModelStreamEvent) {
        let event = match &self.subagent {
            Some(scope) => ModelStreamEvent::SubagentEvent {
                subagent_run_id: scope.subagent_run_id.clone(),
                agent_name: scope.agent_name.clone(),
                event: Box::new(event),
            },
            None => event,
        };
        let _ = self.sender.send(Ok(event));
    }

    fn send_raw(&self, event: ModelStreamEvent) {
        let _ = self.sender.send(Ok(event));
    }
}

impl RigModelCore {
    async fn run_main_turn(
        self: Arc<Self>,
        request: ModelTurnRequest,
        sender: mpsc::UnboundedSender<RuntimeResult<ModelStreamEvent>>,
    ) -> RuntimeResult<()> {
        let branch = request.kb_branch.clone();
        let agent = self
            .kb
            .get_agent_on(branch.as_deref(), &request.agent_id)
            .await?
            .ok_or_else(|| {
                RuntimeError::InvalidRequest(format!(
                    "agent `{}` is not defined on branch `{}`; configure it with kb_upsert_agent",
                    request.agent_id,
                    branch.as_deref().unwrap_or("active")
                ))
            })?;
        let model_route = if request.model_route.trim().is_empty() {
            agent.model.clone()
        } else {
            request.model_route.clone()
        };
        let allowed_tools = if request.allowed_tools.is_empty() {
            agent.allowed_tools.clone()
        } else {
            request.allowed_tools.clone()
        };
        let messages = messages_from_request(&request);
        let execution = AgentExecution {
            agent_id: request.agent_id,
            model_route,
            allowed_tools,
            system_prompt: request.system_prompt,
            kb_branch: branch,
            doc_id: request.doc_id,
            session_id: request.session_id,
            run_id: request.run_id,
            cancellation: request.cancellation.clone(),
        };
        let scope = EventScope::top(sender, request.cancellation);
        self.run_turn(execution, messages, scope).await?;
        Ok(())
    }

    async fn run_turn(
        self: Arc<Self>,
        execution: AgentExecution,
        mut messages: Vec<Message>,
        scope: EventScope,
    ) -> RuntimeResult<String> {
        let resolved = self
            .models
            .resolve(&execution.model_route)
            .await
            .map_err(|error| {
                RuntimeError::InvalidRequest(format!(
                    "model route `{}` is not configured: {error}",
                    execution.model_route
                ))
            })?;
        let context_limit = u64::try_from(resolved.context_limit).unwrap_or(0);
        let model = self
            .models
            .build_rig_model(&execution.model_route)
            .await
            .map_err(|error| {
                RuntimeError::InvalidRequest(format!(
                    "model route `{}` cannot be built: {error}",
                    execution.model_route
                ))
            })?;
        let mut tools = self.build_tools(&execution, scope.clone(), Arc::clone(&self))?;
        let remote_allowed: Vec<String> = execution
            .allowed_tools
            .iter()
            .filter(|name| !BUILTIN_TOOLS.contains(&name.as_str()))
            .cloned()
            .collect();
        if !remote_allowed.is_empty() {
            let client = McpHttpClient::new(
                &self.config.quire_base_url,
                self.config.quire_rw_token.clone(),
                &execution.session_id,
                &execution.agent_id,
            );
            tools.extend(build_rig_tools(client, &remote_allowed).await?);
        }
        let tool_map: BTreeMap<String, DynamicTool> = tools
            .into_iter()
            .map(|tool| (tool.name().as_str().to_string(), tool))
            .collect();
        let mut total_usage = Usage::default();
        let mut final_text = String::new();
        loop {
            if execution.cancellation.is_cancelled() {
                return Ok(final_text);
            }
            let definitions: Vec<ToolDefinition> =
                tool_map.values().map(|tool| tool.definition()).collect();
            let request = build_completion_request(&execution, &messages, definitions);
            let mut stream = model.stream(request).map_err(model_provider_error)?;
            let mut previews = PreviewState::new(&execution, &self.config);
            let mut streamed_text = false;
            loop {
                tokio::select! {
                    _ = execution.cancellation.cancelled() => {
                        return Ok(final_text);
                    }
                    item = stream.next() => {
                        match item {
                            Some(Ok(Item::Event(event))) => {
                                if handle_stream_event(
                                    &event,
                                    &scope,
                                    &mut previews,
                                )
                                .await
                                {
                                    streamed_text = true;
                                }
                            }
                            Some(Ok(Item::Unknown(_))) => {}
                            Some(Err(error)) => return Err(model_provider_error(error)),
                            None => break,
                        }
                    }
                }
            }
            let response = stream.finish().await.map_err(model_provider_error)?;
            total_usage += response.usage;
            let text = response_text(&response);
            if !streamed_text && !text.is_empty() {
                scope.send(ModelStreamEvent::TextDelta(text.clone()));
            }
            let calls: Vec<ToolCall> = response
                .choice
                .iter()
                .filter_map(|content| match content {
                    AssistantContent::ToolCall(call) => Some(call.clone()),
                    _ => None,
                })
                .collect();
            if calls.is_empty() {
                final_text = text;
                break;
            }
            messages.push(Message::Assistant(AssistantMessage::new(
                response.choice.clone(),
            )));
            let mut answers: Vec<MessageToolResult> = Vec::new();
            for call in calls {
                answers.push(
                    execute_tool_call(&tool_map, &scope, &execution, &mut previews, call).await,
                );
            }
            messages.push(Message::tool_results(answers));
        }
        if execution.cancellation.is_cancelled() {
            return Ok(final_text);
        }
        scope.send(ModelStreamEvent::Usage(context_usage(
            &total_usage,
            context_limit,
        )));
        Ok(final_text)
    }

    fn build_tools(
        &self,
        execution: &AgentExecution,
        scope: EventScope,
        engine: Arc<RigModelCore>,
    ) -> RuntimeResult<Vec<DynamicTool>> {
        let mut tools = Vec::new();
        if execution
            .allowed_tools
            .iter()
            .any(|name| name == "load_skill")
        {
            let kb = self.kb.clone();
            let branch = execution.kb_branch.clone();
            tools.push(DynamicTool::new(
                ToolName::new("load_skill")
                    .map_err(|error| RuntimeError::Internal(error.to_string()))?,
                "Load a skill definition from the session knowledge-base branch.",
                json!({
                    "type": "object",
                    "properties": {"name": {"type": "string"}},
                    "required": ["name"],
                    "additionalProperties": false
                }),
                move |arguments| {
                    let kb = kb.clone();
                    let branch = branch.clone();
                    Box::pin(async move {
                        let name =
                            arguments
                                .get("name")
                                .and_then(Value::as_str)
                                .ok_or_else(|| {
                                    ToolExecutionError::invalid_args("`name` is required")
                                })?;
                        let skill = kb
                            .get_skill_on(branch.as_deref(), name)
                            .await
                            .map_err(|error| ToolExecutionError::other(error.to_string()))?
                            .ok_or_else(|| {
                                ToolExecutionError::not_found(format!("skill `{name}`"))
                            })?;
                        serde_json::to_value(skill)
                            .map(ToolOutput::json)
                            .map_err(|error| ToolExecutionError::other(error.to_string()))
                    })
                },
            ));
        }
        if execution
            .allowed_tools
            .iter()
            .any(|name| name == "read_skill_resource")
        {
            let kb = self.kb.clone();
            let branch = execution.kb_branch.clone();
            tools.push(DynamicTool::new(
                ToolName::new("read_skill_resource")
                    .map_err(|error| RuntimeError::Internal(error.to_string()))?,
                "Read one resource file bundled with a skill on the session branch.",
                json!({
                    "type": "object",
                    "properties": {
                        "skill": {"type": "string"},
                        "path": {"type": "string"}
                    },
                    "required": ["skill", "path"],
                    "additionalProperties": false
                }),
                move |arguments| {
                    let kb = kb.clone();
                    let branch = branch.clone();
                    Box::pin(async move {
                        let skill =
                            arguments
                                .get("skill")
                                .and_then(Value::as_str)
                                .ok_or_else(|| {
                                    ToolExecutionError::invalid_args("`skill` is required")
                                })?;
                        let path =
                            arguments
                                .get("path")
                                .and_then(Value::as_str)
                                .ok_or_else(|| {
                                    ToolExecutionError::invalid_args("`path` is required")
                                })?;
                        let definition = kb
                            .get_skill_on(branch.as_deref(), skill)
                            .await
                            .map_err(|error| ToolExecutionError::other(error.to_string()))?
                            .ok_or_else(|| {
                                ToolExecutionError::not_found(format!("skill `{skill}`"))
                            })?;
                        let resource = definition
                            .resources
                            .iter()
                            .find(|resource| resource.path == path)
                            .ok_or_else(|| {
                                ToolExecutionError::not_found(format!(
                                    "resource `{path}` in `{skill}`"
                                ))
                            })?;
                        Ok(ToolOutput::json(json!({
                            "skill": skill,
                            "path": path,
                            "content": resource.content
                        })))
                    })
                },
            ));
        }
        if execution
            .allowed_tools
            .iter()
            .any(|name| name == "describe_task")
        {
            let task_scope = scope.clone();
            tools.push(DynamicTool::new(
                ToolName::new("describe_task")
                    .map_err(|error| RuntimeError::Internal(error.to_string()))?,
                "Publish the current task focus to the frontend event stream.",
                json!({
                    "type": "object",
                    "properties": {
                        "title": {"type": "string"},
                        "detail": {"type": "string", "nullable": true},
                        "section_id": {"type": "string", "nullable": true}
                    },
                    "required": ["title"],
                    "additionalProperties": false
                }),
                move |arguments| {
                    let scope = task_scope.clone();
                    Box::pin(async move {
                        let title =
                            arguments
                                .get("title")
                                .and_then(Value::as_str)
                                .ok_or_else(|| {
                                    ToolExecutionError::invalid_args("`title` is required")
                                })?;
                        let detail = arguments
                            .get("detail")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        let section_id = arguments
                            .get("section_id")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        scope.send(ModelStreamEvent::TaskFocus {
                            title: title.to_string(),
                            detail: detail.clone(),
                            section_id: section_id.clone(),
                        });
                        Ok(ToolOutput::json(json!({
                            "title": title,
                            "detail": detail,
                            "section_id": section_id
                        })))
                    })
                },
            ));
        }
        if execution
            .allowed_tools
            .iter()
            .any(|name| name == "spawn_subagents")
        {
            let spawn_scope = scope.clone();
            let spawn_execution = execution.clone();
            tools.push(DynamicTool::new(
                ToolName::new("spawn_subagents")
                    .map_err(|error| RuntimeError::Internal(error.to_string()))?,
                "Run multiple branch-scoped subagents concurrently with a JoinSet.",
                json!({
                    "type": "object",
                    "properties": {
                        "tasks": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "agent_name": {"type": "string"},
                                    "model": {"type": "string", "nullable": true},
                                    "task": {"type": "string"},
                                    "section_id": {"type": "string", "nullable": true}
                                },
                                "required": ["agent_name", "task"],
                                "additionalProperties": false
                            }
                        }
                    },
                    "required": ["tasks"],
                    "additionalProperties": false
                }),
                move |arguments| {
                    let engine = engine.clone();
                    let scope = spawn_scope.clone();
                    let execution = spawn_execution.clone();
                    Box::pin(async move {
                        let tasks: Vec<SubagentTask> =
                            serde_json::from_value(arguments.get("tasks").cloned().ok_or_else(
                                || ToolExecutionError::invalid_args("`tasks` is required"),
                            )?)
                            .map_err(|error| ToolExecutionError::invalid_args(error.to_string()))?;
                        let mut joinset = JoinSet::new();
                        for task in tasks {
                            let engine = engine.clone();
                            let scope = scope.clone();
                            let execution = execution.clone();
                            joinset.spawn(async move {
                                engine.run_subagent(scope, execution, task).await
                            });
                        }
                        let mut outcomes = Vec::new();
                        while let Some(joined) = joinset.join_next().await {
                            match joined {
                                Ok(outcome) => outcomes.push(outcome),
                                Err(error) => outcomes.push(SubagentOutcome {
                                    agent_name: "unknown".into(),
                                    status: "failed".into(),
                                    summary: format!("subagent join error: {error}"),
                                }),
                            }
                        }
                        Ok(ToolOutput::json(json!({"outcomes": outcomes})))
                    })
                },
            ));
        }
        Ok(tools)
    }

    async fn run_subagent(
        self: Arc<Self>,
        parent_scope: EventScope,
        parent: AgentExecution,
        task: SubagentTask,
    ) -> SubagentOutcome {
        let subagent_run_id = format!("run_sub_{}", uuid::Uuid::new_v4().simple());
        parent_scope.send_raw(ModelStreamEvent::SubagentStarted {
            subagent_run_id: subagent_run_id.clone(),
            agent_name: task.agent_name.clone(),
            task: task.task.clone(),
            section_id: task.section_id.clone(),
        });
        let result = async {
            if parent.cancellation.is_cancelled() {
                return Err(RuntimeError::InvalidRequest("run cancelled".into()));
            }
            let definition = self
                .kb
                .get_agent_on(parent.kb_branch.as_deref(), &task.agent_name)
                .await?
                .ok_or_else(|| {
                    RuntimeError::InvalidRequest(format!(
                        "subagent `{}` is not defined on branch `{}`",
                        task.agent_name,
                        parent.kb_branch.as_deref().unwrap_or("active")
                    ))
                })?;
            let execution = AgentExecution {
                agent_id: task.agent_name.clone(),
                model_route: task.model.clone().unwrap_or(definition.model),
                allowed_tools: definition.allowed_tools,
                system_prompt: definition.system_prompt,
                kb_branch: parent.kb_branch.clone(),
                doc_id: parent.doc_id.clone(),
                session_id: parent.session_id.clone(),
                run_id: subagent_run_id.clone(),
                cancellation: parent.cancellation.clone(),
            };
            let scope = parent_scope.child(subagent_run_id.clone(), task.agent_name.clone());
            let summary = self
                .run_turn(execution, vec![Message::user(task.task.clone())], scope)
                .await?;
            Ok::<String, RuntimeError>(summary)
        }
        .await;
        let (status, summary) = match result {
            Ok(_) if parent_scope.cancellation.is_cancelled() => {
                ("cancelled".to_string(), "cancelled".to_string())
            }
            Ok(summary) if summary.trim().is_empty() => {
                ("completed".to_string(), "completed".to_string())
            }
            Ok(summary) => ("completed".to_string(), summary),
            Err(RuntimeError::InvalidRequest(message))
                if parent_scope.cancellation.is_cancelled() || message == "run cancelled" =>
            {
                ("cancelled".to_string(), "cancelled".to_string())
            }
            Err(error) => ("failed".to_string(), error.to_string()),
        };
        parent_scope.send_raw(ModelStreamEvent::SubagentFinished {
            subagent_run_id,
            status: status.clone(),
            summary: summary.clone(),
        });
        SubagentOutcome {
            agent_name: task.agent_name,
            status,
            summary,
        }
    }
}

fn messages_from_request(request: &ModelTurnRequest) -> Vec<Message> {
    let mut messages: Vec<Message> = request.history.iter().map(context_message).collect();
    if !matches!(messages.last(), Some(Message::User { .. })) {
        messages.push(Message::user(request.user_message.clone()));
    }
    messages
}

fn context_message(message: &ContextMessage) -> Message {
    match message.role {
        MessageRole::Assistant => Message::assistant(message.content.clone()),
        // Later system reminders are transient host guidance. Sending them as user content keeps
        // providers that only accept one leading system message happy.
        MessageRole::System => Message::user(message.content.clone()),
        MessageRole::User | MessageRole::Tool => Message::user(message.content.clone()),
    }
}

fn build_completion_request(
    execution: &AgentExecution,
    messages: &[Message],
    tools: Vec<ToolDefinition>,
) -> CompletionRequest {
    let mut conversation = messages.to_vec();
    let prompt = conversation
        .pop()
        .unwrap_or_else(|| Message::user(execution.system_prompt.clone()));
    let mut request = CompletionRequest::new(prompt)
        .preamble(runtime_preamble(execution))
        .tools(tools);
    if !conversation.is_empty() {
        request = request.messages(conversation);
    }
    request
}

fn runtime_preamble(execution: &AgentExecution) -> String {
    let mut preamble = execution.system_prompt.clone();
    preamble.push_str("\n\n<runtime_session>\n");
    if let Some(doc_id) = execution.doc_id.as_deref() {
        preamble.push_str(&format!(
            "Bound Quire report doc_id: `{doc_id}`. Use this doc_id for document tools and do not call create_report for this session.\n"
        ));
    }
    if let Some(branch) = execution.kb_branch.as_deref() {
        preamble.push_str(&format!(
            "Knowledge-base branch for this session: `{branch}`. Agent and skill lookups use this branch.\n"
        ));
    }
    preamble.push_str("</runtime_session>\n");
    preamble
}

async fn handle_stream_event(
    event: &StreamEvent,
    scope: &EventScope,
    previews: &mut PreviewState,
) -> bool {
    match event {
        StreamEvent::Start { part, kind, name } => {
            previews.on_start(part.index(), *kind, name.as_ref());
            false
        }
        StreamEvent::Text { text, .. } => {
            scope.send(ModelStreamEvent::TextDelta(text.clone()));
            true
        }
        StreamEvent::Reasoning { text, .. } => {
            scope.send(ModelStreamEvent::ThinkingDelta(text.clone()));
            false
        }
        StreamEvent::Arguments { part, json } => {
            previews.on_arguments(part.index(), json).await;
            false
        }
        StreamEvent::End { part, content } => {
            if let AssistantContent::ToolCall(call) = content {
                previews.on_end(part.index(), call);
            }
            false
        }
    }
}

async fn execute_tool_call(
    tool_map: &BTreeMap<String, DynamicTool>,
    scope: &EventScope,
    execution: &AgentExecution,
    previews: &mut PreviewState,
    call: ToolCall,
) -> MessageToolResult {
    let provider_call_id = call.id.wire().to_string();
    let tool_call_id = format!("{}:{provider_call_id}", execution.run_id);
    let tool_name = call.function.name.as_str().to_string();
    let mut arguments = call.function.arguments_value();
    if let Some(preview_ids) = previews.take_preview_ids(&provider_call_id) {
        inject_preview_ids(&mut arguments, &preview_ids);
    }
    let (input_preview, input_truncated) = preview_json(&arguments);
    scope.send(ModelStreamEvent::ToolCallStarted {
        tool_call_id: tool_call_id.clone(),
        tool_name: tool_name.clone(),
        input_preview,
        truncated: input_truncated,
        input: arguments.clone(),
    });
    let started = Instant::now();
    let outcome = if let Some(invalid) = call.function.invalid_arguments.clone() {
        Err(ToolExecutionError::invalid_args(format!(
            "tool arguments were not a JSON object: {invalid}"
        )))
    } else if let Some(tool) = tool_map.get(&tool_name) {
        tokio::select! {
            _ = execution.cancellation.cancelled() => {
                Err(ToolExecutionError::other("tool call cancelled by user"))
            }
            outcome = tool.run(arguments.clone()) => outcome,
        }
    } else {
        Err(ToolExecutionError::not_found(format!(
            "tool `{tool_name}` is not enabled for this agent"
        )))
    };
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let (result, output_value, error, status) = match outcome {
        Ok(output) => {
            let value = tool_output_value(&output);
            (
                ExecutedToolResult::success(output),
                value,
                None,
                "completed".to_string(),
            )
        }
        Err(error) => {
            let message = error.to_string();
            let value = json!({
                "error": message,
                "kind": format!("{:?}", error.kind()),
            });
            (
                ExecutedToolResult::failed(error),
                value,
                Some(message),
                "failed".to_string(),
            )
        }
    };
    let (output_preview, output_truncated) = preview_json(&output_value);
    scope.send(ModelStreamEvent::ToolCallFinished {
        tool_call_id,
        tool_name,
        status,
        output_preview,
        truncated: output_truncated,
        duration_ms,
        input: arguments,
        output: output_value,
        error,
    });
    call.answer(&result)
}

fn response_text(response: &CompletionResponse) -> String {
    let parts: Vec<&str> = response
        .choice
        .iter()
        .filter_map(|content| match content {
            AssistantContent::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect();
    parts.join("\n\n")
}

fn tool_output_value(output: &ToolOutput) -> Value {
    if let Some(value) = output.as_json() {
        return value.clone();
    }
    if let Some(text) = output.as_text() {
        return json!({"text": text});
    }
    serde_json::to_value(output).unwrap_or(Value::Null)
}

fn context_usage(usage: &Usage, context_limit: u64) -> ContextUsage {
    let input_tokens = usage.input_tokens.unwrap_or(0);
    let output_tokens = usage.output_tokens.unwrap_or(0);
    ContextUsage {
        input_tokens,
        output_tokens,
        cached_tokens: usage.cached_input_tokens.unwrap_or(0),
        context_used: usage
            .context_tokens()
            .unwrap_or_else(|| input_tokens.saturating_add(output_tokens)),
        context_limit,
    }
}

fn preview_json(value: &Value) -> (String, bool) {
    let raw = serde_json::to_string(value).unwrap_or_default();
    let mut chars = raw.chars();
    let mut preview: String = chars.by_ref().take(240).collect();
    let truncated = chars.next().is_some();
    if truncated {
        preview.push('…');
    }
    (preview, truncated)
}

fn model_provider_error(error: rig_core::error::ProviderError) -> RuntimeError {
    RuntimeError::Internal(format!("model provider error: {error}"))
}

#[derive(Clone)]
struct PreviewBlock {
    block_id: Option<String>,
    block_type: String,
    title: Option<String>,
    level: Option<u8>,
    text: Option<String>,
}

impl PreviewBlock {
    fn partial(&self, block_id: BlockId) -> PartialBlockPreview {
        PartialBlockPreview {
            block_id: Some(block_id),
            block_type: self.block_type.clone(),
            title: self.title.clone(),
            level: self.level,
            text: self.text.clone(),
        }
    }
}

struct StreamingPreview {
    tool_name: String,
    buffer: String,
    streamers: HashMap<String, ToolPreviewStreamer>,
    preview_ids: BTreeSet<String>,
}

struct PreviewState {
    run_id: String,
    doc_id: Option<String>,
    base_url: String,
    token: SecretString,
    actor: String,
    throttle_ms: u64,
    streams: HashMap<usize, StreamingPreview>,
    previews_by_call: HashMap<String, Vec<String>>,
}

impl PreviewState {
    fn new(execution: &AgentExecution, config: &Config) -> Self {
        Self {
            run_id: execution.run_id.clone(),
            doc_id: execution.doc_id.clone(),
            base_url: config.quire_base_url.clone(),
            token: config.quire_rw_token.clone(),
            actor: execution.agent_id.clone(),
            throttle_ms: config.preview_throttle_ms,
            streams: HashMap::new(),
            previews_by_call: HashMap::new(),
        }
    }

    fn on_start(&mut self, part: usize, kind: PartKind, name: Option<&ToolName>) {
        if kind != PartKind::ToolCall {
            return;
        }
        let Some(name) = name else {
            return;
        };
        if !PREVIEW_TOOLS.contains(&name.as_str()) || self.doc_id.is_none() {
            return;
        }
        self.streams.insert(
            part,
            StreamingPreview {
                tool_name: name.as_str().to_string(),
                buffer: String::new(),
                streamers: HashMap::new(),
                preview_ids: BTreeSet::new(),
            },
        );
    }

    async fn on_arguments(&mut self, part: usize, fragment: &str) {
        let Some(stream) = self.streams.get_mut(&part) else {
            return;
        };
        stream.buffer.push_str(fragment);
        let Some(doc_id) = self.doc_id.clone() else {
            return;
        };
        let arguments = Value::Object(parse_partial_arguments(&stream.buffer));
        let blocks = extract_preview_blocks(&arguments);
        for (index, candidate) in blocks.iter().enumerate() {
            let key = candidate
                .block_id
                .clone()
                .unwrap_or_else(|| format!("auto-{index}"));
            if !stream.streamers.contains_key(&key) {
                let block_id = candidate
                    .block_id
                    .clone()
                    .unwrap_or_else(|| format!("preview{part}{index}"));
                let preview_id = format!("{}:{}:{}:{}", self.run_id, stream.tool_name, part, key);
                let anchor = if candidate.block_id.is_some() {
                    PreviewAnchor::ReplaceBlock {
                        block_id: BlockId::pending(),
                    }
                } else {
                    PreviewAnchor::AppendToDoc
                };
                stream.streamers.insert(
                    key.clone(),
                    ToolPreviewStreamer::new(
                        &self.base_url,
                        self.token.clone(),
                        &doc_id,
                        &preview_id,
                        &self.actor,
                        &block_id,
                        self.throttle_ms,
                    )
                    .with_anchor(anchor),
                );
            }
            if let Some(streamer) = stream.streamers.get_mut(&key) {
                let block_id = BlockId::parse(
                    candidate
                        .block_id
                        .clone()
                        .unwrap_or_else(|| format!("preview{part}{index}")),
                )
                .unwrap_or_else(|_| BlockId::pending());
                if let Err(error) = streamer.push_block(candidate.partial(block_id)).await {
                    tracing::warn!(%error, tool = %stream.tool_name, "ephemeral preview publish failed");
                } else {
                    stream.preview_ids.insert(preview_id_for(streamer));
                }
            }
        }
    }

    fn on_end(&mut self, part: usize, call: &ToolCall) {
        let Some(stream) = self.streams.remove(&part) else {
            return;
        };
        if !stream.preview_ids.is_empty() {
            self.previews_by_call.insert(
                call.id.wire().to_string(),
                stream.preview_ids.into_iter().collect(),
            );
        }
    }

    fn take_preview_ids(&mut self, call_id: &str) -> Option<Vec<String>> {
        self.previews_by_call.remove(call_id)
    }
}

fn preview_id_for(streamer: &ToolPreviewStreamer) -> String {
    streamer.preview_id().to_string()
}

fn extract_preview_blocks(arguments: &Value) -> Vec<PreviewBlock> {
    let mut blocks = Vec::new();
    let mut seen = BTreeSet::new();
    collect_preview_blocks(arguments, 0, &mut blocks, &mut seen);
    blocks.truncate(8);
    blocks
}

fn collect_preview_blocks(
    value: &Value,
    depth: usize,
    blocks: &mut Vec<PreviewBlock>,
    seen: &mut BTreeSet<String>,
) {
    if depth > 8 || blocks.len() >= 8 {
        return;
    }
    match value {
        Value::Object(object) => {
            if let Some(block) = preview_block(object) {
                let key = block
                    .block_id
                    .clone()
                    .unwrap_or_else(|| format!("{}:{}", block.block_type, blocks.len()));
                if seen.insert(key) {
                    blocks.push(block);
                }
            }
            for child in object.values() {
                collect_preview_blocks(child, depth + 1, blocks, seen);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_preview_blocks(child, depth + 1, blocks, seen);
            }
        }
        _ => {}
    }
}

fn preview_block(object: &serde_json::Map<String, Value>) -> Option<PreviewBlock> {
    let block_type = object.get("type").and_then(Value::as_str)?;
    if !matches!(
        block_type,
        "heading" | "paragraph" | "quote" | "table" | "echarts" | "custom_html"
    ) {
        return None;
    }
    let text = object.get("text").and_then(Value::as_str);
    let title = object.get("title").and_then(Value::as_str);
    if text.is_none() && title.is_none() {
        return None;
    }
    let block_id = object
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| BlockId::parse((*value).to_string()).is_ok())
        .map(str::to_string);
    let level = object
        .get("level")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok());
    Some(PreviewBlock {
        block_id,
        block_type: block_type.to_string(),
        title: title.map(str::to_string),
        level,
        text: text.map(str::to_string),
    })
}

fn inject_preview_ids(arguments: &mut Value, preview_ids: &[String]) {
    if preview_ids.is_empty() {
        return;
    }
    let Some(object) = arguments.as_object_mut() else {
        return;
    };
    let mut merged: Vec<String> = object
        .get("preview_ids")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    for preview_id in preview_ids {
        if !merged.iter().any(|existing| existing == preview_id) {
            merged.push(preview_id.clone());
        }
    }
    object.insert("preview_ids".to_string(), json!(merged));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_throttle_respects_the_interval() {
        let mut throttle = PreviewThrottle::new(60);
        let start = Instant::now();
        assert!(throttle.should_emit_at(start));
        assert!(!throttle.should_emit_at(start + Duration::from_millis(20)));
        assert!(throttle.should_emit_at(start + Duration::from_millis(61)));
    }

    #[tokio::test]
    async fn subagents_run_concurrently_and_preserve_outcomes() {
        let tasks = vec![
            SubagentTask {
                agent_name: "a".into(),
                model: None,
                task: "one".into(),
                section_id: None,
            },
            SubagentTask {
                agent_name: "b".into(),
                model: Some("subagent/model-b".into()),
                task: "two".into(),
                section_id: Some("ch2".into()),
            },
        ];
        let outcomes = spawn_subagents(tasks, |task| async move {
            SubagentOutcome {
                agent_name: task.agent_name,
                status: "completed".into(),
                summary: task.task,
            }
        })
        .await
        .unwrap();
        assert_eq!(outcomes.len(), 2);
    }

    #[test]
    fn partial_apply_operations_arguments_produce_block_previews() {
        let raw = r#"{"ops":[{"type":"append","blocks":[{"id":"q3-revenue","type":"paragraph","text":"Revenue grew"#;
        let arguments = Value::Object(parse_partial_arguments(raw));
        let blocks = extract_preview_blocks(&arguments);
        assert_eq!(blocks.len(), 1);
        let block = blocks.first().expect("preview block");
        assert_eq!(block.block_id.as_deref(), Some("q3-revenue"));
        assert_eq!(block.block_type, "paragraph");
        assert_eq!(block.text.as_deref(), Some("Revenue grew"));
    }

    #[test]
    fn preview_ids_are_merged_into_tool_arguments() {
        let mut arguments = json!({"doc_id": "report", "preview_ids": ["old"]});
        inject_preview_ids(&mut arguments, &["new".into(), "old".into()]);
        let ids = arguments
            .get("preview_ids")
            .and_then(Value::as_array)
            .expect("preview ids");
        assert_eq!(ids.len(), 2);
        assert_eq!(ids.first().and_then(Value::as_str), Some("old"));
        assert_eq!(ids.last().and_then(Value::as_str), Some("new"));
    }

    #[test]
    fn usage_carries_real_tokens_and_context_limit() {
        let usage = Usage::new()
            .input_tokens(120_u64)
            .output_tokens(30_u64)
            .cached_input_tokens(40_u64);
        let mapped = context_usage(&usage, 64_000);
        assert_eq!(mapped.input_tokens, 120);
        assert_eq!(mapped.output_tokens, 30);
        assert_eq!(mapped.cached_tokens, 40);
        assert_eq!(mapped.context_used, 150);
        assert_eq!(mapped.context_limit, 64_000);
    }
}
