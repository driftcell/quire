//! PostgreSQL persistence for sessions, messages, runs, events and tool calls.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use quire_protocol::short_id;
use quire_session_protocol::{
    ActiveRunStatus, AttachmentMeta, PendingQuestion, PendingQueueItem, PendingQueueSource,
    QuestionAnswer, QuestionItem, ResolvedQuestion, RunTraceSegment, RunTraceSubagent,
    RunTraceSummary, SendMessageRequest, SessionCommentRef, SessionDetail, SessionEvent,
    SessionEventKind, SessionMessage, SessionStatus, SessionSummary, ToolCallDetail,
    aggregate_run_traces,
};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use tracing::warn;

use crate::{
    bus::RedisBus,
    comment_refs::validate_user_payload,
    error::{RuntimeError, RuntimeResult},
    memory::ConversationMemory,
};

pub struct CompactionWrite<'a> {
    pub session_id: &'a str,
    pub summary: &'a str,
    pub exclude_message_id: Option<&'a str>,
    pub pinned_system_prompt: &'a str,
    pub kb_version: u64,
    pub context_usage: &'a quire_session_protocol::ContextUsage,
}

pub struct EnqueuePendingMessage<'a> {
    pub session_id: &'a str,
    pub client_msg_id: &'a str,
    pub content: &'a str,
    pub comment_refs: &'a [SessionCommentRef],
    pub source: PendingQueueSource,
    pub thread_id: Option<&'a str>,
    pub attachment_ids: &'a [String],
}

#[derive(Debug, Clone)]
pub struct SessionEventDraft {
    pub run_id: String,
    pub agent_id: String,
    pub parent_run_id: Option<String>,
    pub kind: SessionEventKind,
}

#[derive(Debug, Clone)]
pub struct SessionEventFrame {
    pub seq: u64,
    pub event_type: String,
    pub event: Value,
}

#[derive(Debug, Clone)]
pub struct RunRecord {
    pub run_id: String,
    pub session_id: String,
    pub parent_run_id: Option<String>,
    pub agent_id: String,
    pub status: String,
    pub model: Option<String>,
    pub error: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    /// Sequence of this run's `turn_started` event, recorded in the same
    /// transaction. Active-run hydration uses it for a primary-key range scan.
    pub start_seq: Option<u64>,
}

/// Fields that may be changed on an existing session. `None` keeps the
/// current value; only title/status changes are considered content updates
/// and may advance `updated_at`.
#[derive(Debug, Clone, Default)]
pub struct SessionPatch<'a> {
    pub title: Option<&'a str>,
    pub status: Option<SessionStatus>,
    pub pinned: Option<bool>,
}

/// Values needed to persist an uploaded attachment without exposing storage
/// provenance to the session wire contract.
pub struct AttachmentInsert<'a> {
    pub session_id: &'a str,
    pub attachment_id: Option<&'a str>,
    pub filename: &'a str,
    pub mime_type: &'a str,
    pub size_bytes: u64,
    pub storage_path: &'a str,
    pub storage_provider: &'a str,
}

/// Internal attachment record including storage provenance. Never serialized
/// into `AttachmentMeta` or any model-visible payload.
#[derive(Debug, Clone)]
pub struct StoredAttachment {
    pub attachment_id: String,
    pub session_id: String,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub storage_path: String,
    pub storage_provider: String,
}

#[derive(Clone)]
pub struct RuntimeStore {
    pool: PgPool,
    rig_pool: PgPool,
    bus: RedisBus,
    report_store: quire_store::PgReportStore,
    memory: Arc<tokio::sync::OnceCell<ConversationMemory>>,
}

impl RuntimeStore {
    pub async fn connect(database_url: &str, redis_url: &str) -> RuntimeResult<Self> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(20)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(database_url)
            .await?;
        let rig_pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .after_connect(|connection, _metadata| {
                Box::pin(async move {
                    sqlx::query("SET search_path TO rig")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect(database_url)
            .await?;
        let report_bus = quire_store::RedisStreamBus::connect(redis_url)
            .await
            .map_err(|error| {
                RuntimeError::Internal(format!("report redis connect failed: {error}"))
            })?;
        let report_store = quire_store::PgReportStore::from_pool(pool.clone(), report_bus);
        Ok(Self {
            pool,
            rig_pool,
            bus: RedisBus::connect(redis_url)?,
            report_store,
            memory: Arc::new(tokio::sync::OnceCell::new()),
        })
    }

    pub fn from_pool(
        pool: PgPool,
        rig_pool: PgPool,
        bus: RedisBus,
        report_store: quire_store::PgReportStore,
    ) -> Self {
        Self {
            pool,
            rig_pool,
            bus,
            report_store,
            memory: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    pub fn report_store(&self) -> &quire_store::PgReportStore {
        &self.report_store
    }

    pub async fn migrate(&self) -> RuntimeResult<()> {
        self.report_store
            .migrate()
            .await
            .map_err(|error| RuntimeError::Internal(format!("report migration failed: {error}")))?;
        let mut migrator = sqlx::migrate!("./migrations");
        migrator.create_schema("runtime");
        migrator.dangerous_set_table_name("runtime._quire_runtime_migrations");
        migrator
            .run(&self.pool)
            .await
            .map_err(|error| RuntimeError::Internal(format!("runtime migration failed: {error}")))
    }

    pub async fn health(&self) -> RuntimeResult<()> {
        sqlx::query_scalar::<_, i64>("SELECT 1::bigint")
            .fetch_one(&self.pool)
            .await?;
        self.bus.health().await
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn bus(&self) -> &RedisBus {
        &self.bus
    }

    pub async fn memory(&self) -> RuntimeResult<ConversationMemory> {
        self.memory
            .get_or_try_init(|| ConversationMemory::open(self.rig_pool.clone()))
            .await
            .cloned()
    }
}

mod attachments;
mod chat_charts;
mod events;
mod helpers;
mod queue;
mod runs;
mod sessions;
mod styles;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod trace_tests;
mod users;

pub(crate) use helpers::*;
pub use styles::{
    CreateStylePresetInput, StylePresetBundle, StylePresetRecord, UpdateStylePresetInput,
    builtin_style_presets, find_builtin_style_preset,
};
