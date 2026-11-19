//! Session orchestration, run lifecycle, recovery and prompt-cache handling.

use std::{
    collections::HashMap,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use base64::Engine as _;
use futures::StreamExt;
use quire_session_protocol::{
    ContextUsage, MessageRole, PendingQuestion, QuestionAnswer, QuestionItem, RunTraceSummary,
    SendMessageRequest, SessionDetail, SessionEvent, SessionEventKind, SessionMessage,
    SessionStatus, SessionSummary, ToolCallDetail,
};
use secrecy::SecretString;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::{
    bus::InterruptCommand,
    config::Config,
    control::{RunRegistry, spawn_interrupt_listener},
    delta::{CoalescedDelta, DeltaCoalescer, DeltaKind},
    error::{RuntimeError, RuntimeResult},
    kb::{KbService, SeedReport},
    mcp_client::RmcpClient,
    mcp_registry::McpRegistry,
    model_stream::{ModelStreamEvent, ModelStreamProvider, ModelTurnRequest, StubStreamProvider},
    models::ModelRegistry,
    prompt::{ContextMessage, ResumeContext, build_resume_context},
    store::{RunRecord, RuntimeStore},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedDoc {
    pub doc_id: String,
    pub title: String,
}

#[async_trait]
pub trait QuireControl: Send + Sync {
    async fn create_report(
        &self,
        session_id: &str,
        agent_id: &str,
        title: &str,
    ) -> RuntimeResult<LinkedDoc>;
}

pub struct HttpQuireControl {
    base_url: String,
    token: SecretString,
}

impl HttpQuireControl {
    pub fn new(base_url: &str, token: SecretString) -> Self {
        Self {
            base_url: base_url.to_string(),
            token,
        }
    }
}

#[async_trait]
impl QuireControl for HttpQuireControl {
    async fn create_report(
        &self,
        session_id: &str,
        agent_id: &str,
        title: &str,
    ) -> RuntimeResult<LinkedDoc> {
        let client = RmcpClient::connect(
            &self.base_url,
            Some(self.token.clone()),
            session_id,
            agent_id,
        )
        .await?;
        let value = client
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

/// Admin-facing session observation: full detail plus the latest run regardless of outcome.
#[derive(Debug, Clone)]
pub struct SessionInspection {
    pub detail: SessionDetail,
    pub latest_run: Option<RunRecord>,
}

#[derive(Clone)]
pub struct RuntimeService {
    store: RuntimeStore,
    kb: KbService,
    models: Arc<ModelRegistry>,
    mcp: Arc<McpRegistry>,
    runs: RunRegistry,
    config: Arc<Config>,
    model_stream: Arc<dyn ModelStreamProvider>,
}

impl RuntimeService {
    pub fn new(
        store: RuntimeStore,
        kb: KbService,
        models: Arc<ModelRegistry>,
        config: Arc<Config>,
    ) -> Self {
        let mcp = Arc::new(McpRegistry::new(store.clone(), config.master_key.clone()));
        Self {
            store,
            kb,
            models,
            mcp,
            runs: RunRegistry::default(),
            config,
            model_stream: Arc::new(StubStreamProvider),
        }
    }

    pub fn store(&self) -> &RuntimeStore {
        &self.store
    }

    pub fn kb(&self) -> &KbService {
        &self.kb
    }

    pub fn models(&self) -> &ModelRegistry {
        &self.models
    }

    pub fn mcp(&self) -> &McpRegistry {
        &self.mcp
    }

    pub fn mcp_registry(&self) -> Arc<McpRegistry> {
        Arc::clone(&self.mcp)
    }

    pub fn with_mcp_registry(mut self, mcp: Arc<McpRegistry>) -> Self {
        self.mcp = mcp;
        self
    }

    pub fn with_model_stream_provider(mut self, provider: Arc<dyn ModelStreamProvider>) -> Self {
        self.model_stream = provider;
        self
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn runs(&self) -> &RunRegistry {
        &self.runs
    }

    pub async fn boot(&self) -> RuntimeResult<usize> {
        let _handle = self.kb.spawn_invalidation_listener();
        let _models = self.models.spawn_invalidation_listener();
        let _mcp = self.mcp.spawn_invalidation_listener();
        let _interrupt = spawn_interrupt_listener(self.store.bus().clone(), self.runs.clone());
        let recovered = self.store.recover_running_runs().await?;
        Ok(recovered)
    }

    pub async fn seed(&self) -> RuntimeResult<SeedReport> {
        let report = self
            .kb
            .seed(&self.config.workspace_dir, &self.config.default_model)
            .await?;
        self.mcp
            .seed_default_quire_report(&self.config.quire_base_url)
            .await?;
        Ok(report)
    }

    pub async fn create_session(
        &self,
        title: &str,
        doc_id: Option<&str>,
    ) -> RuntimeResult<SessionSummary> {
        self.create_session_on_branch(title, doc_id, None).await
    }

    /// Create a session against an explicit KB branch. `None` resolves the current active branch.
    pub async fn create_session_on_branch(
        &self,
        title: &str,
        doc_id: Option<&str>,
        kb_branch: Option<&str>,
    ) -> RuntimeResult<SessionSummary> {
        let branch = self.kb.session_branch(kb_branch).await?;
        self.store
            .create_session_on_branch(title, doc_id, &branch)
            .await
    }

    pub async fn inspect_session(&self, session_id: &str) -> RuntimeResult<SessionInspection> {
        let detail = self.store.get_session_detail(session_id).await?;
        let latest_run = self.store.latest_run(session_id).await?;
        Ok(SessionInspection { detail, latest_run })
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

    pub async fn list_run_traces(&self, session_id: &str) -> RuntimeResult<Vec<RunTraceSummary>> {
        self.store.list_run_traces(session_id).await
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
        let dismissed = self.store.dismiss_pending_questions(session_id).await?;
        for question in dismissed {
            let _ = self.store.resolve_suspended_run(&question.run_id).await?;
            self.emit(
                session_id,
                &question.run_id,
                "main",
                None,
                SessionEventKind::QuestionnaireDismissed {
                    question_id: question.question_id,
                    reason: "superseded_by_user_message".into(),
                },
            )
            .await?;
        }
        let run_id = self.spawn_turn(session_id, request.content.clone()).await?;
        Ok((message, true, Some(run_id)))
    }

    /// Resolve a pending ask_user card and continue the interrupted reasoning immediately.
    pub async fn answer_question(
        &self,
        session_id: &str,
        question_id: &str,
        answers: &[QuestionAnswer],
        skipped: bool,
    ) -> RuntimeResult<SessionDetail> {
        let question = self
            .store
            .pending_question_by_id(session_id, question_id)
            .await?
            .ok_or_else(|| {
                RuntimeError::InvalidRequest(format!(
                    "question `{question_id}` is not pending for session `{session_id}`"
                ))
            })?;
        let answers = if skipped {
            Vec::new()
        } else {
            validate_question_answers(&question.questions, answers)?;
            answers.to_vec()
        };
        self.store
            .resolve_question(session_id, question_id, &answers, skipped)
            .await?;
        let _ = self.store.resolve_suspended_run(&question.run_id).await?;
        self.emit(
            session_id,
            &question.run_id,
            "main",
            None,
            SessionEventKind::QuestionnaireAnswered {
                question_id: question.question_id.clone(),
                answers: answers.clone(),
                skipped_all: skipped,
            },
        )
        .await?;
        let continuation = if skipped {
            format!(
                "The user skipped questionnaire `{}`. Continue the task without those details.",
                question.question_id
            )
        } else {
            format!(
                "The user answered questionnaire `{}`:\n{}",
                question.question_id,
                serde_json::to_string_pretty(&serde_json::json!({
                    "questions": question.questions,
                    "answers": answers,
                }))?
            )
        };
        self.store
            .insert_context_message(
                session_id,
                MessageRole::Tool,
                &continuation,
                &format!("question-context:{}", question.question_id),
                Some(&question.run_id),
            )
            .await?;
        self.spawn_turn(
            session_id,
            "Continue the task using the questionnaire response.".to_string(),
        )
        .await?;
        self.store.get_session_detail(session_id).await
    }

    pub async fn create_attachment(
        &self,
        session_id: &str,
        filename: &str,
        mime_type: &str,
        size_bytes: u64,
        content_base64: Option<&str>,
    ) -> RuntimeResult<quire_session_protocol::AttachmentMeta> {
        let filename = safe_attachment_filename(filename)?;
        let bytes = match content_base64 {
            Some(encoded) => base64::engine::general_purpose::STANDARD
                .decode(encoded.trim())
                .map_err(|error| {
                    RuntimeError::InvalidRequest(format!("invalid base64 attachment: {error}"))
                })?,
            None => Vec::new(),
        };
        let storage_dir = self
            .config
            .workspace_dir
            .join(".quire-runtime")
            .join("attachments");
        tokio::fs::create_dir_all(&storage_dir).await?;
        let path = storage_dir.join(format!("{}-{filename}", uuid::Uuid::new_v4().simple()));
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
        let session = self.store.get_session_summary(session_id).await?;
        let prompt = self
            .kb
            .system_prompt_for(session.kb_branch.as_deref())
            .await?;
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
                display_name: None,
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
        let session = self.store.get_session_summary(session_id).await?;
        let prompt = self
            .kb
            .system_prompt_for(session.kb_branch.as_deref())
            .await?;
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
        let session = self.store.get_session_summary(session_id).await?;
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
        let branch = session.kb_branch.clone();
        let agent = self.kb.get_default_agent_on(branch.as_deref()).await?;
        let model_route = agent
            .as_ref()
            .map(|agent| agent.model.clone())
            .unwrap_or_else(|| self.config.default_model.clone());
        let allowed_tools = agent.map(|agent| agent.allowed_tools).unwrap_or_default();
        let mut coalescer = DeltaCoalescer::default();
        let request = ModelTurnRequest {
            session_id: session_id.to_string(),
            run_id: run_id.to_string(),
            agent_id: "main".into(),
            system_prompt: context.pinned_system_prompt.clone(),
            user_message: content.to_string(),
            history: context.messages.clone(),
            doc_id: session.doc_id.clone(),
            kb_branch: branch.clone(),
            model_route: model_route.clone(),
            allowed_tools,
            cancellation: token.clone(),
        };
        let mut stream = self.model_stream.stream_turn(request).await?;
        let mut tick = tokio::time::interval(Duration::from_millis(50));
        let mut response = String::new();
        let mut reported_usage = None;
        let mut suspended_question: Option<PendingQuestion> = None;
        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    let deltas = coalescer.flush_all();
                    self.emit_coalesced(session_id, run_id, "main", deltas).await?;
                    return Ok(());
                }
                _ = tick.tick() => {
                    let deltas = coalescer.flush_due(Instant::now());
                    self.emit_coalesced(session_id, run_id, "main", deltas).await?;
                }
                item = stream.next() => {
                    match item {
                        Some(Ok(ModelStreamEvent::ThinkingDelta(delta))) => {
                            let deltas = coalescer.push(
                                Instant::now(),
                                run_id,
                                "main",
                                DeltaKind::Thinking,
                                &delta,
                            );
                            self.emit_coalesced(session_id, run_id, "main", deltas).await?;
                        }
                        Some(Ok(ModelStreamEvent::TextDelta(delta))) => {
                            response.push_str(&delta);
                            let deltas = coalescer.push(
                                Instant::now(),
                                run_id,
                                "main",
                                DeltaKind::Text,
                                &delta,
                            );
                            self.emit_coalesced(session_id, run_id, "main", deltas).await?;
                        }
                        Some(Ok(ModelStreamEvent::TaskFocus {
                            title,
                            detail,
                            section_id,
                        })) => {
                            let deltas = coalescer.flush_all();
                            self.emit_coalesced(session_id, run_id, "main", deltas).await?;
                            self.emit(
                                session_id,
                                run_id,
                                "main",
                                None,
                                SessionEventKind::TaskFocus {
                                    title,
                                    detail,
                                    section_id,
                                },
                            )
                            .await?;
                        }
                        Some(Ok(ModelStreamEvent::DocLinked { doc_id, title })) => {
                            let deltas = coalescer.flush_all();
                            self.emit_coalesced(session_id, run_id, "main", deltas).await?;
                            self.emit(
                                session_id,
                                run_id,
                                "main",
                                None,
                                SessionEventKind::DocLinked { doc_id, title },
                            )
                            .await?;
                        }
                        Some(Ok(ModelStreamEvent::SessionTitleUpdated { title })) => {
                            let deltas = coalescer.flush_all();
                            self.emit_coalesced(session_id, run_id, "main", deltas).await?;
                            self.emit(
                                session_id,
                                run_id,
                                "main",
                                None,
                                SessionEventKind::SessionTitleUpdated { title },
                            )
                            .await?;
                        }
                        Some(Ok(ModelStreamEvent::SubagentStarted {
                            subagent_run_id,
                            agent_name,
                            display_name,
                            task,
                            section_id,
                        })) => {
                            let deltas = coalescer.flush_all();
                            self.emit_coalesced(session_id, run_id, "main", deltas).await?;
                            self.emit(
                                session_id,
                                run_id,
                                &agent_name,
                                Some(run_id),
                                SessionEventKind::SubagentStarted {
                                    subagent_run_id,
                                    agent_name: agent_name.clone(),
                                    display_name,
                                    task,
                                    section_id,
                                },
                            )
                            .await?;
                        }
                        Some(Ok(ModelStreamEvent::SubagentFinished {
                            subagent_run_id,
                            status,
                            summary,
                        })) => {
                            let deltas = coalescer.flush_all();
                            self.emit_coalesced(session_id, run_id, "main", deltas).await?;
                            self.emit(
                                session_id,
                                run_id,
                                "main",
                                Some(run_id),
                                SessionEventKind::SubagentFinished {
                                    subagent_run_id,
                                    status,
                                    summary,
                                },
                            )
                            .await?;
                        }
                        Some(Ok(ModelStreamEvent::SubagentEvent {
                            subagent_run_id,
                            agent_name,
                            event,
                        })) => {
                            let deltas = coalescer.flush_all();
                            self.emit_coalesced(session_id, run_id, "main", deltas).await?;
                            self.handle_subagent_event(
                                session_id,
                                run_id,
                                &subagent_run_id,
                                &agent_name,
                                *event,
                            )
                            .await?;
                        }
                        Some(Ok(ModelStreamEvent::ToolCallStarted {
                            tool_call_id,
                            tool_name,
                            input_preview,
                            truncated,
                            input,
                        })) => {
                            let deltas = coalescer.flush_all();
                            self.emit_coalesced(session_id, run_id, "main", deltas).await?;
                            self.record_stream_tool_started(
                                session_id,
                                run_id,
                                "main",
                                None,
                                &tool_call_id,
                                &tool_name,
                                &input_preview,
                                truncated,
                                input,
                            )
                            .await?;
                        }
                        Some(Ok(ModelStreamEvent::ToolCallFinished {
                            tool_call_id,
                            tool_name,
                            status,
                            output_preview,
                            truncated,
                            duration_ms,
                            input,
                            output,
                            error,
                        })) => {
                            let deltas = coalescer.flush_all();
                            self.emit_coalesced(session_id, run_id, "main", deltas).await?;
                            self.record_stream_tool_finished(
                                session_id,
                                run_id,
                                "main",
                                None,
                                &tool_call_id,
                                &tool_name,
                                &status,
                                &output_preview,
                                truncated,
                                duration_ms,
                                input,
                                output,
                                error,
                            )
                            .await?;
                        }
                        Some(Ok(ModelStreamEvent::QuestionnaireRequested {
                            tool_call_id,
                            questions,
                        })) => {
                            let deltas = coalescer.flush_all();
                            self.emit_coalesced(session_id, run_id, "main", deltas).await?;
                            let question = self
                                .store
                                .create_question(
                                    session_id,
                                    run_id,
                                    tool_call_id.as_deref(),
                                    &questions,
                                )
                                .await?;
                            self.emit(
                                session_id,
                                run_id,
                                "main",
                                None,
                                SessionEventKind::QuestionnaireRequested {
                                    question_id: question.question_id.clone(),
                                    tool_call_id: question.tool_call_id.clone(),
                                    questions: question.questions.clone(),
                                },
                            )
                            .await?;
                            suspended_question = Some(question);
                        }
                        Some(Ok(ModelStreamEvent::Usage(usage))) => {
                            let deltas = coalescer.flush_all();
                            self.emit_coalesced(session_id, run_id, "main", deltas).await?;
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
                            reported_usage = Some(usage);
                        }
                        Some(Err(error)) => {
                            let deltas = coalescer.flush_all();
                            self.emit_coalesced(session_id, run_id, "main", deltas).await?;
                            return Err(error);
                        }
                        None => break,
                    }
                }
            }
        }
        let deltas = coalescer.flush_all();
        self.emit_coalesced(session_id, run_id, "main", deltas)
            .await?;
        if token.is_cancelled() {
            return Ok(());
        }
        if let Some(question) = suspended_question {
            if !response.is_empty() {
                self.store
                    .insert_assistant_message(session_id, &response, run_id)
                    .await?;
            }
            if let Some(usage) = reported_usage {
                self.store.update_context_usage(session_id, &usage).await?;
            }
            self.store.mark_run_awaiting_input(run_id).await?;
            let _ = question;
            return Ok(());
        }
        self.store
            .insert_assistant_message(session_id, &response, run_id)
            .await?;
        let usage = match reported_usage {
            Some(usage) => usage,
            None => {
                let usage = self
                    .fallback_usage(
                        &model_route,
                        &context.pinned_system_prompt,
                        content,
                        &response,
                    )
                    .await;
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
                usage
            }
        };
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
        if kb_version > synced_version {
            self.store
                .advance_synced_kb_version(session_id, kb_version)
                .await?;
        }
        self.store.finish_run(run_id, "completed", None).await?;
        Ok(())
    }

    async fn handle_subagent_event(
        &self,
        session_id: &str,
        parent_run_id: &str,
        subagent_run_id: &str,
        agent_name: &str,
        event: ModelStreamEvent,
    ) -> RuntimeResult<()> {
        let parent = Some(parent_run_id);
        match event {
            ModelStreamEvent::ThinkingDelta(delta) => {
                self.emit(
                    session_id,
                    subagent_run_id,
                    agent_name,
                    parent,
                    SessionEventKind::ThinkingDelta { delta },
                )
                .await?;
            }
            ModelStreamEvent::TextDelta(delta) => {
                self.emit(
                    session_id,
                    subagent_run_id,
                    agent_name,
                    parent,
                    SessionEventKind::TextDelta { delta },
                )
                .await?;
            }
            ModelStreamEvent::TaskFocus {
                title,
                detail,
                section_id,
            } => {
                self.emit(
                    session_id,
                    subagent_run_id,
                    agent_name,
                    parent,
                    SessionEventKind::TaskFocus {
                        title,
                        detail,
                        section_id,
                    },
                )
                .await?;
            }
            ModelStreamEvent::DocLinked { .. } | ModelStreamEvent::SessionTitleUpdated { .. } => {}
            ModelStreamEvent::SubagentStarted {
                subagent_run_id: nested_id,
                agent_name: nested_name,
                display_name,
                task,
                section_id,
            } => {
                self.emit(
                    session_id,
                    subagent_run_id,
                    agent_name,
                    parent,
                    SessionEventKind::SubagentStarted {
                        subagent_run_id: nested_id,
                        agent_name: nested_name,
                        display_name,
                        task,
                        section_id,
                    },
                )
                .await?;
            }
            ModelStreamEvent::SubagentFinished {
                subagent_run_id: nested_id,
                status,
                summary,
            } => {
                self.emit(
                    session_id,
                    subagent_run_id,
                    agent_name,
                    parent,
                    SessionEventKind::SubagentFinished {
                        subagent_run_id: nested_id,
                        status,
                        summary,
                    },
                )
                .await?;
            }
            ModelStreamEvent::SubagentEvent {
                subagent_run_id: nested_id,
                agent_name: nested_name,
                event,
            } => {
                Box::pin(self.handle_subagent_event(
                    session_id,
                    parent_run_id,
                    &nested_id,
                    &nested_name,
                    *event,
                ))
                .await?;
            }
            ModelStreamEvent::ToolCallStarted {
                tool_call_id,
                tool_name,
                input_preview,
                truncated,
                input,
            } => {
                self.record_stream_tool_started(
                    session_id,
                    subagent_run_id,
                    agent_name,
                    parent,
                    &tool_call_id,
                    &tool_name,
                    &input_preview,
                    truncated,
                    input,
                )
                .await?;
            }
            ModelStreamEvent::ToolCallFinished {
                tool_call_id,
                tool_name,
                status,
                output_preview,
                truncated,
                duration_ms,
                input,
                output,
                error,
            } => {
                self.record_stream_tool_finished(
                    session_id,
                    subagent_run_id,
                    agent_name,
                    parent,
                    &tool_call_id,
                    &tool_name,
                    &status,
                    &output_preview,
                    truncated,
                    duration_ms,
                    input,
                    output,
                    error,
                )
                .await?;
            }
            ModelStreamEvent::Usage(usage) => {
                self.emit(
                    session_id,
                    subagent_run_id,
                    agent_name,
                    parent,
                    SessionEventKind::ContextUsage {
                        input_tokens: usage.input_tokens,
                        output_tokens: usage.output_tokens,
                        cached_tokens: usage.cached_tokens,
                        context_used: usage.context_used,
                        context_limit: usage.context_limit,
                    },
                )
                .await?;
            }
            ModelStreamEvent::QuestionnaireRequested { .. } => {
                warn!(
                    session = %session_id,
                    subagent = %subagent_run_id,
                    "ask_user is only supported by the main agent"
                );
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn record_stream_tool_started(
        &self,
        session_id: &str,
        run_id: &str,
        agent_id: &str,
        parent_run_id: Option<&str>,
        tool_call_id: &str,
        tool_name: &str,
        input_preview: &str,
        truncated: bool,
        input: Value,
    ) -> RuntimeResult<()> {
        let detail = ToolCallDetail {
            tool_call_id: tool_call_id.to_string(),
            session_id: session_id.to_string(),
            run_id: run_id.to_string(),
            tool_name: tool_name.to_string(),
            status: "running".into(),
            input,
            output: Value::Null,
            error: None,
            duration_ms: 0,
            created_at: chrono::Utc::now(),
        };
        self.store.save_tool_call(&detail).await?;
        self.emit(
            session_id,
            run_id,
            agent_id,
            parent_run_id,
            SessionEventKind::ToolCallStarted {
                tool_call_id: tool_call_id.to_string(),
                tool_name: tool_name.to_string(),
                input_preview: input_preview.to_string(),
                truncated,
            },
        )
        .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn record_stream_tool_finished(
        &self,
        session_id: &str,
        run_id: &str,
        agent_id: &str,
        parent_run_id: Option<&str>,
        tool_call_id: &str,
        tool_name: &str,
        status: &str,
        output_preview: &str,
        truncated: bool,
        duration_ms: u64,
        input: Value,
        output: Value,
        error: Option<String>,
    ) -> RuntimeResult<()> {
        let detail = ToolCallDetail {
            tool_call_id: tool_call_id.to_string(),
            session_id: session_id.to_string(),
            run_id: run_id.to_string(),
            tool_name: tool_name.to_string(),
            status: status.to_string(),
            input,
            output,
            error: error.clone(),
            duration_ms,
            created_at: chrono::Utc::now(),
        };
        self.store.save_tool_call(&detail).await?;
        self.emit(
            session_id,
            run_id,
            agent_id,
            parent_run_id,
            SessionEventKind::ToolCallFinished {
                tool_call_id: tool_call_id.to_string(),
                status: status.to_string(),
                output_preview: output_preview.to_string(),
                truncated,
                duration_ms,
            },
        )
        .await?;
        Ok(())
    }

    async fn fallback_usage(
        &self,
        model_route: &str,
        system_prompt: &str,
        content: &str,
        response: &str,
    ) -> ContextUsage {
        let context_limit = match self.models.resolve(model_route).await {
            Ok(model) => u64::try_from(model.context_limit).unwrap_or(0),
            Err(error) => {
                warn!(%error, model = model_route, "model route unavailable; using offline context limit");
                200_000
            }
        };
        ContextUsage {
            input_tokens: u64::try_from(content.chars().count() / 4).unwrap_or(0),
            output_tokens: u64::try_from(response.chars().count() / 4).unwrap_or(0),
            cached_tokens: u64::try_from(system_prompt.chars().count() / 4).unwrap_or(0),
            context_used: u64::try_from(
                (system_prompt.chars().count()
                    + content.chars().count()
                    + response.chars().count())
                    / 4,
            )
            .unwrap_or(0),
            context_limit,
        }
    }

    async fn emit_coalesced(
        &self,
        session_id: &str,
        run_id: &str,
        agent_id: &str,
        deltas: Vec<CoalescedDelta>,
    ) -> RuntimeResult<()> {
        for delta in deltas {
            let kind = match delta.kind {
                DeltaKind::Thinking => SessionEventKind::ThinkingDelta { delta: delta.delta },
                DeltaKind::Text => SessionEventKind::TextDelta { delta: delta.delta },
            };
            self.emit(session_id, run_id, agent_id, None, kind).await?;
        }
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
    async fn create_report(
        &self,
        _session_id: &str,
        _agent_id: &str,
        _title: &str,
    ) -> RuntimeResult<LinkedDoc> {
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

fn validate_question_answers(
    questions: &[QuestionItem],
    answers: &[QuestionAnswer],
) -> RuntimeResult<()> {
    let questions: HashMap<&str, &QuestionItem> = questions
        .iter()
        .map(|question| (question.id.as_str(), question))
        .collect();
    for answer in answers {
        let question = questions.get(answer.question_id.as_str()).ok_or_else(|| {
            RuntimeError::InvalidRequest(format!(
                "answer references unknown question `{}`",
                answer.question_id
            ))
        })?;
        if answer.skipped {
            continue;
        }
        for option_id in &answer.selected_option_ids {
            if !question
                .options
                .iter()
                .any(|option| option.id == *option_id)
            {
                return Err(RuntimeError::InvalidRequest(format!(
                    "answer for `{}` references unknown option `{option_id}`",
                    answer.question_id
                )));
            }
        }
    }
    Ok(())
}

fn safe_attachment_filename(filename: &str) -> RuntimeResult<&str> {
    let trimmed = filename.trim();
    if trimmed.is_empty()
        || trimmed.contains("..")
        || trimmed.contains('/')
        || trimmed.contains('\\')
    {
        return Err(RuntimeError::InvalidRequest(
            "attachment filename must be a plain file name".into(),
        ));
    }
    let Some(name) = Path::new(trimmed)
        .file_name()
        .and_then(|name| name.to_str())
    else {
        return Err(RuntimeError::InvalidRequest(
            "attachment filename must be valid UTF-8".into(),
        ));
    };
    if name != trimmed {
        return Err(RuntimeError::InvalidRequest(
            "attachment filename must not contain a path".into(),
        ));
    }
    Ok(name)
}
