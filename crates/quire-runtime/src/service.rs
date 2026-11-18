//! Session orchestration, run lifecycle, recovery and prompt-cache handling.

use std::sync::Arc;

use async_trait::async_trait;
use quire_session_protocol::{
    MessageRole, SendMessageRequest, SessionDetail, SessionEvent, SessionEventKind, SessionMessage,
    SessionStatus, SessionSummary, ToolCallDetail,
};
use secrecy::SecretString;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::{
    bus::InterruptCommand,
    config::Config,
    control::{RunRegistry, spawn_interrupt_listener},
    error::{RuntimeError, RuntimeResult},
    kb::{KbService, SeedReport},
    prompt::{ContextMessage, ResumeContext, build_resume_context},
    rig_agent::McpHttpClient,
    store::{RunRecord, RuntimeStore},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedDoc {
    pub doc_id: String,
    pub title: String,
}

#[async_trait]
pub trait QuireControl: Send + Sync {
    async fn create_report(&self, title: &str) -> RuntimeResult<LinkedDoc>;
}

pub struct HttpQuireControl {
    mcp: McpHttpClient,
}

impl HttpQuireControl {
    pub fn new(base_url: &str, token: SecretString) -> Self {
        Self {
            mcp: McpHttpClient::new(base_url, token, "runtime_orchestrator"),
        }
    }
}

#[async_trait]
impl QuireControl for HttpQuireControl {
    async fn create_report(&self, title: &str) -> RuntimeResult<LinkedDoc> {
        let value = self
            .mcp
            .call_tool(
                "create_report",
                serde_json::json!({
                    "title": title,
                    "enable_cover": true,
                    "enable_toc": true,
                    "idempotency_key": format!("runtime:{}", uuid::Uuid::new_v4().simple()),
                }),
            )
            .await?;
        let doc_id = value
            .get("doc_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                RuntimeError::Internal("create_report receipt did not contain doc_id".into())
            })?;
        Ok(LinkedDoc {
            doc_id: doc_id.to_string(),
            title: title.to_string(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub session_id: String,
    pub run_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
    pub output: serde_json::Value,
    pub status: String,
    pub duration_ms: u64,
}

#[derive(Clone)]
pub struct RuntimeService {
    store: RuntimeStore,
    kb: KbService,
    runs: RunRegistry,
    config: Arc<Config>,
    quire: Arc<dyn QuireControl>,
}

impl RuntimeService {
    pub fn new(
        store: RuntimeStore,
        kb: KbService,
        config: Arc<Config>,
        quire: Arc<dyn QuireControl>,
    ) -> Self {
        Self {
            store,
            kb,
            runs: RunRegistry::default(),
            config,
            quire,
        }
    }

    pub fn store(&self) -> &RuntimeStore {
        &self.store
    }

    pub fn kb(&self) -> &KbService {
        &self.kb
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn runs(&self) -> &RunRegistry {
        &self.runs
    }

    pub async fn boot(&self) -> RuntimeResult<usize> {
        let _handle = self.kb.spawn_invalidation_listener();
        let _interrupt = spawn_interrupt_listener(self.store.bus().clone(), self.runs.clone());
        let recovered = self.store.recover_running_runs().await?;
        Ok(recovered)
    }

    pub async fn seed(&self) -> RuntimeResult<SeedReport> {
        self.kb
            .seed(&self.config.workspace_dir, &self.config.default_model)
            .await
    }

    pub async fn create_session(
        &self,
        title: &str,
        doc_id: Option<&str>,
    ) -> RuntimeResult<SessionSummary> {
        self.store.create_session(title, doc_id).await
    }

    pub async fn list_sessions(
        &self,
        limit: i64,
        offset: i64,
    ) -> RuntimeResult<Vec<SessionSummary>> {
        self.store.list_sessions(limit, offset).await
    }

    pub async fn session_detail(&self, session_id: &str) -> RuntimeResult<SessionDetail> {
        self.store.get_session_detail(session_id).await
    }

    pub async fn update_session(
        &self,
        session_id: &str,
        title: Option<&str>,
        status: Option<SessionStatus>,
    ) -> RuntimeResult<SessionSummary> {
        self.store.update_session(session_id, title, status).await
    }

    pub async fn delete_session(&self, session_id: &str) -> RuntimeResult<()> {
        self.store.delete_session(session_id).await
    }

    pub async fn post_message(
        &self,
        session_id: &str,
        request: &SendMessageRequest,
    ) -> RuntimeResult<(SessionMessage, bool, Option<String>)> {
        let session = self.store.get_session_summary(session_id).await?;
        if session.status == SessionStatus::Archived {
            return Err(RuntimeError::InvalidRequest(
                "archived sessions cannot receive new messages".into(),
            ));
        }
        let (message, inserted) = self.store.insert_user_message(session_id, request).await?;
        if !inserted {
            return Ok((message, false, None));
        }
        let run_id = self.spawn_turn(session_id, request.content.clone()).await?;
        Ok((message, true, Some(run_id)))
    }

    pub async fn create_attachment(
        &self,
        session_id: &str,
        filename: &str,
        mime_type: &str,
        size_bytes: u64,
        content_base64: Option<&str>,
    ) -> RuntimeResult<quire_session_protocol::AttachmentMeta> {
        let storage_dir = self
            .config
            .workspace_dir
            .join(".quire-runtime")
            .join("attachments");
        tokio::fs::create_dir_all(&storage_dir).await?;
        let path = storage_dir.join(format!("{}-{filename}", uuid::Uuid::new_v4().simple()));
        let bytes = content_base64
            .map(|value| value.as_bytes().to_vec())
            .unwrap_or_default();
        tokio::fs::write(&path, &bytes).await?;
        self.store
            .insert_attachment(
                session_id,
                filename,
                mime_type,
                size_bytes,
                &path.to_string_lossy(),
            )
            .await
    }

    pub async fn interrupt(
        &self,
        session_id: &str,
        run_id: Option<&str>,
    ) -> RuntimeResult<SessionDetail> {
        let run = match run_id {
            Some(run_id) => self.store.get_run(run_id).await?,
            None => self
                .store
                .latest_running_run(session_id)
                .await?
                .ok_or_else(|| RuntimeError::InvalidRequest("session has no active run".into()))?,
        };
        if run.session_id != session_id {
            return Err(RuntimeError::InvalidRequest(
                "run does not belong to this session".into(),
            ));
        }
        self.store
            .bus()
            .publish_interrupt(&InterruptCommand {
                session_id: session_id.to_string(),
                run_id: run.run_id.clone(),
            })
            .await?;
        let _ = self.runs.cancel(&run.run_id).await;
        if let Some(run) = self
            .store
            .interrupt_run(&run.run_id, "interrupted by user")
            .await?
        {
            self.emit(
                session_id,
                &run.run_id,
                &run.agent_id,
                run.parent_run_id.as_deref(),
                SessionEventKind::TurnFailed {
                    error: "interrupted by user".into(),
                    recoverable: true,
                },
            )
            .await?;
            self.runs.remove(&run.run_id).await;
        }
        self.store.get_session_detail(session_id).await
    }

    pub async fn retry(&self, session_id: &str) -> RuntimeResult<SessionDetail> {
        let messages = self.store.list_messages(session_id).await?;
        let Some(last_user) = messages
            .iter()
            .rev()
            .find(|message| message.role == MessageRole::User)
        else {
            return Err(RuntimeError::InvalidRequest(
                "session has no user message to retry".into(),
            ));
        };
        let _ = self
            .spawn_turn(session_id, last_user.content.clone())
            .await?;
        self.store.get_session_detail(session_id).await
    }

    pub async fn compact(&self, session_id: &str) -> RuntimeResult<SessionDetail> {
        let prompt = self.kb.system_prompt().await?;
        let version = self.kb.current_version().await?;
        self.store
            .set_pinned_prompt(session_id, prompt.as_str(), version)
            .await?;
        self.store.get_session_detail(session_id).await
    }

    pub async fn get_tool_call(
        &self,
        session_id: &str,
        tool_call_id: &str,
    ) -> RuntimeResult<ToolCallDetail> {
        self.store.get_tool_call(session_id, tool_call_id).await
    }

    /// Persist the full tool input/output and emit live start/finish summaries.
    pub async fn record_tool_call(&self, record: ToolCallRecord) -> RuntimeResult<ToolCallDetail> {
        let tool_call_id = format!("tc_{}", uuid::Uuid::new_v4().simple());
        let (input_preview, input_truncated) = preview_json(&record.input);
        let (output_preview, output_truncated) = preview_json(&record.output);
        self.emit(
            &record.session_id,
            &record.run_id,
            "main",
            None,
            SessionEventKind::ToolCallStarted {
                tool_call_id: tool_call_id.clone(),
                tool_name: record.tool_name.clone(),
                input_preview,
                truncated: input_truncated,
            },
        )
        .await?;
        let detail = ToolCallDetail {
            tool_call_id: tool_call_id.clone(),
            session_id: record.session_id.clone(),
            run_id: record.run_id.clone(),
            tool_name: record.tool_name.clone(),
            status: record.status.clone(),
            input: record.input,
            output: record.output,
            error: None,
            duration_ms: record.duration_ms,
            created_at: chrono::Utc::now(),
        };
        self.store.save_tool_call(&detail).await?;
        self.emit(
            &record.session_id,
            &record.run_id,
            "main",
            None,
            SessionEventKind::ToolCallFinished {
                tool_call_id,
                status: record.status,
                output_preview,
                truncated: output_truncated,
                duration_ms: record.duration_ms,
            },
        )
        .await?;
        Ok(detail)
    }

    pub async fn record_task_focus(
        &self,
        session_id: &str,
        run_id: &str,
        title: &str,
        detail: Option<&str>,
        section_id: Option<&str>,
    ) -> RuntimeResult<SessionEvent> {
        self.emit(
            session_id,
            run_id,
            "main",
            None,
            SessionEventKind::TaskFocus {
                title: title.to_string(),
                detail: detail.map(str::to_string),
                section_id: section_id.map(str::to_string),
            },
        )
        .await
    }

    pub async fn record_subagent_started(
        &self,
        session_id: &str,
        parent_run_id: &str,
        subagent_run_id: &str,
        agent_name: &str,
        task: &str,
        section_id: Option<&str>,
    ) -> RuntimeResult<SessionEvent> {
        self.emit(
            session_id,
            parent_run_id,
            agent_name,
            Some(parent_run_id),
            SessionEventKind::SubagentStarted {
                subagent_run_id: subagent_run_id.to_string(),
                agent_name: agent_name.to_string(),
                task: task.to_string(),
                section_id: section_id.map(str::to_string),
            },
        )
        .await
    }

    pub async fn record_subagent_finished(
        &self,
        session_id: &str,
        parent_run_id: &str,
        subagent_run_id: &str,
        status: &str,
        summary: &str,
    ) -> RuntimeResult<SessionEvent> {
        self.emit(
            session_id,
            parent_run_id,
            "main",
            Some(parent_run_id),
            SessionEventKind::SubagentFinished {
                subagent_run_id: subagent_run_id.to_string(),
                status: status.to_string(),
                summary: summary.to_string(),
            },
        )
        .await
    }

    pub async fn resume_context(
        &self,
        session_id: &str,
        user_message: &str,
    ) -> RuntimeResult<ResumeContext> {
        let pinned = self.ensure_pinned_prompt(session_id).await?;
        let current = self.kb.current_version().await?;
        let messages = self.store.list_messages(session_id).await?;
        let history: Vec<ContextMessage> = messages
            .iter()
            .map(|message| ContextMessage {
                role: message.role,
                content: message.content.clone(),
            })
            .collect();
        Ok(build_resume_context(
            &pinned.0,
            &history,
            user_message,
            current,
            pinned.1,
        ))
    }

    async fn ensure_pinned_prompt(&self, session_id: &str) -> RuntimeResult<(String, u64)> {
        let (pinned, synced) = self.store.pinned_prompt(session_id).await?;
        if let Some(pinned) = pinned {
            return Ok((pinned, synced));
        }
        let prompt = self.kb.system_prompt().await?;
        let version = self.kb.current_version().await?;
        self.store
            .set_pinned_prompt(session_id, prompt.as_str(), version)
            .await?;
        Ok((prompt.as_str().to_string(), version))
    }

    async fn spawn_turn(&self, session_id: &str, content: String) -> RuntimeResult<String> {
        self.store.get_session_summary(session_id).await?;
        let run = self
            .store
            .create_run(session_id, None, "main", &self.config.default_model)
            .await?;
        let token = self.runs.register(&run.run_id).await;
        let service = self.clone();
        let run_id = run.run_id.clone();
        let message_content = content;
        let session_id = session_id.to_string();
        tokio::spawn(async move {
            if let Err(error) = service
                .run_turn(&session_id, &run_id, &message_content, token)
                .await
            {
                warn!(%error, %session_id, run_id, "agent turn failed");
            }
        });
        Ok(run.run_id)
    }

    async fn run_turn(
        &self,
        session_id: &str,
        run_id: &str,
        content: &str,
        token: CancellationToken,
    ) -> RuntimeResult<()> {
        let result = self.execute_turn(session_id, run_id, content, &token).await;
        self.runs.remove(run_id).await;
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                if !token.is_cancelled() {
                    let _ = self
                        .store
                        .finish_run(run_id, "failed", Some(&error.to_string()))
                        .await;
                    let _ = self
                        .emit(
                            session_id,
                            run_id,
                            "main",
                            None,
                            SessionEventKind::TurnFailed {
                                error: error.to_string(),
                                recoverable: true,
                            },
                        )
                        .await;
                }
                Err(error)
            }
        }
    }

    async fn execute_turn(
        &self,
        session_id: &str,
        run_id: &str,
        content: &str,
        token: &CancellationToken,
    ) -> RuntimeResult<()> {
        let mut session = self.store.get_session_summary(session_id).await?;
        if session.doc_id.is_none() {
            let linked = self.quire.create_report(&session.title).await?;
            self.store.bind_doc(session_id, &linked.doc_id).await?;
            self.emit(
                session_id,
                run_id,
                "main",
                None,
                SessionEventKind::DocLinked {
                    doc_id: linked.doc_id.clone(),
                    title: linked.title,
                },
            )
            .await?;
            session = self.store.get_session_summary(session_id).await?;
        }
        let (pinned, synced_version) = self.ensure_pinned_prompt(session_id).await?;
        let kb_version = self.kb.current_version().await?;
        let messages = self.store.list_messages(session_id).await?;
        let mut history: Vec<ContextMessage> = messages
            .iter()
            .filter(|message| message.role != MessageRole::System)
            .map(|message| ContextMessage {
                role: message.role,
                content: message.content.clone(),
            })
            .collect();
        if history
            .last()
            .is_some_and(|message| message.role == MessageRole::User && message.content == content)
        {
            history.pop();
        }
        let context = build_resume_context(&pinned, &history, content, kb_version, synced_version);
        if token.is_cancelled() {
            return Ok(());
        }
        self.emit(
            session_id,
            run_id,
            "main",
            None,
            SessionEventKind::TurnStarted,
        )
        .await?;
        if token.is_cancelled() {
            return Ok(());
        }
        self.emit(
            session_id,
            run_id,
            "main",
            None,
            SessionEventKind::ThinkingDelta {
                delta: "Preparing the next report action.".into(),
            },
        )
        .await?;
        let knowledge_reminder = context
            .messages
            .iter()
            .find(|message| message.content.starts_with("<knowledge_update>"))
            .map(|message| format!("{}\n", message.content))
            .unwrap_or_default();
        let response = format!(
            "{knowledge_reminder}Runtime accepted the request for `{}`. Connect a rig model provider to stream the full answer.",
            session.title
        );
        self.emit(
            session_id,
            run_id,
            "main",
            None,
            SessionEventKind::TextDelta {
                delta: response.clone(),
            },
        )
        .await?;
        if token.is_cancelled() {
            return Ok(());
        }
        self.store
            .insert_assistant_message(session_id, &response, run_id)
            .await?;
        let usage = quire_session_protocol::ContextUsage {
            input_tokens: u64::try_from(content.chars().count() / 4).unwrap_or(0),
            output_tokens: u64::try_from(response.chars().count() / 4).unwrap_or(0),
            cached_tokens: u64::try_from(context.pinned_system_prompt.chars().count() / 4)
                .unwrap_or(0),
            context_used: u64::try_from(
                (context.pinned_system_prompt.chars().count()
                    + content.chars().count()
                    + response.chars().count())
                    / 4,
            )
            .unwrap_or(0),
            context_limit: 200_000,
        };
        self.emit(
            session_id,
            run_id,
            "main",
            None,
            SessionEventKind::ContextUsage {
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                cached_tokens: usage.cached_tokens,
                context_used: usage.context_used,
                context_limit: usage.context_limit,
            },
        )
        .await?;
        self.store.update_context_usage(session_id, &usage).await?;
        if token.is_cancelled() {
            return Ok(());
        }
        self.emit(
            session_id,
            run_id,
            "main",
            None,
            SessionEventKind::TurnFinished,
        )
        .await?;
        self.store.finish_run(run_id, "completed", None).await?;
        Ok(())
    }

    async fn emit(
        &self,
        session_id: &str,
        run_id: &str,
        agent_id: &str,
        parent_run_id: Option<&str>,
        kind: SessionEventKind,
    ) -> RuntimeResult<SessionEvent> {
        self.store
            .append_event(session_id, run_id, agent_id, parent_run_id, kind)
            .await
    }
}

/// Test/embedding-friendly Quire control that never creates a remote report.
pub struct StaticQuireControl;

#[async_trait]
impl QuireControl for StaticQuireControl {
    async fn create_report(&self, _title: &str) -> RuntimeResult<LinkedDoc> {
        Err(RuntimeError::InvalidRequest(
            "doc linking requires a configured Quire server".into(),
        ))
    }
}

pub async fn running_record(store: &RuntimeStore, session_id: &str) -> Option<RunRecord> {
    store.latest_running_run(session_id).await.ok().flatten()
}

fn preview_json(value: &serde_json::Value) -> (String, bool) {
    let raw = serde_json::to_string(value).unwrap_or_default();
    let mut chars = raw.chars();
    let mut preview: String = chars.by_ref().take(240).collect();
    let truncated = chars.next().is_some();
    if truncated {
        preview.push('…');
    }
    (preview, truncated)
}
