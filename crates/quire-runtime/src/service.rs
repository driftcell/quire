//! Session orchestration, run lifecycle, recovery and prompt-cache handling.

use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use base64::Engine as _;
use futures::StreamExt;
use quire_protocol::{short_id, short_token};
use quire_session_protocol::{
    ActiveRunStatus, AttachmentMeta, ContextUsage, MessageRole, PendingQuestion, PendingQueueItem,
    PendingQueueSource, QuestionAnswer, QuestionItem, RunTraceSummary, SendMessageRequest,
    SessionCommentRef, SessionDetail, SessionEvent, SessionEventKind, SessionMessage,
    SessionStatus, SessionSummary, ToolCallDetail,
};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;
use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::{
    bus::InterruptCommand,
    comment_refs::render_stored_user_message,
    compaction::{
        COMPACTION_INSTRUCTION, COMPACTION_MODE_MESSAGE, COMPACTION_TIMEOUT_SECS,
        assemble_context_history, estimate_context_tokens, validate_compaction_summary,
    },
    config::Config,
    control::{RunRegistry, spawn_interrupt_listener},
    error::{RuntimeError, RuntimeResult},
    events::{EventScope, EventSink, project},
    kb::{KbService, SeedReport},
    mcp_client::RmcpClient,
    mcp_registry::McpRegistry,
    model_stream::{ModelStreamEvent, ModelStreamProvider, ModelTurnRequest, StubStreamProvider},
    models::ModelRegistry,
    prompt::{ContextMessage, ResumeContext, build_resume_context},
    store::{AttachmentInsert, CompactionWrite, RunRecord, RuntimeStore, SessionPatch},
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
                    "idempotency_key": format!("runtime:{}", short_token(16)),
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

#[derive(Debug, Clone)]
pub enum PostMessageOutcome {
    Started {
        message: SessionMessage,
        run_id: String,
    },
    Replayed {
        message: SessionMessage,
    },
    Queued {
        item: PendingQueueItem,
    },
}

/// Presign TTL for browser PUT uploads.
pub const ATTACHMENT_PRESIGN_TTL_SECS: u64 = 900;
/// Short-lived GET TTL for downloads inside the sandbox VPC.
pub const SANDBOX_ATTACHMENT_GET_TTL_SECS: u64 = 600;

/// Browser-facing presign result. The signed URL is never persisted or traced.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum AttachmentPresign {
    Oss {
        attachment_id: String,
        object_key: String,
        upload_url: String,
        headers: BTreeMap<String, String>,
    },
    Inline,
}

/// Input for confirming a local or OSS-backed attachment.
pub struct AttachmentWrite<'a> {
    pub session_id: &'a str,
    pub attachment_id: Option<&'a str>,
    pub filename: &'a str,
    pub mime_type: &'a str,
    pub size_bytes: u64,
    pub content_base64: Option<&'a str>,
    pub object_key: Option<&'a str>,
}

struct CompactionFlagGuard {
    store: RuntimeStore,
    session_id: String,
    started_at: chrono::DateTime<chrono::Utc>,
    armed: bool,
}

impl CompactionFlagGuard {
    fn new(
        store: RuntimeStore,
        session_id: &str,
        started_at: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        Self {
            store,
            session_id: session_id.to_string(),
            started_at,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CompactionFlagGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let store = self.store.clone();
        let session_id = self.session_id.clone();
        let started_at = self.started_at;
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = store
                    .clear_compaction_if_started(&session_id, started_at)
                    .await;
            });
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PendingBatch {
    pub message: SessionMessage,
}

/// Single source of truth for pending-queue dequeue, persistence and event broadcast.
#[derive(Clone)]
pub struct PendingQueueCoordinator {
    store: RuntimeStore,
    session_gates: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    notify: Arc<Notify>,
}

impl PendingQueueCoordinator {
    fn new(store: RuntimeStore) -> Self {
        Self {
            store,
            session_gates: Arc::new(Mutex::new(HashMap::new())),
            notify: Arc::new(Notify::new()),
        }
    }

    async fn session_gate(&self, session_id: &str) -> Arc<Mutex<()>> {
        let mut gates = self.session_gates.lock().await;
        gates
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    async fn notified(&self) {
        self.notify.notified().await;
    }

    fn notify_one(&self) {
        self.notify.notify_one();
    }

    /// Atomically take the next queue batch while the caller already owns the session gate.
    pub(crate) async fn take_pending_batch_locked(
        &self,
        session_id: &str,
        run_id: &str,
        inject_into_run: bool,
    ) -> RuntimeResult<Option<PendingBatch>> {
        let items = self.store.list_pending_queue(session_id).await?;
        let Some(first) = items.first() else {
            return Ok(None);
        };
        let mut batch = vec![first.clone()];
        if first.source == PendingQueueSource::Comment {
            for item in items.iter().skip(1) {
                if item.source != PendingQueueSource::Comment {
                    break;
                }
                batch.push(item.clone());
            }
        }
        let content = batch
            .iter()
            .map(|item| item.content.trim())
            .filter(|content| !content.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        let comment_refs = batch
            .iter()
            .flat_map(|item| item.comment_refs.clone())
            .collect::<Vec<_>>();
        let mut attachment_ids = Vec::new();
        for item in &batch {
            for attachment_id in &item.attachment_ids {
                if !attachment_ids.contains(attachment_id) {
                    attachment_ids.push(attachment_id.clone());
                }
            }
        }
        let client_msg_id = format!("queue:{}", first.queue_id);
        let request = SendMessageRequest {
            client_msg_id,
            content,
            comment_refs,
            attachment_ids,
        };
        let queue_ids = batch
            .iter()
            .map(|item| item.queue_id.clone())
            .collect::<Vec<_>>();
        let Some(message) = self
            .store
            .claim_pending_batch(session_id, run_id, &request, &queue_ids, inject_into_run)
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(PendingBatch { message }))
    }

    /// Release the in-memory gate when a session is deleted.
    pub(crate) async fn remove_session_gate(&self, session_id: &str) {
        self.session_gates.lock().await.remove(session_id);
    }

    /// Acquire the session gate and take one pending batch.
    pub(crate) async fn take_pending_batch(
        &self,
        session_id: &str,
        run_id: &str,
    ) -> RuntimeResult<Option<PendingBatch>> {
        let gate = self.session_gate(session_id).await;
        let _guard = gate.lock_owned().await;
        self.take_pending_batch_locked(session_id, run_id, true)
            .await
    }

    async fn emit_queue_update(&self, session_id: &str) -> RuntimeResult<()> {
        let items = self.store.list_pending_queue(session_id).await?;
        self.store
            .append_event(
                session_id,
                "queue",
                "system",
                None,
                SessionEventKind::PendingQueueUpdated { items },
            )
            .await?;
        Ok(())
    }
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
    queue: PendingQueueCoordinator,
}

impl RuntimeService {
    pub fn new(
        store: RuntimeStore,
        kb: KbService,
        models: Arc<ModelRegistry>,
        config: Arc<Config>,
    ) -> Self {
        let mcp = Arc::new(McpRegistry::new(store.clone(), config.master_key.clone()));
        let queue = PendingQueueCoordinator::new(store.clone());
        Self {
            store,
            kb,
            models,
            mcp,
            runs: RunRegistry::default(),
            config,
            model_stream: Arc::new(StubStreamProvider),
            queue,
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

    /// Shared queue coordinator used by the idle drain loop and in-run tool boundaries.
    pub fn pending_queue(&self) -> PendingQueueCoordinator {
        self.queue.clone()
    }

    pub fn runs(&self) -> &RunRegistry {
        &self.runs
    }

    pub async fn boot(&self) -> RuntimeResult<usize> {
        let _handle = self.kb.spawn_invalidation_listener();
        let _models = self.models.spawn_invalidation_listener();
        let _mcp = self.mcp.spawn_invalidation_listener();
        let _interrupt = spawn_interrupt_listener(self.store.bus().clone(), self.runs.clone());
        let queue_service = self.clone();
        let _queue = tokio::spawn(async move {
            queue_service.pending_queue_loop().await;
        });
        let recovered = self.store.recover_running_runs().await?;
        Ok(recovered)
    }

    async fn pending_queue_loop(self) {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            tokio::select! {
                _ = self.queue.notified() => {}
                _ = interval.tick() => {}
            }
            let sessions = match self.store.list_sessions_with_pending_queue().await {
                Ok(sessions) => sessions,
                Err(error) => {
                    warn!(%error, "failed to list sessions with pending queues");
                    continue;
                }
            };
            for session_id in sessions {
                if let Err(error) = self.drain_pending(&session_id).await {
                    warn!(%error, %session_id, "failed to drain pending session queue");
                }
            }
        }
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
        patch: SessionPatch<'_>,
    ) -> RuntimeResult<SessionSummary> {
        self.store.update_session(session_id, patch).await
    }

    pub async fn delete_session(&self, session_id: &str) -> RuntimeResult<()> {
        let result = self.store.delete_session(session_id).await;
        if result.is_ok() {
            self.queue.remove_session_gate(session_id).await;
        }
        result
    }

    async fn session_gate(&self, session_id: &str) -> Arc<Mutex<()>> {
        self.queue.session_gate(session_id).await
    }

    pub async fn post_message(
        &self,
        session_id: &str,
        request: &SendMessageRequest,
    ) -> RuntimeResult<PostMessageOutcome> {
        self.post_message_with_source(session_id, request, PendingQueueSource::User, None)
            .await
    }

    /// Submit an auto-generated comment review message. Queued instances keep
    /// their `Comment` source so consecutive comments can be merged and queue
    /// deletion can remove the still-open server thread.
    pub async fn post_comment_message(
        &self,
        session_id: &str,
        comment_ref: SessionCommentRef,
    ) -> RuntimeResult<PostMessageOutcome> {
        let thread_id = comment_ref.thread_id.clone();
        let client_msg_id = format!("comment:{thread_id}");
        self.post_comment_message_with_id(session_id, &thread_id, &client_msg_id, comment_ref)
            .await
    }

    /// Re-submit an addressed comment thread after a human follow-up reply.
    /// The reply message ID keeps retries idempotent while allowing one run per
    /// new instruction.
    pub async fn post_comment_reply_message(
        &self,
        session_id: &str,
        reply_message_id: &str,
        comment_ref: SessionCommentRef,
    ) -> RuntimeResult<PostMessageOutcome> {
        let thread_id = comment_ref.thread_id.clone();
        let client_msg_id = format!("comment:{thread_id}:{reply_message_id}");
        self.post_comment_message_with_id(session_id, &thread_id, &client_msg_id, comment_ref)
            .await
    }

    async fn post_comment_message_with_id(
        &self,
        session_id: &str,
        thread_id: &str,
        client_msg_id: &str,
        comment_ref: SessionCommentRef,
    ) -> RuntimeResult<PostMessageOutcome> {
        if comment_ref.thread_id != thread_id {
            return Err(RuntimeError::InvalidRequest(
                "comment reference thread_id must match the comment queue thread".into(),
            ));
        }
        let request = SendMessageRequest {
            client_msg_id: client_msg_id.to_string(),
            content: String::new(),
            comment_refs: vec![comment_ref],
            attachment_ids: Vec::new(),
        };
        self.post_message_with_source(
            session_id,
            &request,
            PendingQueueSource::Comment,
            Some(thread_id),
        )
        .await
    }

    async fn post_message_with_source(
        &self,
        session_id: &str,
        request: &SendMessageRequest,
        source: PendingQueueSource,
        thread_id: Option<&str>,
    ) -> RuntimeResult<PostMessageOutcome> {
        let gate = self.session_gate(session_id).await;
        let _guard = gate.lock_owned().await;
        let session = self.store.get_session_summary(session_id).await?;
        if session.status == SessionStatus::Archived {
            return Err(RuntimeError::InvalidRequest(
                "archived sessions cannot receive new messages".into(),
            ));
        }
        if let Some(message) = self
            .store
            .find_message_by_client_id(session_id, &request.client_msg_id)
            .await?
        {
            return Ok(PostMessageOutcome::Replayed { message });
        }
        let awaiting_input = session.active_run_status == ActiveRunStatus::AwaitingInput;
        let active = !awaiting_input
            && (session.active_run_status == ActiveRunStatus::Running
                || self.store.latest_running_run(session_id).await?.is_some());
        let queue_non_empty =
            !awaiting_input && !self.store.list_pending_queue(session_id).await?.is_empty();
        if source == PendingQueueSource::Comment
            && let Some(thread_id) = thread_id
            && let Some(existing) = self
                .store
                .pending_comment_item_for_thread(session_id, thread_id)
                .await?
        {
            let item = self
                .store
                .update_pending_message(
                    session_id,
                    &existing.queue_id,
                    &request.content,
                    &request.comment_refs,
                )
                .await?;
            self.emit_queue_update(session_id).await?;
            if !active && !awaiting_input {
                self.drain_pending_locked(session_id, true).await?;
            }
            return Ok(PostMessageOutcome::Queued { item });
        }
        if active || queue_non_empty {
            let item = self
                .store
                .enqueue_pending_message(
                    session_id,
                    &request.client_msg_id,
                    &request.content,
                    &request.comment_refs,
                    source,
                    thread_id,
                    &request.attachment_ids,
                )
                .await?;
            self.emit_queue_update(session_id).await?;
            if !active {
                self.drain_pending_locked(session_id, true).await?;
            }
            return Ok(PostMessageOutcome::Queued { item });
        }
        let (message, inserted) = self.store.insert_user_message(session_id, request).await?;
        if !inserted {
            return Ok(PostMessageOutcome::Replayed { message });
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
        let message_id = message.message_id.clone();
        let model_input = render_stored_user_message(&message);
        let run_id = self
            .spawn_turn_for_message(session_id, &message_id, model_input)
            .await?;
        Ok(PostMessageOutcome::Started { message, run_id })
    }

    pub async fn list_pending_queue(
        &self,
        session_id: &str,
    ) -> RuntimeResult<Vec<PendingQueueItem>> {
        self.store.list_pending_queue(session_id).await
    }

    pub async fn update_pending_message(
        &self,
        session_id: &str,
        queue_id: &str,
        content: &str,
        comment_refs: &[SessionCommentRef],
    ) -> RuntimeResult<PendingQueueItem> {
        let gate = self.session_gate(session_id).await;
        let _guard = gate.lock_owned().await;
        let item = self
            .store
            .update_pending_message(session_id, queue_id, content, comment_refs)
            .await?;
        self.emit_queue_update(session_id).await?;
        Ok(item)
    }

    pub async fn delete_pending_message(
        &self,
        session_id: &str,
        queue_id: &str,
    ) -> RuntimeResult<bool> {
        let gate = self.session_gate(session_id).await;
        let _guard = gate.lock_owned().await;
        let item = self
            .store
            .get_pending_queue_item(session_id, queue_id)
            .await?;
        let deleted = self
            .store
            .delete_pending_message(session_id, queue_id)
            .await?;
        if deleted {
            if let Some(item) = item
                && item.source == PendingQueueSource::Comment
                && let Some(thread_id) = item.thread_id.as_deref()
                && let Err(error) = self.delete_server_comment(session_id, thread_id).await
            {
                warn!(
                    %error,
                    %session_id,
                    %thread_id,
                    "failed to delete the comment linked to a removed queue item"
                );
            }
            self.emit_queue_update(session_id).await?;
        }
        Ok(deleted)
    }

    pub async fn remove_pending_comment_threads(
        &self,
        session_id: &str,
        thread_id: &str,
    ) -> RuntimeResult<u64> {
        let gate = self.session_gate(session_id).await;
        let _guard = gate.lock_owned().await;
        let removed = self
            .store
            .remove_pending_comment_thread(session_id, thread_id)
            .await?;
        if removed > 0 {
            self.emit_queue_update(session_id).await?;
        }
        Ok(removed)
    }

    async fn delete_server_comment(&self, session_id: &str, thread_id: &str) -> RuntimeResult<()> {
        let session = self.store.get_session_summary(session_id).await?;
        let doc_id = session
            .doc_id
            .filter(|doc_id| !doc_id.trim().is_empty())
            .or_else(|| {
                session
                    .linked_doc_id
                    .filter(|doc_id| !doc_id.trim().is_empty())
            })
            .ok_or_else(|| {
                RuntimeError::InvalidRequest(format!(
                    "session `{session_id}` is not bound to a report"
                ))
            })?;
        let token = self.config.quire_rw_token.expose_secret().trim();
        if token.is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "QUIRE_RW_TOKEN is not configured on quire-runtime".into(),
            ));
        }
        let response = crate::http_client::timed_client(Duration::from_secs(30))
            .delete(format!(
                "{}/api/docs/{}/comments/{}",
                self.config.quire_base_url.trim_end_matches('/'),
                doc_id,
                thread_id
            ))
            .bearer_auth(token)
            .header("x-session-id", session_id)
            .header("x-actor", "reviewer")
            .send()
            .await?;
        if !response.status().is_success() && response.status() != reqwest::StatusCode::NOT_FOUND {
            return Err(RuntimeError::Internal(format!(
                "quire-server comment delete failed with {}",
                response.status()
            )));
        }
        Ok(())
    }

    async fn emit_queue_update(&self, session_id: &str) -> RuntimeResult<()> {
        self.queue.emit_queue_update(session_id).await
    }

    async fn drain_pending_locked(
        &self,
        session_id: &str,
        force: bool,
    ) -> RuntimeResult<Option<String>> {
        let session = self.store.get_session_summary(session_id).await?;
        if !force && session.active_run_status != ActiveRunStatus::Idle {
            return Ok(None);
        }
        if self.store.latest_running_run(session_id).await?.is_some() {
            return Ok(None);
        }
        let Some(batch) = self
            .queue
            .take_pending_batch_locked(session_id, "queue", false)
            .await?
        else {
            return Ok(None);
        };
        let model_input = render_stored_user_message(&batch.message);
        let run_id = self
            .start_turn_for_stored_message(session_id, &batch.message.message_id, model_input)
            .await?;
        Ok(Some(run_id))
    }

    async fn drain_pending(&self, session_id: &str) -> RuntimeResult<Option<String>> {
        let gate = self.session_gate(session_id).await;
        let _guard = gate.lock_owned().await;
        self.drain_pending_locked(session_id, false).await
    }

    /// Resolve a pending ask_user card and continue the interrupted reasoning immediately.
    pub async fn answer_question(
        &self,
        session_id: &str,
        question_id: &str,
        answers: &[QuestionAnswer],
        skipped: bool,
    ) -> RuntimeResult<SessionDetail> {
        let gate = self.session_gate(session_id).await;
        let _guard = gate.lock_owned().await;
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

    pub async fn presign_attachment(
        &self,
        session_id: &str,
        filename: &str,
        mime_type: &str,
        size_bytes: u64,
    ) -> RuntimeResult<AttachmentPresign> {
        self.store.get_session_summary(session_id).await?;
        let filename = safe_attachment_filename(filename)?;
        let mime_type = mime_type.trim();
        if mime_type.is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "attachment mime_type must not be empty".into(),
            ));
        }
        if i64::try_from(size_bytes).is_err() {
            return Err(RuntimeError::InvalidRequest(
                "attachment size_bytes is out of range".into(),
            ));
        }
        if !self.config.oss.is_configured() {
            return Ok(AttachmentPresign::Inline);
        }
        let attachment_id = short_id("att");
        let object_key = self
            .config
            .oss
            .object_key(session_id, &attachment_id, filename);
        let upload_url =
            self.config
                .oss
                .presign_put(&object_key, mime_type, ATTACHMENT_PRESIGN_TTL_SECS)?;
        let headers = BTreeMap::from([("Content-Type".to_string(), mime_type.to_string())]);
        Ok(AttachmentPresign::Oss {
            attachment_id,
            object_key,
            upload_url,
            headers,
        })
    }

    pub async fn create_attachment(
        &self,
        upload: AttachmentWrite<'_>,
    ) -> RuntimeResult<AttachmentMeta> {
        self.store.get_session_summary(upload.session_id).await?;
        let filename = safe_attachment_filename(upload.filename)?;
        let attachment_id = upload.attachment_id.map(safe_attachment_id).transpose()?;
        if let Some(object_key) = upload.object_key {
            let object_key = object_key.trim();
            self.validate_oss_object_key(upload.session_id, object_key)?;
            return self
                .store
                .insert_attachment(AttachmentInsert {
                    session_id: upload.session_id,
                    attachment_id,
                    filename,
                    mime_type: upload.mime_type,
                    size_bytes: upload.size_bytes,
                    storage_path: object_key,
                    storage_provider: "oss",
                })
                .await;
        }

        let bytes = match upload.content_base64 {
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
        let path = storage_dir.join(format!("{}-{filename}", short_token(12)));
        tokio::fs::write(&path, &bytes).await?;
        self.store
            .insert_attachment(AttachmentInsert {
                session_id: upload.session_id,
                attachment_id,
                filename,
                mime_type: upload.mime_type,
                size_bytes: upload.size_bytes,
                storage_path: &path.to_string_lossy(),
                storage_provider: "local",
            })
            .await
    }

    fn validate_oss_object_key(&self, session_id: &str, object_key: &str) -> RuntimeResult<()> {
        if !self.config.oss.is_configured() {
            return Err(RuntimeError::InvalidRequest(
                "OSS attachments are not configured".into(),
            ));
        }
        if !self.config.oss.owns_object_key(session_id, object_key) {
            return Err(RuntimeError::InvalidRequest(
                "attachment object_key does not belong to this session".into(),
            ));
        }
        Ok(())
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
        let _ = self.runs.cancel(&run.run_id).await;
        if let Err(error) = self
            .store
            .bus()
            .publish_interrupt(&InterruptCommand {
                session_id: session_id.to_string(),
                run_id: run.run_id.clone(),
            })
            .await
        {
            warn!(%error, %session_id, run_id = %run.run_id, "failed to publish cross-instance interrupt; continuing local cancellation");
        }

        // Let the owning turn flush its remaining deltas and finish itself. Only
        // a bounded fallback writes the terminal event here; the conditional
        // transition guarantees there is exactly one winner and one terminal frame.
        let mut finalized = false;
        for _ in 0..40 {
            if self.store.get_run(&run.run_id).await?.status != "running" {
                finalized = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        if !finalized {
            self.finish_cancelled_run(&run.run_id).await?;
        }
        self.runs.remove(&run.run_id).await;
        self.store.get_session_detail(session_id).await
    }

    pub async fn retry(&self, session_id: &str) -> RuntimeResult<SessionDetail> {
        let gate = self.session_gate(session_id).await;
        let _guard = gate.lock_owned().await;
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
        let model_input = render_stored_user_message(last_user);
        let _ = self
            .start_turn_for_stored_message(session_id, &last_user.message_id, model_input)
            .await?;
        self.store.get_session_detail(session_id).await
    }

    pub async fn compact(&self, session_id: &str) -> RuntimeResult<SessionDetail> {
        let gate = self.session_gate(session_id).await;
        let _guard = gate.lock_owned().await;
        self.compact_locked(session_id, None).await
    }

    async fn compact_locked(
        &self,
        session_id: &str,
        exclude_message_id: Option<&str>,
    ) -> RuntimeResult<SessionDetail> {
        let started_at = self.store.begin_compaction(session_id).await?;
        let mut flag_guard = CompactionFlagGuard::new(self.store.clone(), session_id, started_at);
        if let Err(error) = self
            .emit(
                session_id,
                "compaction",
                "main",
                None,
                SessionEventKind::CompactionStarted,
            )
            .await
        {
            let _ = self
                .store
                .clear_compaction_if_started(session_id, started_at)
                .await;
            flag_guard.disarm();
            return Err(error);
        }
        let result = self
            .compact_started(session_id, exclude_message_id, started_at)
            .await;
        match &result {
            Ok(_) => {
                let _ = self
                    .emit(
                        session_id,
                        "compaction",
                        "main",
                        None,
                        SessionEventKind::CompactionFinished { success: true },
                    )
                    .await;
            }
            Err(_) => {
                if let Err(error) = self
                    .store
                    .clear_compaction_if_started(session_id, started_at)
                    .await
                {
                    warn!(%error, %session_id, "failed to clear compaction state");
                }
                let _ = self
                    .emit(
                        session_id,
                        "compaction",
                        "main",
                        None,
                        SessionEventKind::CompactionFinished { success: false },
                    )
                    .await;
            }
        }
        flag_guard.disarm();
        result
    }

    async fn compact_started(
        &self,
        session_id: &str,
        exclude_message_id: Option<&str>,
        started_at: chrono::DateTime<chrono::Utc>,
    ) -> RuntimeResult<SessionDetail> {
        let session = self.store.get_session_summary(session_id).await?;
        if session.active_run_status == ActiveRunStatus::Running
            || self.store.latest_running_run(session_id).await?.is_some()
        {
            return Err(RuntimeError::CompactionConflict(
                "session has an active run; wait for it to finish before compacting".into(),
            ));
        }

        let state = self.store.compaction_state(session_id).await?;
        let messages = self.store.list_messages(session_id).await?;
        let start = state
            .up_to_message_id
            .as_deref()
            .and_then(|anchor| {
                messages
                    .iter()
                    .position(|message| message.message_id == anchor)
                    .map(|index| index.saturating_add(1))
            })
            .unwrap_or(0);
        let new_messages: Vec<&SessionMessage> = messages
            .iter()
            .skip(start)
            .filter(|message| {
                matches!(
                    message.role,
                    MessageRole::User | MessageRole::Assistant | MessageRole::Tool
                ) && Some(message.message_id.as_str()) != exclude_message_id
            })
            .collect();

        if new_messages.is_empty() {
            let prompt = self
                .kb
                .system_prompt_for(session.kb_branch.as_deref())
                .await?;
            let version = self.kb.current_version().await?;
            self.store
                .set_pinned_prompt(session_id, prompt.as_str(), version)
                .await?;
            self.store
                .clear_compaction_if_started(session_id, started_at)
                .await?;
            return self.store.get_session_detail(session_id).await;
        }

        let (old_prompt, synced_version) = match self.store.pinned_prompt(session_id).await? {
            (Some(prompt), version) => (prompt, version),
            (None, version) => (
                self.kb
                    .system_prompt_for(session.kb_branch.as_deref())
                    .await?
                    .as_str()
                    .to_string(),
                version,
            ),
        };
        let history_messages: Vec<SessionMessage> = messages
            .iter()
            .filter(|message| Some(message.message_id.as_str()) != exclude_message_id)
            .cloned()
            .collect();
        let history = assemble_context_history(
            &history_messages,
            &state,
            session.doc_id.as_deref(),
            synced_version,
        );
        let mut history = history;
        history.push(ContextMessage {
            role: MessageRole::System,
            content: COMPACTION_MODE_MESSAGE.to_string(),
        });
        let agent = self
            .kb
            .get_default_agent_on(session.kb_branch.as_deref())
            .await?;
        let model_route = agent
            .as_ref()
            .map(|agent| agent.model.clone())
            .unwrap_or_else(|| self.config.default_model.clone());
        let request = ModelTurnRequest {
            session_id: session_id.to_string(),
            run_id: "compaction".into(),
            agent_id: "main".into(),
            system_prompt: old_prompt,
            user_message: COMPACTION_INSTRUCTION.to_string(),
            history,
            doc_id: session.doc_id.clone(),
            kb_branch: session.kb_branch.clone(),
            model_route,
            allowed_tools: Vec::new(),
            disable_tools: true,
            cancellation: CancellationToken::new(),
        };
        let mut stream = self.model_stream.stream_turn(request).await?;
        let summary = tokio::time::timeout(Duration::from_secs(COMPACTION_TIMEOUT_SECS), async {
            let mut summary = String::new();
            while let Some(item) = stream.next().await {
                if let ModelStreamEvent::TextDelta(delta) = item? {
                    summary.push_str(&delta);
                }
            }
            Ok::<String, RuntimeError>(summary.trim().to_string())
        })
        .await
        .map_err(|_| RuntimeError::Internal("compaction summary timed out".into()))??;
        if summary.is_empty() {
            return Err(RuntimeError::Internal(
                "compaction summary was empty".into(),
            ));
        }
        if !validate_compaction_summary(&summary) {
            tracing::warn!(
                summary_len = summary.len(),
                session_id,
                "compaction summary validation failed"
            );
            return Err(RuntimeError::Internal(
                "compaction summary did not match the required structured handoff".into(),
            ));
        }

        let latest_prompt = self
            .kb
            .system_prompt_for(session.kb_branch.as_deref())
            .await?;
        let latest_version = self.kb.current_version().await?;
        let estimated_tokens = estimate_context_tokens(&summary);
        let context_usage = ContextUsage {
            input_tokens: estimated_tokens,
            output_tokens: 0,
            cached_tokens: 0,
            context_used: estimated_tokens,
            context_limit: session.context_usage.context_limit,
        };
        let up_to_message_id = new_messages
            .last()
            .map(|message| message.message_id.clone())
            .unwrap_or_default();
        self.store
            .apply_compaction(&CompactionWrite {
                session_id,
                summary: &summary,
                up_to_message_id: &up_to_message_id,
                marker_before_message_id: exclude_message_id,
                pinned_system_prompt: latest_prompt.as_str(),
                kb_version: latest_version,
                context_usage: &context_usage,
            })
            .await?;
        self.store.get_session_detail(session_id).await
    }

    async fn maybe_auto_compact(&self, session_id: &str, exclude_message_id: Option<&str>) {
        let Ok(session) = self.store.get_session_summary(session_id).await else {
            return;
        };
        if session.active_run_status != ActiveRunStatus::Idle || session.compacting {
            return;
        }
        let usage = &session.context_usage;
        if usage.context_limit == 0
            || usage.context_used.saturating_mul(100) < usage.context_limit.saturating_mul(80)
        {
            return;
        }
        if let Err(error) = self.compact_locked(session_id, exclude_message_id).await {
            warn!(%error, %session_id, "automatic context compaction failed; continuing turn");
        }
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
        let tool_call_id = short_id("tc");
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
        let session = self.store.get_session_summary(session_id).await?;
        let state = self.store.compaction_state(session_id).await?;
        let messages = self.store.list_messages(session_id).await?;
        let history =
            assemble_context_history(&messages, &state, session.doc_id.as_deref(), pinned.1);
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

    async fn prepare_turn(&self, session_id: &str) -> RuntimeResult<(String, CancellationToken)> {
        self.store.get_session_summary(session_id).await?;
        if self.store.latest_running_run(session_id).await?.is_some() {
            return Err(RuntimeError::InvalidRequest(format!(
                "session `{session_id}` already has an active run; queue the message instead"
            )));
        }
        let run = self
            .store
            .create_run(session_id, None, "main", &self.config.default_model)
            .await?;
        let token = self.runs.register(&run.run_id).await;
        Ok((run.run_id, token))
    }

    fn launch_turn(
        &self,
        session_id: &str,
        run_id: &str,
        content: String,
        current_message_id: Option<String>,
        token: CancellationToken,
    ) {
        let service = self.clone();
        let run_id = run_id.to_string();
        let session_id = session_id.to_string();
        tokio::spawn(async move {
            if let Err(error) = service
                .run_turn(
                    &session_id,
                    &run_id,
                    &content,
                    current_message_id.as_deref(),
                    token,
                )
                .await
            {
                warn!(%error, %session_id, run_id, "agent turn failed");
            }
        });
    }

    async fn spawn_turn(&self, session_id: &str, content: String) -> RuntimeResult<String> {
        self.maybe_auto_compact(session_id, None).await;
        let (run_id, token) = self.prepare_turn(session_id).await?;
        self.launch_turn(session_id, &run_id, content, None, token);
        Ok(run_id)
    }

    async fn start_turn_for_stored_message(
        &self,
        session_id: &str,
        message_id: &str,
        content: String,
    ) -> RuntimeResult<String> {
        self.maybe_auto_compact(session_id, Some(message_id)).await;
        let (run_id, token) = self.prepare_turn(session_id).await?;
        self.launch_turn(
            session_id,
            &run_id,
            content,
            Some(message_id.to_string()),
            token,
        );
        Ok(run_id)
    }

    async fn spawn_turn_for_message(
        &self,
        session_id: &str,
        message_id: &str,
        content: String,
    ) -> RuntimeResult<String> {
        self.maybe_auto_compact(session_id, Some(message_id)).await;
        let (run_id, token) = self.prepare_turn(session_id).await?;
        if let Err(error) = self
            .emit(
                session_id,
                &run_id,
                "main",
                None,
                SessionEventKind::UserMessageAdded {
                    message_id: message_id.to_string(),
                },
            )
            .await
        {
            warn!(%error, %session_id, %run_id, "failed to broadcast user message");
        }
        self.launch_turn(
            session_id,
            &run_id,
            content,
            Some(message_id.to_string()),
            token,
        );
        Ok(run_id)
    }

    async fn run_turn(
        &self,
        session_id: &str,
        run_id: &str,
        content: &str,
        current_message_id: Option<&str>,
        token: CancellationToken,
    ) -> RuntimeResult<()> {
        let result = self
            .execute_turn(session_id, run_id, content, current_message_id, &token)
            .await;
        self.runs.remove(run_id).await;
        match result {
            Ok(()) => {
                if token.is_cancelled() {
                    return Ok(());
                }
                let session = self.store.get_session_summary(session_id).await?;
                if session.active_run_status != ActiveRunStatus::AwaitingInput {
                    self.queue.notify_one();
                }
                Ok(())
            }
            Err(error) => {
                let was_cancelled = token.is_cancelled();
                // Stop the producer before reporting failure: a persistence or mapping error
                // must not leave the agent executing tools after the run is marked as failed.
                token.cancel();
                if !was_cancelled {
                    let message = error.to_string();
                    let _ = self
                        .store
                        .finish_run_with_event(
                            run_id,
                            "failed",
                            Some(&message),
                            SessionEventKind::TurnFailed {
                                error: message.clone(),
                                recoverable: true,
                            },
                        )
                        .await;
                }
                Err(error)
            }
        }
    }

    /// Finish a user-cancelled run after the turn has flushed its deltas.
    /// A concurrent finisher simply makes this a no-op.
    async fn finish_cancelled_run(&self, run_id: &str) -> RuntimeResult<()> {
        let _ = self
            .store
            .finish_run_with_event(
                run_id,
                "cancelled",
                Some("interrupted by user"),
                SessionEventKind::TurnFailed {
                    error: "interrupted by user".into(),
                    recoverable: true,
                },
            )
            .await?;
        Ok(())
    }

    async fn execute_turn(
        &self,
        session_id: &str,
        run_id: &str,
        content: &str,
        current_message_id: Option<&str>,
        token: &CancellationToken,
    ) -> RuntimeResult<()> {
        let session = self.store.get_session_summary(session_id).await?;
        let (pinned, synced_version) = self.ensure_pinned_prompt(session_id).await?;
        let kb_version = self.kb.current_version().await?;
        let state = self.store.compaction_state(session_id).await?;
        let mut messages = self.store.list_messages(session_id).await?;
        if let Some(message_id) = current_message_id
            && let Some(index) = messages
                .iter()
                .rposition(|message| message.message_id == message_id)
        {
            messages.remove(index);
        }
        let history =
            assemble_context_history(&messages, &state, session.doc_id.as_deref(), synced_version);
        let context = build_resume_context(&pinned, &history, content, kb_version, synced_version);
        if token.is_cancelled() {
            self.finish_cancelled_run(run_id).await?;
            return Ok(());
        }
        let mut sink = EventSink::new(session_id, self.store.clone());
        let main_scope = EventScope::main(run_id);
        self.store
            .start_run_with_event(run_id, SessionEventKind::TurnStarted)
            .await?;
        if token.is_cancelled() {
            self.finish_cancelled_run(run_id).await?;
            return Ok(());
        }
        let branch = session.kb_branch.clone();
        let agent = self.kb.get_default_agent_on(branch.as_deref()).await?;
        let model_route = agent
            .as_ref()
            .map(|agent| agent.model.clone())
            .unwrap_or_else(|| self.config.default_model.clone());
        let allowed_tools = agent.map(|agent| agent.allowed_tools).unwrap_or_default();
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
            disable_tools: false,
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
                    sink.flush().await?;
                    self.finish_cancelled_run(run_id).await?;
                    return Ok(());
                }
                _ = tick.tick() => {
                    sink.flush_due(Instant::now()).await?;
                }
                item = stream.next() => {
                    match item {
                        Some(Ok(event)) => {
                            let action = project(main_scope.clone(), event);
                            if let Some(text) = action.main_text_delta() {
                                response.push_str(text);
                            }
                            let outcome = sink.apply(action).await?;
                            if let Some(reason) = outcome.ignored {
                                warn!(%session_id, %run_id, reason, "ignoring out-of-scope model event");
                            }
                            if let Some(usage) = outcome.main_usage {
                                reported_usage = Some(usage);
                            }
                            if let Some(question) = outcome.question {
                                suspended_question = Some(question);
                            }
                        }
                        Some(Err(error)) => {
                            sink.flush().await?;
                            return Err(error);
                        }
                        None => break,
                    }
                }
            }
        }
        sink.flush().await?;
        if token.is_cancelled() {
            self.finish_cancelled_run(run_id).await?;
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
                sink.emit(
                    main_scope.clone(),
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
            self.finish_cancelled_run(run_id).await?;
            return Ok(());
        }
        let Some(_terminal) = self
            .store
            .finish_run_with_event(run_id, "completed", None, SessionEventKind::TurnFinished)
            .await?
        else {
            // A user interrupt won the terminal transition; do not emit a second
            // terminal event or advance the session as a completed run.
            return Ok(());
        };
        if kb_version > synced_version {
            self.store
                .advance_synced_kb_version(session_id, kb_version)
                .await?;
        }
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

pub(crate) fn safe_attachment_id(attachment_id: &str) -> RuntimeResult<&str> {
    let trimmed = attachment_id.trim();
    if trimmed.is_empty()
        || trimmed.len() > 128
        || !trimmed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(RuntimeError::InvalidRequest(
            "attachment_id must contain only ASCII letters, digits, `_` or `-`".into(),
        ));
    }
    Ok(trimmed)
}

pub(crate) fn safe_attachment_filename(filename: &str) -> RuntimeResult<&str> {
    let trimmed = filename.trim();
    if trimmed.is_empty()
        || trimmed.contains("..")
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.chars().any(char::is_control)
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
