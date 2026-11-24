//! Native Rig agent construction, MCP adaptation and AgentRunner orchestration.

use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, RwLock},
};

use futures::{StreamExt, future::join_all};
use quire_session_protocol::ContextUsage;
use rig::{
    agent::{AgentBuilder, MultiTurnStreamItem},
    completion::{Message, PromptError, Usage},
    tool::{
        ToolContext,
        rmcp::{McpMeta, rmcp::model::RequestMetaObject},
    },
};
use serde_json::Value;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::{
    config::Config,
    error::{RuntimeError, RuntimeResult},
    kb::KbService,
    mcp_registry::McpRegistry,
    model_stream::{
        ModelEventStream, ModelStreamEvent, ModelStreamProvider, ModelTurnRequest,
        spawn_producer_stream,
    },
    models::ModelRegistry,
    sandbox::{SandboxLease, SandboxProvider, sandbox_spec_from_config},
    service::PendingQueueCoordinator,
};

mod builtin_tools;
mod context;
mod hooks;
mod preview_stream;
mod subagents;
mod tool_schemas;

pub use builtin_tools::BUILTIN_TOOLS;
pub use preview_stream::{PreviewThrottle, ToolPreviewStreamer};
pub use subagents::{SubagentOutcome, SubagentTask};

use builtin_tools::{ToolBuildContext, warn_missing_allowed_tools};
use hooks::{
    ModelTurnPricing, RunTermination, SharedTurnState, ToolLifecycleHook, TurnControlHook,
    TurnRunState,
};
use preview_stream::PreviewState;

/// Maximum concurrent tool executions per model batch. Rig commits answers in
/// call order, but the tool bodies themselves may interleave.
const DEFAULT_TOOL_CONCURRENCY: usize = 8;

#[derive(serde::Serialize)]
struct QuireMcpMeta<'a> {
    session_id: &'a str,
    run_id: &'a str,
    agent_id: &'a str,
}

#[derive(serde::Serialize)]
struct TextToolOutput<'a> {
    text: &'a str,
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

impl ModelStreamProvider for RigModelStreamProvider {
    fn stream_turn(
        &self,
        request: ModelTurnRequest,
    ) -> Pin<Box<dyn Future<Output = RuntimeResult<ModelEventStream>> + Send + '_>> {
        let core = self.core.clone();
        Box::pin(async move {
            Ok(spawn_producer_stream(move |sender| async move {
                if let Err(error) = core.run_main_turn(request, sender.clone()).await {
                    let _ = sender.send(Err(error)).await;
                }
            }))
        })
    }

    fn commits_canonical_history(&self) -> bool {
        true
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
    system_prompt: String,
    kb_branch: Option<String>,
    doc_id: SharedDocId,
    session_id: String,
    run_id: String,
    cancellation: CancellationToken,
}

pub(super) type SharedDocId = Arc<RwLock<Option<String>>>;

pub(super) fn new_shared_doc_id(doc_id: Option<String>) -> SharedDocId {
    Arc::new(RwLock::new(doc_id))
}

pub(super) fn current_doc_id(doc_id: &SharedDocId) -> Option<String> {
    doc_id
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

pub(super) fn update_doc_id(doc_id: &SharedDocId, value: Option<String>) {
    *doc_id
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = value;
}

#[derive(Clone)]
struct SubagentScope {
    subagent_run_id: String,
    agent_name: String,
}

#[derive(Clone)]
struct StreamEventSender {
    sender: mpsc::Sender<RuntimeResult<ModelStreamEvent>>,
    subagent: Option<SubagentScope>,
}

impl StreamEventSender {
    fn top(sender: mpsc::Sender<RuntimeResult<ModelStreamEvent>>) -> Self {
        Self {
            sender,
            subagent: None,
        }
    }

    fn child(&self, subagent_run_id: String, agent_name: String) -> Self {
        Self {
            sender: self.sender.clone(),
            subagent: Some(SubagentScope {
                subagent_run_id,
                agent_name,
            }),
        }
    }

    async fn send(&self, event: ModelStreamEvent) {
        let event = match &self.subagent {
            Some(scope) => ModelStreamEvent::SubagentEvent {
                subagent_run_id: scope.subagent_run_id.clone(),
                agent_name: scope.agent_name.clone(),
                event: Box::new(event),
            },
            None => event,
        };
        let _ = self.sender.send(Ok(event)).await;
    }

    async fn send_raw(&self, event: ModelStreamEvent) {
        let _ = self.sender.send(Ok(event)).await;
    }
}

impl RigModelCore {
    pub(super) fn build_report_mcp(&self) -> quire_server::mcp::QuireMcpServer {
        let bind_addr: std::net::SocketAddr = self
            .config
            .bind_addr
            .parse()
            .unwrap_or_else(|_| std::net::SocketAddr::from(([127, 0, 0, 1], 8081)));
        let server_config = quire_server::Config {
            bind_addr,
            database_url: secrecy::SecretString::from(self.config.database_url.clone()),
            redis_url: secrecy::SecretString::from(self.config.redis_url.clone()),
            rw_token: None,
            ro_token: None,
            jwt_secret: Some(self.config.jwt_secret.clone()),
            allowed_hosts: self.config.mcp_allowed_hosts(),
            cors_allowed_origins: Vec::new(),
            sse_heartbeat: std::time::Duration::from_millis(self.config.sse_heartbeat_ms),
            auto_migrate: false,
            mcp_json_response: true,
        };
        let state =
            quire_server::AppState::new(self.kb.store().report_store().clone(), server_config);
        quire_server::mcp::QuireMcpServer::new(state)
    }

    async fn run_main_turn(
        self: Arc<Self>,
        request: ModelTurnRequest,
        sender: mpsc::Sender<RuntimeResult<ModelStreamEvent>>,
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
        let execution = AgentExecution {
            agent_id: request.agent_id,
            model_route,
            allowed_tools,
            system_prompt: request.system_prompt,
            kb_branch: branch,
            doc_id: new_shared_doc_id(request.doc_id),
            session_id: request.session_id,
            run_id: request.run_id,
            cancellation: request.cancellation.clone(),
        };
        let scope = StreamEventSender::top(sender);
        self.run_turn(
            execution,
            Message::user(request.user_message),
            request.kb_update_notice,
            scope,
            request.prompt_recorded,
            request.persist_history,
        )
        .await?;
        Ok(())
    }

    async fn run_turn(
        self: Arc<Self>,
        execution: AgentExecution,
        prompt: Message,
        kb_update_notice: Option<String>,
        scope: StreamEventSender,
        prompt_recorded: bool,
        persist_history: bool,
    ) -> RuntimeResult<(String, Vec<Message>)> {
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
        let max_tokens = resolved
            .max_output_tokens
            .and_then(|value| u64::try_from(value).ok())
            .or((resolved.protocol == crate::models::ProviderProtocol::Anthropic).then_some(8192));
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

        let result = async {
            let memory = self.kb.store().memory().await?;
            let memory_key = if execution.run_id.starts_with("run_sub_") {
                crate::memory::ConversationMemory::subagent_log(
                    &execution.session_id,
                    &execution.run_id,
                )
            } else {
                execution.session_id.clone()
            };
            let conversation = memory.load(&memory_key).await?;
            let recorded_history: Vec<Message> = conversation
                .messages()
                .iter()
                .map(|message| message.as_ref().clone())
                .collect();
            let initial_history = if prompt_recorded {
                if recorded_history.last() != Some(&prompt) {
                    return Err(RuntimeError::Internal(format!(
                        "canonical prompt for run `{}` is not the latest recorded message",
                        execution.run_id
                    )));
                }
                let mut history = recorded_history.clone();
                history.pop();
                history
            } else {
                recorded_history.clone()
            };

            let built_in = self.build_tools(ToolBuildContext {
                execution: execution.clone(),
                scope: scope.clone(),
                engine: Arc::clone(&self),
                sandbox: Arc::clone(&sandbox_lease),
            });
            let mut available_tools = built_in.names.clone();
            let mut remote_tools = Vec::new();
            let bindings = self.mcp.tools_for_agent(&execution.allowed_tools).await?;
            let clients = join_all(bindings.into_iter().map(|binding| async {
                let allowed: Vec<String> = binding
                    .tool_names
                    .iter()
                    .filter(|name| !BUILTIN_TOOLS.contains(&name.as_str()))
                    .cloned()
                    .collect();
                let client = if allowed.is_empty() {
                    None
                } else {
                    Some(self.mcp.shared_client(&binding).await)
                };
                (binding, allowed, client)
            }))
            .await;
            for (binding, allowed, client) in clients {
                let Some(client) = client else {
                    continue;
                };
                match client {
                    Ok(client) => match client.dynamic_tools(&allowed) {
                        Ok(remote) => {
                            for tool in remote {
                                available_tools.insert(tool.name().as_str().to_string());
                                remote_tools.push(tool);
                            }
                        }
                        Err(error) => warn!(%error, server_id = %binding.server_id, "skipping unavailable MCP tools"),
                    },
                    Err(error) => warn!(%error, server_id = %binding.server_id, "skipping unavailable MCP server"),
                }
            }
            warn_missing_allowed_tools(&execution.agent_id, &execution.allowed_tools, &available_tools);
            let tool_server = built_in.server.dynamic_tools(remote_tools).run();

            let agent = AgentBuilder::new(model)
                .name(execution.agent_id.clone())
                .preamble(static_preamble(&execution))
                .tool_server_handle(tool_server)
                .default_max_turns(usize::MAX)
                .build();

            let mut tool_context = ToolContext::new().with_scope(built_in.runtime.clone());
            let mut mcp_meta = RequestMetaObject::new();
            mcp_meta.insert(
                "quire".into(),
                serde_json::to_value(QuireMcpMeta {
                    session_id: &execution.session_id,
                    run_id: &execution.run_id,
                    agent_id: &execution.agent_id,
                })?,
            );
            tool_context
                .insert(McpMeta(mcp_meta))
                .map_err(|error| RuntimeError::Internal(error.to_string()))?;
            tool_context
                .insert(context::QuireTurnMeta {
                    session_id: execution.session_id.clone(),
                    run_id: execution.run_id.clone(),
                    agent_id: execution.agent_id.clone(),
                    kb_branch: execution.kb_branch.clone(),
                    supports_vision,
                })
                .map_err(|error| RuntimeError::Internal(error.to_string()))?;

            let previews = Arc::new(Mutex::new(PreviewState::new(
                &execution,
                &self.config,
                Some(self.kb.store().report_store().clone()),
            )));
            let turn_pricing = ModelTurnPricing {
                context_limit,
                cost_input: resolved.cost_input,
                cost_output: resolved.cost_output,
                cost_cache_read: resolved.cost_cache_read,
            };
            let state: SharedTurnState = Arc::new(Mutex::new(TurnRunState::default()));
            let mut runner = agent
                .prompt(prompt.clone())
                .history(initial_history.clone())
                .tool_context(tool_context)
                .tool_concurrency(DEFAULT_TOOL_CONCURRENCY)
                .max_turns(usize::MAX)
                .max_invalid_tool_call_retries(3)
                .max_consecutive_malformed_tool_calls(3)
                .add_hook(TurnControlHook::new(
                    execution.clone(),
                    scope.clone(),
                    self.queue.clone(),
                    kb_update_notice,
                    turn_pricing,
                    Arc::clone(&state),
                ))
                .add_hook(ToolLifecycleHook::new(
                    execution.clone(),
                    scope.clone(),
                    self.kb.clone(),
                    Arc::clone(&previews),
                    Arc::clone(&state),
                ));
            if let Some(max_tokens) = max_tokens {
                runner = runner.max_tokens(max_tokens);
            }

            let mut stream = runner.stream();
            let mut final_text = String::new();
            let mut cancellation_observed = false;
            let outcome = loop {
                tokio::select! {
                    _ = execution.cancellation.cancelled(), if !cancellation_observed => {
                        cancellation_observed = true;
                        state.lock().await.termination = RunTermination::Cancelled;
                    }
                    item = stream.next() => {
                        match item {
                            Some(Ok(MultiTurnStreamItem::FinalResponse(response))) => {
                                final_text = response.output();
                                let mut history = initial_history.clone();
                                history.extend(response.messages);
                                break Ok((final_text, history));
                            }
                            Some(Ok(_)) => {}
                            Some(Err(PromptError::Cancelled { chat_history, .. })) => {
                                break Ok((final_text, chat_history));
                            }
                            Some(Err(error)) => break Err(prompt_error(error)),
                            None => {
                                break Err(RuntimeError::Internal(
                                    "agent stream ended without a final response".into(),
                                ));
                            }
                        }
                    }
                }
            };
            // Guarantee that any in-flight or aborted preview cards are cleared on the
            // server whenever the run exits (cancelled, superseded, errored, or finished).
            previews.lock().await.abort_all().await;

            let (text, full_history) = outcome?;
            if persist_history {
                memory
                    .commit(
                        &memory_key,
                        &recorded_history,
                        full_history.clone(),
                        (!execution.run_id.starts_with("run_sub_")).then_some(execution.run_id.as_str()),
                    )
                    .await?;
            }

            let termination = state.lock().await.termination;
            if termination == RunTermination::Superseded {
                scope.send(ModelStreamEvent::Superseded).await;
            }
            Ok((text, full_history))
        }
        .await;

        if let Err(error) = sandbox_lease.close().await {
            warn!(%error, run_id = %execution.run_id, "sandbox cleanup failed; provider TTL remains the backstop");
        }
        result
    }
}

fn static_preamble(execution: &AgentExecution) -> String {
    let mut preamble = execution.system_prompt.clone();
    preamble.push_str("\n\n<runtime_rules>\n");
    if let Some(branch) = execution.kb_branch.as_deref() {
        preamble.push_str(&format!(
            "Knowledge-base branch for this session: `{branch}`. Agent and skill lookups use this branch.\n"
        ));
    }
    preamble.push_str("</runtime_rules>\n");
    preamble
}

pub(super) fn tool_output_value(output: &rig::tool::ToolOutput) -> Value {
    if let Some(value) = output.as_json() {
        return value.clone();
    }
    if let Some(text) = output.as_text() {
        return serde_json::to_value(TextToolOutput { text }).unwrap_or_else(|error| {
            warn!(%error, "failed to serialize text tool output");
            Value::Null
        });
    }
    serde_json::to_value(output).unwrap_or(Value::Null)
}

pub(super) fn context_usage(usage: &Usage, context_limit: u64) -> ContextUsage {
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
        cost_rmb: None,
    }
}

pub(super) fn preview_json(value: &Value) -> (String, bool) {
    let raw = serde_json::to_string(value).unwrap_or_default();
    (raw, false)
}

fn prompt_error(error: PromptError) -> RuntimeError {
    RuntimeError::Internal(format!("agent run failed: {error}"))
}

#[cfg(test)]
mod tests;
