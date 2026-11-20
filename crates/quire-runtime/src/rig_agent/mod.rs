//! rig-core tool adaptation, MCP HTTP transport and the main agent turn loop.

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use futures::StreamExt;
use quire_session_protocol::{ContextUsage, MessageRole, QuestionItem};
use rig_core::{
    completion::{
        AssistantContent, CompletionRequest, CompletionResponse, Message, ToolDefinition, Usage,
        message::ToolResult as MessageToolResult,
    },
    message::{AssistantMessage, ImageMediaType, ToolCall, UserContent},
    streaming::{Item, StreamEvent},
    tool::{DynamicTool, ToolExecutionError, ToolOutput, ToolResult as ExecutedToolResult},
};
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::{
    comment_refs::render_stored_user_message,
    config::Config,
    error::{RuntimeError, RuntimeResult},
    kb::KbService,
    mcp_client::RmcpClient,
    mcp_registry::McpRegistry,
    model_stream::{ModelEventStream, ModelStreamEvent, ModelStreamProvider, ModelTurnRequest},
    models::{ModelRegistry, ProviderProtocol},
    prompt::ContextMessage,
    sandbox::{SandboxLease, SandboxProvider, sandbox_spec_from_config},
    service::PendingQueueCoordinator,
};

mod builtin_tools;
mod preview_stream;
mod subagents;

pub use builtin_tools::BUILTIN_TOOLS;
pub use preview_stream::{PreviewThrottle, ToolPreviewStreamer};
pub use subagents::{SubagentOutcome, SubagentTask, spawn_subagents};

use builtin_tools::{ToolBuildContext, warn_missing_allowed_tools};
use preview_stream::{PreviewState, inject_preview_ids};

const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
struct PreviewImage {
    base64: String,
    mime_type: String,
    caption: String,
}

#[derive(Clone)]
pub struct RigModelStreamProvider {
    core: Arc<RigModelCore>,
}

impl RigModelStreamProvider {
    pub fn new(
        kb: KbService,
        models: Arc<ModelRegistry>,
        config: Arc<Config>,
        mcp: Arc<McpRegistry>,
        sandbox_provider: Arc<dyn SandboxProvider>,
        queue: PendingQueueCoordinator,
    ) -> Self {
        Self {
            core: Arc::new(RigModelCore {
                kb,
                models,
                config,
                mcp,
                sandbox_provider,
                queue,
            }),
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
    mcp: Arc<McpRegistry>,
    sandbox_provider: Arc<dyn SandboxProvider>,
    queue: PendingQueueCoordinator,
}

#[derive(Clone)]
struct AgentExecution {
    agent_id: String,
    model_route: String,
    allowed_tools: Vec<String>,
    disable_tools: bool,
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
        let allowed_tools = if request.disable_tools {
            Vec::new()
        } else if request.allowed_tools.is_empty() {
            agent.allowed_tools.clone()
        } else {
            request.allowed_tools.clone()
        };
        let messages = messages_from_request(&request);
        let execution = AgentExecution {
            agent_id: request.agent_id,
            model_route,
            allowed_tools,
            disable_tools: request.disable_tools,
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
        mut execution: AgentExecution,
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
        let max_tokens = match resolved
            .max_output_tokens
            .and_then(|value| u64::try_from(value).ok())
        {
            Some(value) => Some(value),
            None if resolved.protocol == ProviderProtocol::Anthropic => Some(8192),
            None => None,
        };
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
        let supports_vision = resolved.supports_vision;
        let pending_preview_images: Arc<Mutex<Vec<PreviewImage>>> =
            Arc::new(Mutex::new(Vec::new()));
        let doc_id_state = Arc::new(Mutex::new(execution.doc_id.clone()));
        let mut sandbox_spec = sandbox_spec_from_config(&self.config.sandbox);
        sandbox_spec
            .metadata
            .insert("quire_run_id".into(), execution.run_id.clone());
        sandbox_spec
            .metadata
            .insert("quire_session_id".into(), execution.session_id.clone());
        let sandbox_lease = Arc::new(SandboxLease::new(
            Arc::clone(&self.sandbox_provider),
            sandbox_spec,
        ));
        let mut tools = self.build_tools(ToolBuildContext {
            execution: execution.clone(),
            scope: scope.clone(),
            engine: Arc::clone(&self),
            pending_preview_images: Arc::clone(&pending_preview_images),
            supports_vision,
            sandbox: Arc::clone(&sandbox_lease),
            doc_id: Arc::clone(&doc_id_state),
        })?;
        for binding in self.mcp.tools_for_agent(&execution.allowed_tools).await? {
            let allowed: Vec<String> = binding
                .tool_names
                .iter()
                .filter(|name| !BUILTIN_TOOLS.contains(&name.as_str()))
                .cloned()
                .collect();
            if allowed.is_empty() {
                continue;
            }
            match RmcpClient::connect(
                &binding.url,
                binding.auth_token,
                &execution.session_id,
                &execution.agent_id,
            )
            .await
            {
                Ok(client) => match client.dynamic_tools(&allowed) {
                    Ok(mut remote) => tools.append(&mut remote),
                    Err(error) => {
                        warn!(
                            %error,
                            server_id = %binding.server_id,
                            server_name = %binding.name,
                            "skipping unavailable MCP server while assembling tools"
                        );
                    }
                },
                Err(error) => {
                    warn!(
                        %error,
                        server_id = %binding.server_id,
                        server_name = %binding.name,
                        "skipping unavailable MCP server while assembling tools"
                    );
                }
            }
        }
        let tool_map: BTreeMap<String, DynamicTool> = tools
            .into_iter()
            .map(|tool| (tool.name().as_str().to_string(), tool))
            .collect();
        warn_missing_allowed_tools(&execution.agent_id, &execution.allowed_tools, &tool_map);
        let mut last_usage: Usage;
        let mut final_text = String::new();
        let mut suspended = false;
        loop {
            if execution.cancellation.is_cancelled() {
                return Ok(final_text);
            }
            let definitions: Vec<ToolDefinition> =
                tool_map.values().map(|tool| tool.definition()).collect();
            let request = build_completion_request(&execution, &messages, definitions, max_tokens);
            if let Err(error) = model.check(&request) {
                warn!(
                    %error,
                    model = %execution.model_route,
                    "model request preflight reported unsupported options"
                );
            }
            let mut stream = model.stream(request).map_err(model_provider_error)?;
            let mut previews = PreviewState::new(&execution, &self.config);
            let mut streamed_text = false;
            loop {
                tokio::select! {
                    _ = execution.cancellation.cancelled() => {
                        return Ok(final_text);
                    }
                    item = tokio::time::timeout(STREAM_IDLE_TIMEOUT, stream.next()) => {
                        match item {
                            Ok(Some(Ok(Item::Event(event)))) => {
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
                            Ok(Some(Ok(Item::Unknown(_)))) => {}
                            Ok(Some(Err(error))) => return Err(model_provider_error(error)),
                            Ok(None) => break,
                            Err(_) => {
                                return Err(RuntimeError::Internal(format!(
                                    "model stream idle timeout after {}s",
                                    STREAM_IDLE_TIMEOUT.as_secs()
                                )));
                            }
                        }
                    }
                }
            }
            let response = stream.finish().await.map_err(model_provider_error)?;
            // Context occupancy is the final request's prompt size, not the
            // sum of every tool-round prompt. Keep the last call's usage so the
            // session meter reflects the live context window.
            last_usage = response.usage;
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
            if execution.disable_tools {
                final_text = text;
                break;
            }
            if calls.is_empty() {
                final_text = text;
                break;
            }
            messages.push(Message::Assistant(AssistantMessage::new(
                response.choice.clone(),
            )));
            let mut answers: Vec<MessageToolResult> = Vec::new();
            for call in calls {
                if !execution.run_id.starts_with("run_sub_")
                    && call.function.name.as_str() == "ask_user"
                {
                    request_questionnaire(&scope, &execution, call).await?;
                    suspended = true;
                    break;
                }
                let (answer, linked_doc) =
                    execute_tool_call(&tool_map, &scope, &execution, &mut previews, call).await;
                if let Some((doc_id, title)) = linked_doc
                    && !execution.run_id.starts_with("run_sub_")
                {
                    if let Err(error) = self
                        .kb
                        .store()
                        .bind_doc(&execution.session_id, &doc_id)
                        .await
                    {
                        tracing::warn!(%error, %doc_id, session_id = %execution.session_id, "failed to bind lazily created report");
                    } else {
                        execution.doc_id = Some(doc_id.clone());
                        *doc_id_state.lock().await = Some(doc_id.clone());
                        scope.send(ModelStreamEvent::DocLinked { doc_id, title });
                    }
                }
                answers.push(answer);
            }
            if suspended {
                break;
            }
            messages.push(Message::tool_results(answers));
            let preview_images = {
                let mut guard = pending_preview_images.lock().await;
                std::mem::take(&mut *guard)
            };
            if !preview_images.is_empty() {
                let mut parts = Vec::new();
                for image in preview_images {
                    if !image.caption.trim().is_empty() {
                        parts.push(UserContent::text(image.caption));
                    }
                    let media_type = match image.mime_type.as_str() {
                        "image/jpeg" => ImageMediaType::JPEG,
                        "image/webp" => ImageMediaType::WEBP,
                        _ => ImageMediaType::PNG,
                    };
                    parts.push(UserContent::image_base64(
                        image.base64,
                        Some(media_type),
                        None,
                    ));
                }
                if !parts.is_empty() {
                    messages.push(Message::User { content: parts });
                }
            }
            if !execution.run_id.starts_with("run_sub_")
                && let Some(batch) = self
                    .queue
                    .take_pending_batch(&execution.session_id, &execution.run_id)
                    .await?
            {
                // Route the event through the ordered stream so it is persisted after the tool
                // result that preceded it, never before.
                scope.send(ModelStreamEvent::UserMessageInjected {
                    message_id: batch.message.message_id.clone(),
                    content: batch.message.content.clone(),
                    created_at: batch.message.created_at,
                });
                messages.push(Message::user(render_stored_user_message(&batch.message)));
            }
        }
        if let Err(error) = sandbox_lease.close().await {
            warn!(%error, run_id = %execution.run_id, "sandbox cleanup failed; provider TTL remains the backstop");
        }
        if execution.cancellation.is_cancelled() {
            return Ok(final_text);
        }
        scope.send(ModelStreamEvent::Usage(context_usage(
            &last_usage,
            context_limit,
        )));
        Ok(final_text)
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
        MessageRole::Compaction => Message::user(message.content.clone()),
        MessageRole::User | MessageRole::Tool => Message::user(message.content.clone()),
    }
}

fn build_completion_request(
    execution: &AgentExecution,
    messages: &[Message],
    tools: Vec<ToolDefinition>,
    max_tokens: Option<u64>,
) -> CompletionRequest {
    let mut conversation = messages.to_vec();
    let prompt = conversation
        .pop()
        .unwrap_or_else(|| Message::user(execution.system_prompt.clone()));
    let mut request = CompletionRequest::new(prompt)
        .preamble(runtime_preamble(execution))
        .tools(tools)
        .max_tokens(max_tokens);
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
    } else if !execution.run_id.starts_with("run_sub_") {
        preamble.push_str(
            "No Quire report is bound to this session yet. If the user asks to write or generate a report, choose an appropriate formal report title and call `create_report` first; for pure Q&A, reply directly without creating a report.\n",
        );
    }
    if !execution.run_id.starts_with("run_sub_") {
        preamble.push_str(
            "When the topic of this conversation is clear (especially on the first turn), call `set_session_title` with a concise title.\n",
        );
    }
    preamble.push_str(
        "For complex ECharts charts, call `preview_echarts` first with the complete native ECharts 6.0 `option`. Inspect the rendered screenshot and `lint_issues`, refine with `based_on_preview_id` plus a small `option_patch` only (never resend the full option), and commit the approved chart with `commit_chart` using the final `preview_id`. Do not write chart blocks directly through apply_operations when the preview workflow is available.\n",
    );
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

async fn request_questionnaire(
    scope: &EventScope,
    execution: &AgentExecution,
    call: ToolCall,
) -> RuntimeResult<()> {
    let provider_call_id = call.id.wire().to_string();
    let tool_call_id = format!("{}:{}", execution.run_id, provider_call_id);
    let arguments = call.function.arguments_value();
    let (input_preview, input_truncated) = preview_json(&arguments);
    scope.send(ModelStreamEvent::ToolCallStarted {
        tool_call_id: tool_call_id.clone(),
        tool_name: "ask_user".into(),
        input_preview,
        truncated: input_truncated,
        input: arguments.clone(),
    });
    let questions_value = arguments.get("questions").cloned().ok_or_else(|| {
        RuntimeError::InvalidRequest("ask_user requires a `questions` array".into())
    })?;
    let questions: Vec<QuestionItem> =
        serde_json::from_value(questions_value).map_err(|error| {
            RuntimeError::InvalidRequest(format!("invalid ask_user questions: {error}"))
        })?;
    if questions.is_empty() {
        return Err(RuntimeError::InvalidRequest(
            "ask_user requires at least one question".into(),
        ));
    }
    scope.send(ModelStreamEvent::QuestionnaireRequested {
        tool_call_id: Some(tool_call_id.clone()),
        questions,
    });
    scope.send(ModelStreamEvent::ToolCallFinished {
        tool_call_id,
        tool_name: "ask_user".into(),
        status: "awaiting_input".into(),
        output_preview: "{\"awaiting_input\":true}".into(),
        truncated: false,
        duration_ms: 0,
        input: arguments,
        output: json!({"awaiting_input": true}),
        error: None,
    });
    Ok(())
}

fn format_invalid_tool_arguments_error(invalid: &str) -> String {
    match serde_json::from_str::<Value>(invalid) {
        Err(err) => {
            format!("invalid JSON in tool arguments (fix syntax and retry): {err}")
        }
        Ok(_) => format!("tool arguments must be a JSON object, got: {invalid}"),
    }
}

async fn execute_tool_call(
    tool_map: &BTreeMap<String, DynamicTool>,
    scope: &EventScope,
    execution: &AgentExecution,
    previews: &mut PreviewState,
    call: ToolCall,
) -> (MessageToolResult, Option<(String, String)>) {
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
        let message = format_invalid_tool_arguments_error(&invalid);
        let model_output = json!({
            "kind": "InvalidArgs",
            "error": message.clone(),
            "raw_arguments": invalid,
        });
        Err(ToolExecutionError::invalid_args(message)
            .with_model_output(ToolOutput::json(model_output)))
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
    let linked_doc = if error.is_none() && tool_name == "create_report" {
        output_value
            .get("doc_id")
            .and_then(Value::as_str)
            .map(|doc_id| {
                let title = output_value
                    .get("title")
                    .and_then(Value::as_str)
                    .or_else(|| arguments.get("title").and_then(Value::as_str))
                    .unwrap_or("未命名报告");
                (doc_id.to_string(), title.to_string())
            })
    } else {
        None
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
    (call.answer(&result), linked_doc)
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

#[cfg(test)]
mod tests {
    use super::*;
    use rig_core::message::{ToolFunction, ToolName};
    use std::sync::atomic::{AtomicUsize, Ordering};

    async fn execute_invalid_arguments_call(raw: &str) -> (MessageToolResult, Value, usize) {
        let calls = Arc::new(AtomicUsize::new(0));
        let tool_calls = Arc::clone(&calls);
        let tool = DynamicTool::new(
            ToolName::new("probe").unwrap(),
            "Probe used to prove invalid arguments never execute.",
            json!({}),
            move |_args| {
                let tool_calls = Arc::clone(&tool_calls);
                Box::pin(async move {
                    tool_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(ToolOutput::text("executed"))
                })
            },
        );
        let mut tool_map = BTreeMap::new();
        tool_map.insert("probe".to_string(), tool);

        let (sender, mut receiver) = mpsc::unbounded_channel();
        let scope = EventScope::top(sender, CancellationToken::new());
        let execution = AgentExecution {
            agent_id: "main".into(),
            model_route: "stub/echo".into(),
            allowed_tools: vec!["probe".into()],
            disable_tools: false,
            system_prompt: "test".into(),
            kb_branch: Some("main".into()),
            doc_id: None,
            session_id: "sess_test".into(),
            run_id: "run_test".into(),
            cancellation: CancellationToken::new(),
        };
        let mut previews = PreviewState::for_test(&execution.run_id);
        let call = ToolCall::from_wire(
            "call_invalid",
            ToolFunction::parse(ToolName::new("probe").unwrap(), raw),
        );

        let (result, linked_doc) =
            execute_tool_call(&tool_map, &scope, &execution, &mut previews, call).await;
        assert!(linked_doc.is_none());

        let mut finished_output = Value::Null;
        while let Ok(event) = receiver.try_recv() {
            if let Ok(ModelStreamEvent::ToolCallFinished { output, .. }) = event {
                finished_output = output;
            }
        }
        (result, finished_output, calls.load(Ordering::SeqCst))
    }

    #[tokio::test]
    async fn invalid_tool_arguments_never_execute_the_tool() {
        for raw in [r#"{"total":"13%}"#, r#"["a"]"#] {
            let (result, finished_output, calls) = execute_invalid_arguments_call(raw).await;
            assert_eq!(calls, 0);
            assert!(result.is_error);

            let content = result
                .content
                .iter()
                .find_map(|entry| entry.as_json())
                .expect("structured invalid-arguments feedback");
            assert_eq!(
                content.get("kind").and_then(Value::as_str),
                Some("InvalidArgs")
            );
            assert_eq!(
                content.get("raw_arguments").and_then(Value::as_str),
                Some(raw)
            );
            assert!(
                content
                    .get("error")
                    .and_then(Value::as_str)
                    .is_some_and(|error| !error.trim().is_empty())
            );
            assert_eq!(
                finished_output.get("kind").and_then(Value::as_str),
                Some("InvalidArgs")
            );
        }
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

    #[tokio::test]
    async fn ask_user_emits_questionnaire_and_pauses_the_run() {
        use rig_core::message::{ToolFunction, ToolName};

        let (sender, mut receiver) = mpsc::unbounded_channel();
        let scope = EventScope::top(sender, CancellationToken::new());
        let execution = AgentExecution {
            agent_id: "main".into(),
            model_route: "stub/echo".into(),
            allowed_tools: vec!["ask_user".into()],
            disable_tools: false,
            system_prompt: "test".into(),
            kb_branch: Some("main".into()),
            doc_id: Some("report".into()),
            session_id: "sess_test".into(),
            run_id: "run_test".into(),
            cancellation: CancellationToken::new(),
        };
        let call = ToolCall::from_wire(
            "call_ask",
            ToolFunction::new(
                ToolName::new("ask_user").unwrap(),
                json!({
                    "questions": [{
                        "id": "scope",
                        "title": "Which scope?",
                        "multiple": false,
                        "required": true,
                        "options": [{"id": "q3", "label": "Q3"}]
                    }]
                }),
            ),
        );
        request_questionnaire(&scope, &execution, call)
            .await
            .unwrap();
        assert!(matches!(
            receiver.recv().await.unwrap().unwrap(),
            ModelStreamEvent::ToolCallStarted { .. }
        ));
        let requested = receiver.recv().await.unwrap().unwrap();
        match requested {
            ModelStreamEvent::QuestionnaireRequested { questions, .. } => {
                assert_eq!(questions.len(), 1);
                assert_eq!(
                    questions.first().map(|question| question.id.as_str()),
                    Some("scope")
                );
            }
            other => panic!("expected questionnaire, got {other:?}"),
        }
        assert!(matches!(
            receiver.recv().await.unwrap().unwrap(),
            ModelStreamEvent::ToolCallFinished {
                status,
                ..
            } if status == "awaiting_input"
        ));
    }
}
