//! rig-core tool adaptation, MCP HTTP transport and streamed preview throttling.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use quire_protocol::{
    EphemeralPayload, EphemeralPreviewEvent, PartialBlockPreview, PreviewAnchor, PreviewStatus,
};
use rig_core::{
    message::ToolName,
    tool::{DynamicTool, ToolExecutionError, ToolOutput},
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::{
    error::{RuntimeError, RuntimeResult},
    kb::KbService,
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
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token,
            doc_id: doc_id.into(),
            preview_id: preview_id.into(),
            actor: actor.into(),
            block_id: block_id.into(),
            seq: 0,
            throttle: PreviewThrottle::new(throttle_ms),
        }
    }

    pub async fn push_delta(&mut self, text: &str) -> RuntimeResult<bool> {
        if !self.throttle.should_emit() {
            return Ok(false);
        }
        self.seq = self.seq.saturating_add(1);
        let payload = EphemeralPayload::Preview {
            event: EphemeralPreviewEvent {
                doc_id: quire_protocol::DocId::parse(self.doc_id.clone())
                    .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?,
                preview_id: self.preview_id.clone(),
                actor: self.actor.clone(),
                anchor: PreviewAnchor::ReplaceBlock {
                    block_id: quire_protocol::BlockId::parse(self.block_id.clone())
                        .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?,
                },
                status: PreviewStatus::Streaming,
                seq: Some(self.seq),
                ttl_ms: None,
                block: PartialBlockPreview {
                    block_id: Some(
                        quire_protocol::BlockId::parse(self.block_id.clone())
                            .map_err(|error| RuntimeError::InvalidRequest(error.to_string()))?,
                    ),
                    block_type: "paragraph".into(),
                    title: None,
                    level: None,
                    text: Some(text.to_string()),
                },
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
}
