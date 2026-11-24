//! Session orchestration, run lifecycle, recovery and prompt-cache handling.

use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use base64::Engine as _;
use futures::StreamExt;
use quire_protocol::{short_id, short_token};
use quire_session_protocol::{
    ActiveRunStatus, AttachmentMeta, ContextUsage, MessageRole, PendingQuestion, PendingQueueItem,
    PendingQueueSource, QuestionAnswer, QuestionItem, RunTraceSummary, SendMessageRequest,
    SessionCommentRef, SessionDetail, SessionEvent, SessionEventKind, SessionMessage,
    SessionStatus, SessionSummary, ToolCallDetail,
};
use serde::Serialize;
use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::{
    bus::InterruptCommand,
    comment_refs::render_stored_user_message,
    compaction::{
        COMPACTION_INSTRUCTION, COMPACTION_MODE_MESSAGE, COMPACTION_TIMEOUT_SECS,
        estimate_context_tokens,
    },
    config::Config,
    control::{RunRegistry, RunTurnGuard, spawn_interrupt_listener},
    error::{RuntimeError, RuntimeResult},
    events::{EventScope, EventSink, project},
    kb::{KbService, SeedReport},
    mcp_registry::McpRegistry,
    model_stream::{ModelStreamEvent, ModelStreamProvider, ModelTurnRequest, StubStreamProvider},
    models::ModelRegistry,
    store::{
        AttachmentInsert, CompactionWrite, EnqueuePendingMessage, RunRecord, RuntimeStore,
        SessionPatch,
    },
};

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

#[derive(Serialize)]
struct AskUserDismissalPayload<'a> {
    kind: &'static str,
    question_id: &'a str,
    tool_call_id: &'a Option<String>,
    status: &'static str,
    reason: &'static str,
}

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

    pub(crate) async fn has_pending(&self, session_id: &str) -> RuntimeResult<bool> {
        Ok(!self.store.list_pending_queue(session_id).await?.is_empty())
    }

    fn notify_one(&self) {
        self.notify.notify_one();
    }

    /// Atomically take the next queue batch while the caller already owns the session gate.
    pub(crate) async fn take_pending_batch_locked(
        &self,
        session_id: &str,
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
            .claim_pending_batch(session_id, &request, &queue_ids)
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
        self.seed_initial_admin().await?;
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
        self.mcp.retire_legacy_quire_report().await?;
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
        self.create_session_for_user(None, title, doc_id, kb_branch)
            .await
    }

    pub async fn create_session_for_user(
        &self,
        user_id: Option<&str>,
        title: &str,
        doc_id: Option<&str>,
        kb_branch: Option<&str>,
    ) -> RuntimeResult<SessionSummary> {
        let branch = self.kb.session_branch(kb_branch).await?;
        self.store
            .create_session_for_user(user_id, title, doc_id, &branch)
            .await
    }

    pub async fn ensure_session_access(
        &self,
        session_id: &str,
        user_id: &str,
        is_admin: bool,
    ) -> RuntimeResult<()> {
        let owner = self.store.get_session_owner(session_id).await?;
        if is_admin || owner.as_deref() == Some(user_id) {
            return Ok(());
        }
        Err(RuntimeError::Forbidden(
            "you do not have permission to access this session".into(),
        ))
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

    pub async fn list_sessions_for_user(
        &self,
        user_id: Option<&str>,
        include_unowned: bool,
        limit: i64,
        offset: i64,
    ) -> RuntimeResult<Vec<SessionSummary>> {
        self.store
            .list_sessions_for_user(user_id, include_unowned, limit, offset)
            .await
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
}

pub(crate) use execute::{safe_attachment_filename, safe_attachment_id, validate_question_answers};

mod attachments;
mod auth;
mod control;
mod execute;
mod messages;
