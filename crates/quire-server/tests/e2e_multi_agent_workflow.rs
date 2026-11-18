//! End-to-end multi-agent simulation against the compiled Quire server binary.
//!
//! Requires live PostgreSQL and Redis (`just up && just migrate`). The test boots the real
//! binary, exercises MCP tool calls over Streamable HTTP, observes SSE deltas, merges an offline
//! draft, applies Smart CAS retries, self-heals a dangling citation, rolls back, and verifies the
//! canonical frontend contract with computed table layouts.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::{
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, anyhow, bail};
use futures::StreamExt;
use reqwest::{Client, Response, header::HeaderMap};
use serde_json::{Value, json};
use tokio::sync::{Mutex, Notify};

fn unique(prefix: &str) -> String {
    format!(
        "{prefix}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn database_url() -> String {
    let _ = dotenvy::dotenv();
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://report:report@127.0.0.1:5432/report_core".into())
}

fn redis_url() -> String {
    let _ = dotenvy::dotenv();
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into())
}

struct ServerProcess(Child);

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn wait_health(http: &Client, base: &str) -> anyhow::Result<()> {
    let mut last_error = None;
    for _ in 0..150 {
        match http.get(format!("{base}/health")).send().await {
            Ok(response) if response.status().is_success() => return Ok(()),
            Ok(response) => last_error = Some(anyhow!("health status {}", response.status())),
            Err(error) => last_error = Some(error.into()),
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(last_error.unwrap_or_else(|| anyhow!("server did not become healthy")))
}

#[derive(Clone, Debug)]
struct SseFrame {
    event: Option<String>,
    data: String,
}

struct SseCollector {
    frames: Arc<Mutex<Vec<SseFrame>>>,
    connected: Arc<Notify>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for SseCollector {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl SseCollector {
    fn spawn(http: Client, url: String) -> Self {
        let frames = Arc::new(Mutex::new(Vec::new()));
        let connected = Arc::new(Notify::new());
        let frames_task = frames.clone();
        let connected_task = connected.clone();
        let task = tokio::spawn(async move {
            let response = match http.get(url).send().await {
                Ok(response) => response,
                Err(_) => return,
            };
            let mut stream = response.bytes_stream();
            let mut buffer: Vec<u8> = Vec::new();
            while let Some(chunk) = stream.next().await {
                let Ok(chunk) = chunk else { break };
                buffer.extend_from_slice(&chunk);
                while let Some(index) = find_frame_end(&buffer) {
                    let frame_bytes: Vec<u8> = buffer.drain(..index + 2).collect();
                    let frame_text = String::from_utf8_lossy(&frame_bytes[..index]);
                    connected_task.notify_one();
                    if let Some(frame) = parse_sse_frame(&frame_text) {
                        frames_task.lock().await.push(frame);
                    }
                }
            }
        });
        Self {
            frames,
            connected,
            task,
        }
    }

    async fn wait_connected(&self) -> anyhow::Result<()> {
        tokio::time::timeout(Duration::from_secs(8), self.connected.notified())
            .await
            .context("SSE connection did not open")?;
        Ok(())
    }

    async fn snapshot(&self) -> Vec<SseFrame> {
        self.frames.lock().await.clone()
    }

    async fn wait_for_revision(&self, revision: i64) -> anyhow::Result<()> {
        for _ in 0..200 {
            if self.snapshot().await.iter().any(|frame| {
                frame.event.as_deref() == Some("revision")
                    && json_revision(&frame.data) == Some(revision)
            }) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        bail!("timed out waiting for revision {revision} over SSE")
    }

    async fn wait_for_frame<F>(&self, mut predicate: F) -> anyhow::Result<()>
    where
        F: FnMut(&SseFrame) -> bool,
    {
        for _ in 0..200 {
            if self.snapshot().await.iter().any(&mut predicate) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        bail!("timed out waiting for a matching SSE frame")
    }

    async fn wait_for_doc_created(&self, doc_id: &str) -> anyhow::Result<()> {
        for _ in 0..200 {
            if self.snapshot().await.iter().any(|frame| {
                frame.event.as_deref() == Some("doc_created")
                    && serde_json::from_str::<Value>(&frame.data)
                        .ok()
                        .and_then(|value| value.get("doc_id").cloned())
                        .and_then(|value| value.as_str().map(str::to_string))
                        .as_deref()
                        == Some(doc_id)
            }) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        bail!("timed out waiting for doc_created {doc_id}")
    }
}

fn find_frame_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(2).position(|window| window == b"\n\n")
}

fn parse_sse_frame(frame: &str) -> Option<SseFrame> {
    let mut event = None;
    let mut data_lines = Vec::new();
    for line in frame.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim_start().to_string());
        }
    }
    if data_lines.is_empty() {
        return None;
    }
    Some(SseFrame {
        event,
        data: data_lines.join("\n"),
    })
}

fn json_revision(data: &str) -> Option<i64> {
    serde_json::from_str::<Value>(data)
        .ok()
        .and_then(|value| value.get("revision").and_then(Value::as_i64))
}

fn json_string_field(data: &str, field: &str) -> Option<String> {
    serde_json::from_str::<Value>(data)
        .ok()
        .and_then(|value| value.get(field).and_then(Value::as_str).map(str::to_string))
}

fn json_u64_field(data: &str, field: &str) -> Option<u64> {
    serde_json::from_str::<Value>(data)
        .ok()
        .and_then(|value| value.get(field).and_then(Value::as_u64))
}

fn json_string_array_field(data: &str, field: &str) -> Vec<String> {
    serde_json::from_str::<Value>(data)
        .ok()
        .and_then(|value| {
            value.get(field).and_then(Value::as_array).map(|ids| {
                ids.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
        })
        .unwrap_or_default()
}

fn json_cleared_preview_ids(data: &str) -> Vec<String> {
    json_string_array_field(data, "cleared_preview_ids")
}

struct McpClient {
    http: Client,
    endpoint: String,
    session_id: String,
    protocol_version: String,
    x_session_id: String,
    agent_id: String,
    next_id: AtomicU64,
}

impl McpClient {
    async fn connect(
        http: Client,
        base: &str,
        x_session_id: &str,
        agent_id: &str,
    ) -> anyhow::Result<Self> {
        let endpoint = format!("{base}/mcp");
        let response = http
            .post(&endpoint)
            .headers(mcp_headers(None, "2025-11-25", x_session_id, agent_id))
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": {"name": agent_id, "version": "1.0"}
                }
            }))
            .send()
            .await
            .context("MCP initialize request")?;
        let session_id = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| anyhow!("MCP initialize returned no Mcp-Session-Id"))?
            .to_string();
        let body = parse_rpc_response(response).await?;
        if let Some(error) = body.get("error") {
            bail!("MCP initialize error: {error}");
        }
        let result = body
            .get("result")
            .ok_or_else(|| anyhow!("MCP initialize did not return result"))?;
        let instructions = result
            .get("instructions")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(
            instructions.contains("INIT -> SCHEMA -> AUTHOR -> VERIFY"),
            "server instructions must expose the workflow state machine"
        );
        let protocol_version = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or("2025-11-25")
            .to_string();
        let notification = http
            .post(&endpoint)
            .headers(mcp_headers(
                Some(&session_id),
                &protocol_version,
                x_session_id,
                agent_id,
            ))
            .json(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .send()
            .await
            .context("MCP initialized notification")?;
        if !notification.status().is_success() {
            bail!("initialized notification failed: {}", notification.status());
        }
        Ok(Self {
            http,
            endpoint,
            session_id,
            protocol_version,
            x_session_id: x_session_id.to_string(),
            agent_id: agent_id.to_string(),
            next_id: AtomicU64::new(2),
        })
    }

    async fn request_raw(
        &self,
        method: &str,
        params: Value,
    ) -> anyhow::Result<(reqwest::StatusCode, Value)> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let response = self
            .http
            .post(&self.endpoint)
            .headers(mcp_headers(
                Some(&self.session_id),
                &self.protocol_version,
                &self.x_session_id,
                &self.agent_id,
            ))
            .json(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params,
            }))
            .send()
            .await
            .with_context(|| format!("MCP request {method}"))?;
        let status = response.status();
        let body = parse_rpc_response(response).await?;
        Ok((status, body))
    }

    async fn request(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let (status, body) = self.request_raw(method, params).await?;
        if let Some(error) = body.get("error") {
            bail!("MCP {method} failed ({status}): {error}");
        }
        Ok(body)
    }

    async fn call_tool_error(&self, name: &str, arguments: Value) -> anyhow::Result<Value> {
        let (_status, body) = self
            .request_raw("tools/call", json!({"name": name, "arguments": arguments}))
            .await?;
        body.get("error")
            .cloned()
            .ok_or_else(|| anyhow!("tool {name} unexpectedly succeeded"))
    }

    async fn call_tool(&self, name: &str, arguments: Value) -> anyhow::Result<Value> {
        let body = self
            .request("tools/call", json!({"name": name, "arguments": arguments}))
            .await?;
        let result = body
            .get("result")
            .ok_or_else(|| anyhow!("tools/call {name} returned no result"))?;
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            bail!("tool {name} returned isError: {result}");
        }
        if let Some(structured) = result.get("structuredContent") {
            return Ok(structured.clone());
        }
        let text = result
            .get("content")
            .and_then(Value::as_array)
            .and_then(|content| content.first())
            .and_then(|item| item.get("text"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("tool {name} returned no content"))?;
        serde_json::from_str(text).or_else(|_| Ok(Value::String(text.to_string())))
    }

    async fn list_tool_names(&self) -> anyhow::Result<Vec<String>> {
        let body = self.request("tools/list", json!({})).await?;
        Ok(body["result"]["tools"]
            .as_array()
            .ok_or_else(|| anyhow!("tools/list returned no tools"))?
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .map(str::to_string)
            .collect())
    }
}

fn mcp_headers(
    session_id: Option<&str>,
    protocol: &str,
    x_session_id: &str,
    agent_id: &str,
) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", "application/json".parse().unwrap());
    headers.insert(
        "accept",
        "application/json, text/event-stream".parse().unwrap(),
    );
    headers.insert("mcp-protocol-version", protocol.parse().unwrap());
    headers.insert("x-session-id", x_session_id.parse().unwrap());
    headers.insert("x-actor", agent_id.parse().unwrap());
    if let Some(session_id) = session_id {
        headers.insert("mcp-session-id", session_id.parse().unwrap());
    }
    headers
}

async fn parse_rpc_response(response: Response) -> anyhow::Result<Value> {
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let text = response.text().await.context("read MCP response body")?;
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
        found.ok_or_else(|| anyhow!("SSE response contained no JSON-RPC result: {text}"))
    } else if text.trim().is_empty() {
        Ok(json!({}))
    } else {
        serde_json::from_str(&text).with_context(|| format!("invalid JSON-RPC response: {text}"))
    }
}

async fn wait_until<F, Fut>(timeout: Duration, mut predicate: F) -> anyhow::Result<()>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let started = tokio::time::Instant::now();
    while started.elapsed() < timeout {
        if predicate().await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    bail!("condition not met within {timeout:?}")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn e2e_multi_agent_workflow() -> anyhow::Result<()> {
    let database_url = database_url();
    let redis_url = redis_url();
    let workspace = tempfile::tempdir()?;
    let rw_token = unique("rw_token");
    let ro_token = unique("ro_token");
    let mut auth_headers = reqwest::header::HeaderMap::new();
    auth_headers.insert(
        reqwest::header::AUTHORIZATION,
        format!("Bearer {rw_token}").parse()?,
    );
    let http = Client::builder()
        .timeout(Duration::from_secs(60))
        .default_headers(auth_headers)
        .build()?;
    let unauthenticated = Client::builder().timeout(Duration::from_secs(10)).build()?;
    let port = free_port();
    let base = format!("http://127.0.0.1:{port}");
    let child = Command::new(env!("CARGO_BIN_EXE_quire-server"))
        .env("DATABASE_URL", &database_url)
        .env("REDIS_URL", &redis_url)
        .env("QUIRE_RW_TOKEN", &rw_token)
        .env("QUIRE_RO_TOKEN", &ro_token)
        .env("BIND_ADDR", format!("127.0.0.1:{port}"))
        .env(
            "MCP_ALLOWED_HOSTS",
            format!("127.0.0.1,127.0.0.1:{port},localhost,localhost:{port}"),
        )
        .env("AUTO_MIGRATE", "true")
        .env("MCP_JSON_RESPONSE", "true")
        .env("SSE_HEARTBEAT_SECS", "5")
        .env("QUIRE_WORKSPACE_DIR", workspace.path())
        .env("RUST_LOG", "warn,quire_server=info")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("spawn quire-server binary")?;
    let _server = ServerProcess(child);
    wait_health(&http, &base).await?;

    let unauthorized = unauthenticated
        .get(format!("{base}/api/docs"))
        .send()
        .await?;
    assert_eq!(unauthorized.status().as_u16(), 401);
    let read_only_client = Client::builder()
        .timeout(Duration::from_secs(10))
        .default_headers({
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {ro_token}").parse()?,
            );
            headers
        })
        .build()?;
    let read_only = read_only_client
        .get(format!("{base}/api/docs"))
        .send()
        .await?;
    assert!(read_only.status().is_success());

    // The RO token must never be accepted by the mutating MCP endpoint.
    let ro_mcp = read_only_client
        .post(format!("{base}/mcp"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "ro-client", "version": "1.0"}
            }
        }))
        .send()
        .await?;
    assert_eq!(ro_mcp.status().as_u16(), 401);

    // 1. Session discovery SSE must be connected before create_report.
    let session_id = unique("sess_test");
    let session_sse = SseCollector::spawn(
        http.clone(),
        format!("{base}/api/sessions/{session_id}/events"),
    );
    session_sse.wait_connected().await?;

    // 2. Lead agent cold start via MCP.
    let lead = McpClient::connect(http.clone(), &base, &session_id, "lead-orchestrator").await?;
    let tool_names = lead.list_tool_names().await?;
    assert_eq!(tool_names.len(), 15, "tool names: {tool_names:?}");
    let guide = lead
        .call_tool("get_protocol_guide", json!({"topic":"quickstart"}))
        .await?;
    assert!(guide["guide"].as_str().unwrap().contains("INIT -> SCHEMA"));

    let title = "E2E Multi-Agent Report";
    let idempotency_key = unique("idem");
    let initial_blocks = json!([
        {
            "id": "ch1_placeholder",
            "type": "heading",
            "level": 2,
            "text": "Chapter 1 — Financial Performance"
        },
        {
            "id": "ch2_placeholder",
            "type": "heading",
            "level": 2,
            "text": "Chapter 2 — Market Context"
        },
        {
            "id": "ch0_dataset",
            "type": "echarts",
            "title": "Quarterly comparison",
            "height": 280,
            "option": {
                "dataset": {"source": [
                    ["Quarter", "Revenue", "Margin"],
                    ["Q1", 120, 62.5],
                    ["Q2", 150, 64.0]
                ]},
                "xAxis": {"type": "category"},
                "yAxis": {"type": "value"},
                "series": [
                    {"name": "Revenue", "type": "bar", "datasetIndex": 0, "encode": {"x": "Quarter", "y": "Revenue"}},
                    {"name": "Margin", "type": "bar", "datasetIndex": 0, "encode": {"x": "Quarter", "y": "Margin"}}
                ]
            },
            "fallback_summary": "Revenue grew from 120 to 150 while margin rose from 62.5% to 64.0%."
        },
        {
            "id": "ch0_map",
            "type": "echarts",
            "title": "Regional coverage",
            "height": 260,
            "mode": "option",
            "option": {
                "geo": {"map": "china"},
                "series": [
                    {"type": "map", "map": "world", "data": [{"name": "China", "value": 1}]}
                ]
            },
            "fallback_summary": "Coverage spans the China and world map registrations."
        }
    ]);
    let create_args = json!({
        "title": title,
        "enable_cover": true,
        "enable_toc": true,
        "initial_blocks": initial_blocks,
        "idempotency_key": idempotency_key,
    });
    let created = lead.call_tool("create_report", create_args.clone()).await?;
    let doc_id = created["doc_id"].as_str().unwrap().to_string();
    assert_eq!(created["revision"].as_i64(), Some(1));
    assert_eq!(created["session_id"].as_str(), Some(session_id.as_str()));
    let replay = lead.call_tool("create_report", create_args).await?;
    assert_eq!(replay["doc_id"].as_str(), Some(doc_id.as_str()));
    assert_eq!(replay["idempotent_replay"].as_bool(), Some(true));
    session_sse.wait_for_doc_created(&doc_id).await?;

    lead.call_tool(
        "set_cover",
        json!({
            "doc_id": doc_id,
            "cover_patch": {
                "enabled": true,
                "variant": "modern",
                "title": title,
                "subtitle": "Parallel agent verification",
                "authors": ["Lead Orchestrator", "Sub-Agent A", "Sub-Agent B"],
                "date": "2026-11-18",
                "meta": {"classification": "internal"}
            }
        }),
    )
    .await?;
    let toc_receipt = lead
        .call_tool(
            "configure_toc",
            json!({
                "doc_id": doc_id,
                "enabled": true,
                "title": "Contents",
                "max_depth": 2,
                "variant": "numbered"
            }),
        )
        .await?;
    assert_eq!(toc_receipt["toc"]["variant"].as_str(), Some("numbered"));
    let refs_receipt = lead
        .call_tool(
            "configure_references",
            json!({
                "doc_id": doc_id,
                "enabled": true,
                "title": "References",
                "style": "author_year",
                "page_break_before": true
            }),
        )
        .await?;
    assert_eq!(
        refs_receipt["references"]["style"].as_str(),
        Some("author_year")
    );

    let digest = lead
        .call_tool("get_document_digest", json!({"doc_id": doc_id}))
        .await?;
    let lead_revision = digest["digest"]["current_rev"].as_i64().unwrap();
    assert_eq!(lead_revision, 4, "create + cover + toc + references");

    // 3. Document SSE subscribes at revision 0 and must replay the setup revisions.
    let doc_sse = SseCollector::spawn(
        http.clone(),
        format!("{base}/api/docs/{doc_id}/events?sinceRev=0"),
    );
    doc_sse.wait_for_revision(1).await?;
    doc_sse.wait_for_revision(lead_revision).await?;

    // Ephemeral previews and presence are transient (no revision, no database write).
    let streaming_preview_id = unique("preview");
    for (seq, text) in [(1, "Revenue grew"), (2, "Revenue grew 18% year over year.")] {
        let preview_response = http
            .post(format!("{base}/api/docs/{doc_id}/ephemeral"))
            .json(&json!({
                "kind": "preview",
                "doc_id": "forged-doc",
                "preview_id": streaming_preview_id,
                "actor": "sub-agent-a",
                "anchor": {"type": "after_block", "block_id": "ch1_placeholder"},
                "status": "streaming",
                "seq": seq,
                "block": {
                    "block_id": "ch1_preview",
                    "block_type": "paragraph",
                    "text": text
                },
                "received_at": "2000-01-01T00:00:00Z"
            }))
            .send()
            .await?;
        assert_eq!(preview_response.status().as_u16(), 202);
    }
    let invalid_preview = http
        .post(format!("{base}/api/docs/{doc_id}/ephemeral"))
        .json(&json!({"kind": "preview", "preview_id": "incomplete"}))
        .send()
        .await?;
    assert_eq!(invalid_preview.status().as_u16(), 400);
    assert_eq!(
        invalid_preview.json::<Value>().await?["error"]["code"],
        "INVALID_REQUEST"
    );
    let presence_response = http
        .post(format!("{base}/api/docs/{doc_id}/ephemeral"))
        .json(&json!({
            "kind": "presence",
            "actor": "sub-agent-a",
            "status": "writing",
            "section_id": "ch1_placeholder"
        }))
        .send()
        .await?;
    assert_eq!(presence_response.status().as_u16(), 202);

    // Both streamed increments must be visible before the commit, so the SSE frame order below
    // proves the frontend can replace the placeholder in the same frame as the revision.
    doc_sse
        .wait_for_frame(|frame| {
            frame.event.as_deref() == Some("ephemeral_preview")
                && json_string_field(&frame.data, "preview_id").as_deref()
                    == Some(streaming_preview_id.as_str())
                && json_string_field(&frame.data, "status").as_deref() == Some("streaming")
                && json_u64_field(&frame.data, "seq") == Some(2)
        })
        .await?;
    let preview_frames = doc_sse.snapshot().await;
    let first_preview = preview_frames
        .iter()
        .find(|frame| {
            frame.event.as_deref() == Some("ephemeral_preview")
                && json_string_field(&frame.data, "preview_id").as_deref()
                    == Some(streaming_preview_id.as_str())
                && json_u64_field(&frame.data, "seq") == Some(1)
        })
        .ok_or_else(|| anyhow!("first streaming preview missing from SSE frames"))?;
    assert_eq!(
        json_string_field(&first_preview.data, "doc_id"),
        Some(doc_id.clone()),
        "server must override the producer-supplied doc_id"
    );
    assert_eq!(
        json_u64_field(&first_preview.data, "ttl_ms"),
        Some(15_000),
        "server must default ttl_ms"
    );
    assert_ne!(
        json_string_field(&first_preview.data, "received_at").as_deref(),
        Some("2000-01-01T00:00:00Z"),
        "server must override the producer-supplied received_at"
    );
    doc_sse
        .wait_for_frame(|frame| frame.event.as_deref() == Some("agent_presence"))
        .await?;

    // Committing the streamed block clears its preview in the same revision frame.
    let preview_commit = lead
        .call_tool(
            "apply_operations",
            json!({
                "doc_id": doc_id,
                "expected_revision": lead_revision,
                "summary": "commit streamed preview",
                "preview_ids": [streaming_preview_id],
                "ops": [{
                    "type": "insert_after",
                    "target_id": "ch1_placeholder",
                    "blocks": [{
                        "id": "ch1_preview",
                        "type": "paragraph",
                        "text": "Revenue grew 18% year over year."
                    }]
                }]
            }),
        )
        .await?;
    assert_eq!(preview_commit["committed"].as_bool(), Some(true));
    let preview_revision = preview_commit["new_revision"].as_i64().unwrap();
    assert!(preview_revision > lead_revision);

    let expected_preview_id = streaming_preview_id.clone();
    doc_sse
        .wait_for_frame(move |frame| {
            frame.event.as_deref() == Some("revision")
                && json_revision(&frame.data) == Some(preview_revision)
                && json_cleared_preview_ids(&frame.data)
                    .iter()
                    .any(|preview_id| preview_id == &expected_preview_id)
        })
        .await?;
    let frames = doc_sse.snapshot().await;
    let preview_index = frames
        .iter()
        .rposition(|frame| {
            frame.event.as_deref() == Some("ephemeral_preview")
                && json_string_field(&frame.data, "preview_id").as_deref()
                    == Some(streaming_preview_id.as_str())
        })
        .ok_or_else(|| anyhow!("streaming preview missing from SSE frames"))?;
    let revision_index = frames
        .iter()
        .position(|frame| {
            frame.event.as_deref() == Some("revision")
                && json_revision(&frame.data) == Some(preview_revision)
                && json_cleared_preview_ids(&frame.data)
                    .iter()
                    .any(|preview_id| preview_id == &streaming_preview_id)
        })
        .ok_or_else(|| anyhow!("revision with cleared_preview_ids missing from SSE frames"))?;
    assert!(
        preview_index < revision_index,
        "preview frames must arrive before the clearing revision"
    );

    // A no-op commit creates no revision, so previews are cleared by an immediate
    // `ephemeral_clear` frame instead of waiting for ttl_ms to expire.
    let noop_preview_id = unique("preview_noop");
    let noop_preview_response = http
        .post(format!("{base}/api/docs/{doc_id}/ephemeral"))
        .json(&json!({
            "kind": "preview",
            "preview_id": noop_preview_id,
            "actor": "sub-agent-a",
            "anchor": {"type": "append_to_doc"},
            "status": "streaming",
            "block": {
                "block_id": "ch1_noop_preview",
                "block_type": "paragraph",
                "text": "Transient text that never commits"
            }
        }))
        .send()
        .await?;
    assert_eq!(noop_preview_response.status().as_u16(), 202);
    let expected_noop_preview_id = noop_preview_id.clone();
    doc_sse
        .wait_for_frame(move |frame| {
            frame.event.as_deref() == Some("ephemeral_preview")
                && json_string_field(&frame.data, "preview_id").as_deref()
                    == Some(expected_noop_preview_id.as_str())
        })
        .await?;

    let noop_commit = lead
        .call_tool(
            "apply_operations",
            json!({
                "doc_id": doc_id,
                "expected_revision": preview_revision,
                "summary": "no-op preview cleanup",
                "ops": [],
                "preview_ids": [noop_preview_id]
            }),
        )
        .await?;
    assert_eq!(
        noop_commit["new_revision"].as_i64(),
        Some(preview_revision),
        "a no-op commit must not create a revision"
    );
    assert!(
        noop_commit["_next_hint"]
            .as_str()
            .is_some_and(|hint| hint.contains("no-op")),
        "unexpected no-op hint: {noop_commit}"
    );

    doc_sse
        .wait_for_frame(|frame| {
            frame.event.as_deref() == Some("ephemeral_clear")
                && json_string_array_field(&frame.data, "preview_ids")
                    .iter()
                    .any(|preview_id| preview_id == &noop_preview_id)
        })
        .await?;
    let noop_frames = doc_sse.snapshot().await;
    assert!(
        !noop_frames.iter().any(|frame| {
            frame.event.as_deref() == Some("revision")
                && json_revision(&frame.data).is_some_and(|revision| revision > preview_revision)
        }),
        "a no-op commit must not emit a revision delta"
    );
    assert!(
        !noop_frames.iter().any(|frame| {
            frame.event.as_deref() == Some("revision")
                && json_cleared_preview_ids(&frame.data)
                    .iter()
                    .any(|preview_id| preview_id == &noop_preview_id)
        }),
        "a no-op commit must clear previews through ephemeral_clear, not a revision"
    );

    // 4a. Sub-Agent A: offline draft flow.
    let draft = json!({
        "title": "Chapter 1 — Financial Performance",
        "cover": {"enabled": true, "title": "Chapter 1 — Financial Performance"},
        "sources": {
            "sec_fin": {
                "title": "Audited financial statements",
                "url": "https://example.com/financials",
                "authors": ["Grace Hopper"],
                "date": "2026-02-01"
            },
            "sec_market": {
                "title": "Market Outlook 2026",
                "url": "https://example.com/market",
                "authors": ["Katherine Johnson"],
                "date": "2026-01-15"
            }
        },
        "blocks": [
            {"id":"ch1_intro","type":"heading","level":2,"text":"Financial Performance"},
            {"id":"ch1_summary","type":"paragraph","text":"Revenue grew **18%** year over year [^sec_fin], while enterprise demand remained resilient [^sec_market].","source_ids":["sec_fin"]},
            {
                "id":"ch1_financials",
                "type":"table",
                "caption":"Revenue by segment",
                "columns":[
                    {"key":"segment","label":"Segment"},
                    {"key":"revenue","label":"Revenue","children":[
                        {"key":"fy25","label":"FY25","data_type":"currency","sizing":"grow"},
                        {"key":"fy26","label":"FY26","data_type":"currency","sizing":"grow"}
                    ]}
                ],
                "rows":[
                    {"segment":"Enterprise","fy25":1200000,"fy26":1450000},
                    {"segment":"SMB","fy25":600000,"fy26":720000}
                ],
                "summary_rows":[
                    {"segment":"Total","fy25":1800000,"fy26":2170000}
                ],
                "style":{"column_widths":{"segment":"28%"}}
            },
            {
                "id":"ch1_chart",
                "type":"echarts",
                "title":"Revenue and margin",
                "height":340,
                "mode":"preset",
                "preset":"bar",
                "option_patch":{
                    "xAxis":{"data":["FY25","FY26"]},
                    "yAxis":[
                        {"type":"value","name":"Revenue"},
                        {"type":"value","name":"Margin","max":100}
                    ],
                    "series":[
                        {"name":"Revenue","type":"bar","data":[1800000,2170000]},
                        {"name":"Margin","type":"line","yAxisIndex":1,"data":[61.2,63.4]}
                    ]
                },
                "fallback_summary":"Revenue rose from 1.8M to 2.17M while margin improved from 61.2% to 63.4%."
            }
        ]
    });
    let draft_path = workspace.path().join("ch1_draft.json");
    tokio::fs::write(&draft_path, serde_json::to_vec_pretty(&draft)?).await?;

    let a_client = McpClient::connect(http.clone(), &base, &session_id, "sub-agent-a").await?;
    let b_client = McpClient::connect(http.clone(), &base, &session_id, "sub-agent-b").await?;
    let doc_for_a = doc_id.clone();
    let doc_for_b = doc_id.clone();
    let revision_for_b = lead_revision;

    // 4b. Parallel sub-agents: A projects/merges the draft while B concurrently upserts
    // duplicate-URL sources and appends Chapter 2 with a CustomHtml KPI card.
    let agent_a = tokio::spawn(async move {
        let markdown = a_client
            .call_tool(
                "read_as_markdown",
                json!({
                    "draft_file_path": "ch1_draft.json",
                    "include_chart_data": true
                }),
            )
            .await?;
        let markdown_text = markdown["markdown"].as_str().unwrap();
        assert!(markdown_text.contains("<!-- block:ch1_summary | type:paragraph -->"));
        assert!(markdown_text.contains("Revenue / FY25"));
        assert!(markdown_text.contains("| FY25 | 1800000 |"));
        let merged = a_client
            .call_tool(
                "apply_draft",
                json!({
                    "draft_file_path": "ch1_draft.json",
                    "target_doc_id": doc_for_a,
                    "mode": "replace_range",
                    "start_block_id": "ch1_placeholder",
                    "end_block_id": "ch1_placeholder"
                }),
            )
            .await?;
        Ok::<_, anyhow::Error>((merged, markdown_text.to_string()))
    });

    let agent_b = tokio::spawn(async move {
        // Register sources before appending citations so post-commit lint never sees a
        // transient dangling source. Agent A still runs concurrently with this sequence.
        let upsert = b_client
            .call_tool(
                "upsert_sources",
                json!({
                    "doc_id": doc_for_b,
                    "sources": [
                        {
                            "id": "sec_market",
                            "title": "Market Outlook 2026",
                            "url": "https://example.com/market",
                            "authors": ["Katherine Johnson"],
                            "date": "2026-01-15"
                        },
                        {
                            "id": "sec_market_duplicate",
                            "title": "Market Outlook 2026 (mirror)",
                            "url": "https://example.com/market/",
                            "authors": ["Katherine Johnson"],
                            "date": "2026-01-15"
                        }
                    ]
                }),
            )
            .await?;
        let ops = b_client
            .call_tool(
                "apply_operations",
                json!({
                    "doc_id": doc_for_b,
                    "expected_revision": revision_for_b,
                    "summary": "append chapter 2 market context",
                    "ops": [
                        {
                            "type": "insert_after",
                            "target_id": "ch2_placeholder",
                            "blocks": [
                                {
                                    "id": "ch2_summary",
                                    "type": "paragraph",
                                    "text": "Enterprise demand accelerated in Q4 [^sec_market].",
                                    "source_ids": ["sec_market"]
                                },
                                {
                                    "id": "ch2_kpi",
                                    "type": "custom_html",
                                    "html": "<div class=\"kpi-grid\"><strong>+18%</strong> enterprise growth</div>",
                                    "css": ".kpi-grid { border: 1px solid #ddd; padding: 1rem; }",
                                    "fallback_text": "Enterprise growth accelerated to 18% in Q4."
                                }
                            ]
                        }
                    ]
                }),
            )
            .await?;
        Ok::<_, anyhow::Error>((upsert, ops))
    });

    let (a_result, b_result) = tokio::join!(agent_a, agent_b);
    let (draft_receipt, offline_markdown) = a_result??;
    let (upsert_receipt, ops_receipt) = b_result??;
    assert!(draft_receipt["new_revision"].as_i64().unwrap() >= lead_revision);
    assert_eq!(draft_receipt["committed"].as_bool(), Some(true));
    assert!(draft_receipt["base_revision"].is_number());
    assert!(draft_receipt["rebased"].is_boolean());
    assert!(draft_receipt["blocking_errors"].is_array());
    assert_eq!(ops_receipt["committed"].as_bool(), Some(true));
    assert!(ops_receipt["base_revision"].is_number());
    assert!(ops_receipt["rebased"].is_boolean());
    assert!(ops_receipt["rebased_over"].is_number());
    assert!(ops_receipt["blocking_errors"].is_array());
    // Compact apply receipts stay well below ~100 tokens (~4 bytes/token).
    assert!(
        draft_receipt.to_string().len() < 600,
        "draft receipt is not compact: {draft_receipt}"
    );
    assert!(
        ops_receipt.to_string().len() < 600,
        "operations receipt is not compact: {ops_receipt}"
    );
    assert!(offline_markdown.contains("_Chart data: Revenue, Margin (bar, line)_"));
    assert!(offline_markdown.contains("| Category | Revenue | Margin |"));
    assert!(offline_markdown.contains("| FY25 | 1800000 | 61.2 |"));
    assert_eq!(
        upsert_receipt["canonical_id_map"]["sec_market_duplicate"].as_str(),
        Some("sec_market")
    );
    assert!(ops_receipt["new_revision"].as_i64().unwrap() > lead_revision);

    // 5. Reviewer reads the unified report.
    let reviewer = McpClient::connect(http.clone(), &base, &session_id, "reviewer-agent").await?;
    let review = reviewer
        .call_tool(
            "read_as_markdown",
            json!({"doc_id": doc_id, "include_chart_data": true}),
        )
        .await?;
    let review_markdown = review["markdown"].as_str().unwrap();
    assert!(review_markdown.contains("<!-- block:ch1_summary | type:paragraph -->"));
    assert!(review_markdown.contains("<!-- block:ch2_kpi | type:custom_html -->"));
    assert!(review_markdown.contains("Revenue / FY25"));
    assert!(review_markdown.contains("_Chart data: Revenue, Margin (bar, line)_"));
    assert!(review_markdown.contains("_Chart data: Dataset 1 (dataset)_"));
    assert!(review_markdown.contains("| Quarter | Revenue | Margin |"));
    assert!(review_markdown.contains("| Q1 | 120 | 62.5 |"));

    let focused = reviewer
        .call_tool(
            "read_blocks",
            json!({"doc_id": doc_id, "block_ids": ["ch1_financials"]}),
        )
        .await?;
    assert_eq!(focused["blocks"][0]["type"].as_str(), Some("table"));
    assert_eq!(focused["blocks"][0]["id"].as_str(), Some("ch1_financials"));
    let filtered = reviewer
        .call_tool(
            "get_document_digest",
            json!({"doc_id": doc_id, "query": "ch2"}),
        )
        .await?;
    assert!(
        filtered["digest"]["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|block| block["id"].as_str().unwrap().contains("ch2"))
    );

    // 5b. ECharts 6 structural errors are rejected by lint with actionable codes.
    let digest = reviewer
        .call_tool("get_document_digest", json!({"doc_id": doc_id}))
        .await?;
    let revision_before_bad_chart = digest["digest"]["current_rev"].as_i64().unwrap();
    let bad_chart_receipt = reviewer
        .call_tool(
            "apply_operations",
            json!({
                "doc_id": doc_id,
                "expected_revision": revision_before_bad_chart,
                "summary": "append invalid chart for lint QA",
                "ops": [{
                    "type": "append",
                    "blocks": [{
                        "id": "b_bad_chart",
                        "type": "echarts",
                        "title": "Invalid chart",
                        "height": 300,
                        "mode": "option",
                        "option": {
                            "xAxis": {},
                            "series": [
                                {"type": "notARealType", "xAxisIndex": 4, "formatter": "function(){ return 1; }"},
                                {"type": "custom", "coordinateSystem": "matrix"}
                            ]
                        },
                        "fallback_summary": "Invalid chart used for lint verification."
                    }]
                }]
            }),
        )
        .await?;
    assert_eq!(bad_chart_receipt["committed"].as_bool(), Some(true));
    assert_eq!(
        bad_chart_receipt["base_revision"].as_i64(),
        Some(revision_before_bad_chart)
    );
    let blocking_errors = bad_chart_receipt["blocking_errors"].as_array().unwrap();
    assert!(
        blocking_errors
            .iter()
            .all(|diagnostic| diagnostic["block_id"] == "b_bad_chart"),
        "blocking errors must be scoped to the written block: {blocking_errors:?}"
    );
    for code in [
        "ECHARTS_JS_FUNCTION_STRING",
        "ECHARTS_SERIES_TYPE_INVALID",
        "ECHARTS_MATRIX_MISSING",
        "ECHARTS_INDEX_OUT_OF_RANGE",
        "ECHARTS_DATA_MISSING",
    ] {
        assert!(
            blocking_errors
                .iter()
                .any(|diagnostic| diagnostic["code"] == code),
            "missing {code} in blocking_errors: {blocking_errors:?}"
        );
    }
    let bad_chart_lint = reviewer
        .call_tool("lint_document", json!({"doc_id": doc_id}))
        .await?;
    let bad_chart_codes: Vec<&str> = bad_chart_lint["report"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|diagnostic| diagnostic["code"].as_str())
        .collect();
    for code in [
        "ECHARTS_JS_FUNCTION_STRING",
        "ECHARTS_SERIES_TYPE_INVALID",
        "ECHARTS_MATRIX_MISSING",
        "ECHARTS_INDEX_OUT_OF_RANGE",
        "ECHARTS_DATA_MISSING",
    ] {
        assert!(
            bad_chart_codes.contains(&code),
            "missing {code}: {bad_chart_codes:?}"
        );
    }
    let digest = reviewer
        .call_tool("get_document_digest", json!({"doc_id": doc_id}))
        .await?;
    let revision_before_bad_chart_delete = digest["digest"]["current_rev"].as_i64().unwrap();
    reviewer
        .call_tool(
            "apply_operations",
            json!({
                "doc_id": doc_id,
                "expected_revision": revision_before_bad_chart_delete,
                "summary": "remove invalid chart after lint QA",
                "ops": [{"type": "delete_blocks", "block_ids": ["b_bad_chart"]}]
            }),
        )
        .await?;

    // 6. Inject a dangling citation, lint with self-healing hint, then fix it.
    let digest = reviewer
        .call_tool("get_document_digest", json!({"doc_id": doc_id}))
        .await?;
    let revision_before_lint = digest["digest"]["current_rev"].as_i64().unwrap();
    reviewer
        .call_tool(
            "apply_operations",
            json!({
                "doc_id": doc_id,
                "expected_revision": revision_before_lint,
                "summary": "inject dangling citation for QA",
                "ops": [{
                    "type":"patch_block",
                    "block_id":"ch2_summary",
                    "patch":{"text":"Enterprise demand accelerated in Q4 [^sec_market] and [^src_nonexistent]."}
                }]
            }),
        )
        .await?;
    let lint_bad = reviewer
        .call_tool("lint_document", json!({"doc_id": doc_id}))
        .await?;
    assert!(lint_bad["report"]["summary"]["errors"].as_u64().unwrap() >= 1);
    let dangling = lint_bad["report"]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| diagnostic["code"] == "DANGLING_SOURCE_ID")
        .ok_or_else(|| anyhow!("missing DANGLING_SOURCE_ID diagnostic"))?;
    let hint = dangling["hint"].as_str().unwrap();
    assert!(hint.contains("sec_market"));
    assert!(hint.contains("sec_fin"));

    let digest = reviewer
        .call_tool("get_document_digest", json!({"doc_id": doc_id}))
        .await?;
    let revision_for_fix = digest["digest"]["current_rev"].as_i64().unwrap();
    reviewer
        .call_tool(
            "apply_operations",
            json!({
                "doc_id": doc_id,
                "expected_revision": revision_for_fix,
                "summary": "fix dangling citation",
                "ops": [{
                    "type":"patch_block",
                    "block_id":"ch2_summary",
                    "patch":{"text":"Enterprise demand accelerated in Q4 [^sec_market]."}
                }]
            }),
        )
        .await?;
    let lint_clean = reviewer
        .call_tool("lint_document", json!({"doc_id": doc_id}))
        .await?;
    assert_eq!(lint_clean["report"]["summary"]["errors"].as_u64(), Some(0));

    // 7. Theme/CSS update is validated and revisioned.
    let style = reviewer
        .call_tool(
            "update_style_and_css",
            json!({
                "doc_id": doc_id,
                "preset": "editorial",
                "tokens_patch": {"--rpt-primary": "#0b3d91"},
                "custom_css": ".kpi-grid { letter-spacing: 0.01em; }",
                "css_mode": "append"
            }),
        )
        .await?;
    assert_eq!(style["theme"]["preset"].as_str(), Some("editorial"));
    assert_eq!(style["token_count"].as_u64(), Some(1));

    // 8. Revision rollback: bump cover, roll back, verify content and monotonic revision.
    let digest = reviewer
        .call_tool("get_document_digest", json!({"doc_id": doc_id}))
        .await?;
    let revision_before_rollback = digest["digest"]["current_rev"].as_i64().unwrap();
    let temp_cover = reviewer
        .call_tool(
            "set_cover",
            json!({
                "doc_id": doc_id,
                "cover_patch": {"title": "Temporary cover that must be rolled back"}
            }),
        )
        .await?;
    let bumped = temp_cover["revision"].as_i64().unwrap();
    assert_eq!(bumped, revision_before_rollback + 1);
    let rollback = reviewer
        .call_tool(
            "rollback_revision",
            json!({"doc_id": doc_id, "target_revision": revision_before_rollback}),
        )
        .await?;
    assert_eq!(
        rollback["new_revision"].as_i64(),
        Some(revision_before_rollback + 2)
    );
    assert_eq!(
        rollback["rolled_back_to"].as_i64(),
        Some(revision_before_rollback)
    );
    assert_eq!(rollback["replayed_revisions"].as_u64(), Some(1));
    assert!(
        rollback["summary"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert!(
        rollback.get("digest").is_none(),
        "rollback must stay compact"
    );

    // 9. Final frontend contract.
    let mut final_doc = http
        .get(format!("{base}/api/docs/{doc_id}"))
        .send()
        .await?
        .json::<Value>()
        .await?;
    let final_revision = final_doc["current_rev"].as_i64().unwrap();
    assert_eq!(final_revision, revision_before_rollback + 2);
    assert_eq!(final_doc["session_id"].as_str(), Some(session_id.as_str()));
    assert_eq!(final_doc["cover"]["title"].as_str(), Some(title));
    assert_eq!(final_doc["toc"]["variant"].as_str(), Some("numbered"));
    assert_eq!(
        final_doc["references"]["style"].as_str(),
        Some("author_year")
    );
    assert_eq!(final_doc["theme"]["preset"].as_str(), Some("editorial"));
    assert_eq!(
        final_doc["theme"]["tokens"]["--rpt-primary"].as_str(),
        Some("#0b3d91")
    );

    let sources = final_doc["sources"].as_object().unwrap();
    assert!(sources.contains_key("sec_fin"));
    assert!(sources.contains_key("sec_market"));
    assert!(!sources.contains_key("sec_market_duplicate"));
    assert_eq!(
        sources.len(),
        2,
        "duplicate source must be canonicalized: {sources:?}"
    );

    let dataset_chart = final_doc["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|block| block["id"] == "ch0_dataset")
        .ok_or_else(|| anyhow!("dataset chart missing from final document"))?;
    assert_eq!(
        dataset_chart["mode"].as_str(),
        Some("option"),
        "omitted mode must be inferred as option when option is supplied without preset"
    );
    let map_requirements = &final_doc["computed"]["chart_requirements"]["ch0_map"];
    let required_maps = map_requirements["required_maps"].as_array().unwrap();
    assert!(required_maps.iter().any(|value| value == "china"));
    assert!(required_maps.iter().any(|value| value == "world"));
    assert!(
        !final_doc["computed"]["chart_requirements"]["ch0_dataset"]
            .as_object()
            .is_some_and(|requirements| !requirements.is_empty()),
        "charts without maps/extensions must not emit chart requirements"
    );

    let block_ids: Vec<String> = final_doc["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|block| block["id"].as_str().unwrap().to_string())
        .collect();
    let position = |id: &str| block_ids.iter().position(|candidate| candidate == id);
    assert!(position("ch1_intro").unwrap() < position("ch2_placeholder").unwrap());
    assert!(position("ch2_summary").unwrap() > position("ch2_placeholder").unwrap());
    assert!(!block_ids.contains(&"ch1_placeholder".to_string()));

    let layout = &final_doc["computed"]["table_layouts"]["ch1_financials"];
    assert_eq!(layout["header_depth"], 2);
    assert_eq!(layout["total_width_pct"], 100.0);
    assert_eq!(layout["header_rows"][0][1]["colspan"], 2);
    assert_eq!(layout["header_rows"][1].as_array().unwrap().len(), 2);
    let width_sum: f64 = layout["leaf_columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|leaf| leaf["width_pct"].as_f64().unwrap())
        .sum();
    assert!((width_sum - 100.0).abs() < 0.01, "width_sum={width_sum}");
    assert!(final_doc["computed"]["citation_map"]["sec_market"]["number"].is_number());
    assert!(
        !final_doc["computed"]["toc_tree"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let revisions = http
        .get(format!("{base}/api/docs/{doc_id}/revisions?limit=100"))
        .send()
        .await?
        .json::<Value>()
        .await?;
    assert_eq!(revisions["current_rev"].as_i64(), Some(final_revision));
    assert!(revisions["revisions"].as_array().unwrap().len() >= 8);
    assert!(
        revisions["revisions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|revision| revision["actor"].as_str() == Some("reviewer-agent")),
        "X-Actor must be recorded on revisions"
    );

    // Wait for the SSE live stream to catch up, then assert gap-free revisions and no gaps.
    wait_until(Duration::from_secs(10), || {
        let doc_sse = &doc_sse;
        async move {
            doc_sse.snapshot().await.iter().any(|frame| {
                frame.event.as_deref() == Some("revision")
                    && json_revision(&frame.data) == Some(final_revision)
            })
        }
    })
    .await?;
    let frames = doc_sse.snapshot().await;
    assert!(
        !frames
            .iter()
            .any(|frame| frame.event.as_deref() == Some("sync_required")),
        "SSE requested an unexpected full resync"
    );
    let mut revisions_seen: Vec<i64> = frames
        .iter()
        .filter(|frame| frame.event.as_deref() == Some("revision"))
        .filter_map(|frame| json_revision(&frame.data))
        .collect();
    revisions_seen.sort_unstable();
    revisions_seen.dedup();
    assert_eq!(
        revisions_seen,
        (1..=final_revision).collect::<Vec<_>>(),
        "SSE revisions must be contiguous"
    );

    let session_frames = session_sse.snapshot().await;
    assert!(
        session_frames
            .iter()
            .any(|frame| frame.event.as_deref() == Some("doc_created")),
        "session SSE must notify doc_created"
    );

    let list = http
        .get(format!("{base}/api/docs?query={doc_id}&limit=10"))
        .send()
        .await?
        .json::<Value>()
        .await?;
    assert_eq!(list["total"].as_i64(), Some(1));
    assert_eq!(list["reports"][0]["doc_id"].as_str(), Some(doc_id.as_str()));

    let lint_final = reviewer
        .call_tool("lint_document", json!({"doc_id": doc_id}))
        .await?;
    assert_eq!(lint_final["report"]["summary"]["errors"].as_u64(), Some(0));

    // 10. apply_draft honors explicit expected_revision (no silent retry) but can auto-read
    // the current revision when expected_revision is omitted.
    let digest = reviewer
        .call_tool("get_document_digest", json!({"doc_id": doc_id}))
        .await?;
    let draft_base_revision = digest["digest"]["current_rev"].as_i64().unwrap();
    reviewer
        .call_tool(
            "apply_operations",
            json!({
                "doc_id": doc_id,
                "expected_revision": draft_base_revision,
                "summary": "advance revision before draft CAS test",
                "ops": [{
                    "type": "patch_block",
                    "block_id": "ch2_summary",
                    "patch": {"text": "Enterprise demand accelerated in Q4 [^sec_market]. Draft CAS sentinel."}
                }]
            }),
        )
        .await?;
    let draft_path = workspace.path().join("draft_cas_conflict.json");
    let draft = json!({
        "blocks": [{
            "id": "draft_cas_replacement",
            "type": "paragraph",
            "text": "Draft CAS replacement paragraph."
        }]
    });
    tokio::fs::write(&draft_path, serde_json::to_vec_pretty(&draft)?).await?;
    let draft_path = draft_path.to_string_lossy().to_string();
    let explicit_error = reviewer
        .call_tool_error(
            "apply_draft",
            json!({
                "draft_file_path": draft_path,
                "target_doc_id": doc_id,
                "mode": "replace_range",
                "start_block_id": "ch2_summary",
                "end_block_id": "ch2_summary",
                "expected_revision": draft_base_revision
            }),
        )
        .await?;
    assert_eq!(
        explicit_error["data"]["code"].as_str(),
        Some("REVISION_CONFLICT"),
        "explicit expected_revision must return the conflict: {explicit_error}"
    );
    let draft_receipt = reviewer
        .call_tool(
            "apply_draft",
            json!({
                "draft_file_path": draft_path,
                "target_doc_id": doc_id,
                "mode": "replace_range",
                "start_block_id": "ch2_summary",
                "end_block_id": "ch2_summary"
            }),
        )
        .await?;
    assert_eq!(draft_receipt["committed"].as_bool(), Some(true));
    assert_eq!(draft_receipt["rebased"].as_bool(), Some(false));
    let lint_after_draft = reviewer
        .call_tool("lint_document", json!({"doc_id": doc_id}))
        .await?;
    assert_eq!(
        lint_after_draft["report"]["summary"]["errors"].as_u64(),
        Some(0)
    );
    tokio::fs::remove_file(&draft_path).await?;

    final_doc["_test"] = json!({"offline_markdown_chars": offline_markdown.chars().count()});
    println!(
        "E2E complete: doc={doc_id} revisions=1..={final_revision} blocks={}",
        block_ids.len()
    );
    Ok(())
}
