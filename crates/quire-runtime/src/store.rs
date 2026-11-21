//! PostgreSQL persistence for sessions, messages, runs, events and tool calls.

use chrono::{DateTime, Utc};
use quire_protocol::short_id;
use quire_session_protocol::{
    ActiveRunStatus, AttachmentMeta, MessageRole, PendingQuestion, PendingQueueItem,
    PendingQueueSource, QuestionAnswer, QuestionItem, ResolvedQuestion, RunTraceSegment,
    RunTraceSubagent, RunTraceSummary, SendMessageRequest, SessionCommentRef, SessionDetail,
    SessionEvent, SessionEventKind, SessionMessage, SessionStatus, SessionSummary, ToolCallDetail,
    aggregate_run_traces,
};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use tracing::warn;

use crate::{
    bus::RedisBus,
    comment_refs::validate_user_payload,
    compaction::CompactionState,
    error::{RuntimeError, RuntimeResult},
};

pub struct CompactionWrite<'a> {
    pub session_id: &'a str,
    pub summary: &'a str,
    pub up_to_message_id: &'a str,
    pub marker_before_message_id: Option<&'a str>,
    pub pinned_system_prompt: &'a str,
    pub kb_version: u64,
    pub context_usage: &'a quire_session_protocol::ContextUsage,
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
    bus: RedisBus,
}

impl RuntimeStore {
    pub async fn connect(database_url: &str, redis_url: &str) -> RuntimeResult<Self> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(16)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(database_url)
            .await?;
        Ok(Self {
            pool,
            bus: RedisBus::connect(redis_url)?,
        })
    }

    pub fn from_pool(pool: PgPool, bus: RedisBus) -> Self {
        Self { pool, bus }
    }

    pub async fn migrate(&self) -> RuntimeResult<()> {
        let mut migrator = sqlx::migrate!("./migrations");
        migrator.dangerous_set_table_name("_quire_runtime_migrations");
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

    pub async fn create_session(
        &self,
        title: &str,
        doc_id: Option<&str>,
    ) -> RuntimeResult<SessionSummary> {
        let branch: String = sqlx::query_scalar(
            "SELECT name FROM kb_branches WHERE status = 'active' ORDER BY name ASC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| RuntimeError::Internal("no active knowledge-base branch".into()))?;
        self.create_session_on_branch(title, doc_id, &branch).await
    }

    pub async fn create_session_on_branch(
        &self,
        title: &str,
        doc_id: Option<&str>,
        kb_branch: &str,
    ) -> RuntimeResult<SessionSummary> {
        let title = title.trim();
        if title.is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "session title must not be empty".into(),
            ));
        }
        let session_id = short_id("sess");
        sqlx::query(
            r#"INSERT INTO agent_sessions
               (session_id, title, doc_id, linked_doc_id, kb_branch, status, active_run_status)
               VALUES ($1, $2, $3, $3, $4, 'active', 'idle')"#,
        )
        .bind(&session_id)
        .bind(title)
        .bind(doc_id)
        .bind(kb_branch)
        .execute(&self.pool)
        .await?;
        self.get_session_summary(&session_id).await
    }

    pub async fn get_session_summary(&self, session_id: &str) -> RuntimeResult<SessionSummary> {
        let row = sqlx::query(
            r#"SELECT session_id, title, doc_id, linked_doc_id, kb_branch, status,
                      active_run_status, last_event_seq, context_usage, compaction_started_at,
                      compacted_at, pinned_at, created_at, updated_at
               FROM agent_sessions WHERE session_id = $1"#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| RuntimeError::SessionNotFound(session_id.to_string()))?;
        row_to_summary(&row)
    }

    pub async fn get_session_detail(&self, session_id: &str) -> RuntimeResult<SessionDetail> {
        let summary = self.get_session_summary(session_id).await?;
        let active_run = self.latest_active_run(session_id).await?;
        let (active_run_trace, active_subagent_traces) = match active_run.as_ref() {
            Some(run) => {
                let events = match run.start_seq {
                    Some(start_seq) => {
                        self.events_between(session_id, start_seq, summary.last_event_seq)
                            .await?
                    }
                    // Legacy active runs without a recorded start_seq retain the
                    // safe whole-history fallback; new runs never take this path.
                    None => {
                        self.events_through(session_id, summary.last_event_seq)
                            .await?
                    }
                };
                active_trace_snapshot(
                    &summary,
                    Some(run.run_id.as_str()),
                    aggregate_run_traces(&events),
                )
            }
            // Idle sessions do not need to read or aggregate any event history.
            None => (None, Vec::new()),
        };
        let messages = self.list_messages(session_id).await?;
        let attachments = self.list_attachments(session_id).await?;
        let pending_question = self.pending_question(session_id).await?;
        let resolved_questions = self.list_resolved_questions(session_id).await?;
        let pending_queue = self.list_pending_queue(session_id).await?;
        Ok(SessionDetail {
            summary,
            messages,
            attachments,
            pending_question,
            resolved_questions,
            pending_queue,
            active_run_trace,
            active_subagent_traces,
        })
    }

    async fn events_through(
        &self,
        session_id: &str,
        max_seq: u64,
    ) -> RuntimeResult<Vec<SessionEvent>> {
        let frames = self.event_frames_between(session_id, 0, max_seq).await?;
        Ok(decode_projection_events(frames))
    }

    /// Read one active run's events through the `(session_id, seq)` primary key.
    async fn events_between(
        &self,
        session_id: &str,
        start_seq: u64,
        end_seq: u64,
    ) -> RuntimeResult<Vec<SessionEvent>> {
        let frames = self
            .event_frames_between(session_id, start_seq, end_seq)
            .await?;
        Ok(decode_projection_events(frames))
    }

    async fn event_frames_between(
        &self,
        session_id: &str,
        start_seq: u64,
        end_seq: u64,
    ) -> RuntimeResult<Vec<SessionEventFrame>> {
        let rows = sqlx::query(
            r#"SELECT seq, event_type, event FROM session_events
               WHERE session_id = $1 AND seq >= $2 AND seq <= $3
               ORDER BY seq ASC"#,
        )
        .bind(session_id)
        .bind(i64::try_from(start_seq).unwrap_or(i64::MAX))
        .bind(i64::try_from(end_seq).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_event_frame).collect()
    }

    async fn event_frames_between_in(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        session_id: &str,
        start_seq: u64,
        end_seq: u64,
    ) -> RuntimeResult<Vec<SessionEventFrame>> {
        let rows = sqlx::query(
            r#"SELECT seq, event_type, event FROM session_events
               WHERE session_id = $1 AND seq >= $2 AND seq <= $3
               ORDER BY seq ASC"#,
        )
        .bind(session_id)
        .bind(i64::try_from(start_seq).unwrap_or(i64::MAX))
        .bind(i64::try_from(end_seq).unwrap_or(i64::MAX))
        .fetch_all(&mut **tx)
        .await?;
        rows.iter().map(row_to_event_frame).collect()
    }

    /// Aggregate and store one run's trace inside a lifecycle transaction.
    async fn materialize_run_trace_in(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        session_id: &str,
        run_id: &str,
        start_seq: Option<u64>,
        end_seq: u64,
    ) -> RuntimeResult<()> {
        let frames = self
            .event_frames_between_in(tx, session_id, start_seq.unwrap_or(0), end_seq)
            .await?;
        let events = decode_projection_events(frames);
        let Some(trace) = aggregate_run_traces(&events)
            .into_iter()
            .find(|trace| trace.run_id == run_id)
        else {
            return Ok(());
        };
        let value = serde_json::to_value(&trace)?;
        sqlx::query("UPDATE agent_runs SET trace_summary = $2 WHERE run_id = $1")
            .bind(run_id)
            .bind(value)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    /// Lazy backfill for a completed run whose trace has not been materialized.
    pub async fn materialize_run_trace(&self, run_id: &str) -> RuntimeResult<()> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query("SELECT session_id, start_seq FROM agent_runs WHERE run_id = $1")
            .bind(run_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| RuntimeError::RunNotFound(run_id.to_string()))?;
        let session_id: String = row.try_get("session_id")?;
        let start_seq: Option<i64> = row.try_get("start_seq")?;
        let end_seq = lock_event_seq(&mut tx, &session_id).await?;
        self.materialize_run_trace_in(
            &mut tx,
            &session_id,
            run_id,
            start_seq.map(|seq| u64::try_from(seq).unwrap_or(0)),
            end_seq,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Materialize every completed run in a session before pruning raw events.
    pub async fn materialize_completed_run_traces(&self, session_id: &str) -> RuntimeResult<usize> {
        let mut tx = self.pool.begin().await?;
        let end_seq = lock_event_seq(&mut tx, session_id).await?;
        let rows = sqlx::query(
            r#"SELECT run_id, start_seq FROM agent_runs
               WHERE session_id = $1 AND parent_run_id IS NULL
                 AND status NOT IN ('running', 'awaiting_input') AND trace_summary IS NULL"#,
        )
        .bind(session_id)
        .fetch_all(&mut *tx)
        .await?;
        let mut materialized = 0usize;
        for row in &rows {
            let run_id: String = row.try_get("run_id")?;
            let start_seq: Option<i64> = row.try_get("start_seq")?;
            self.materialize_run_trace_in(
                &mut tx,
                session_id,
                &run_id,
                start_seq.map(|seq| u64::try_from(seq).unwrap_or(0)),
                end_seq,
            )
            .await?;
            materialized += 1;
        }
        tx.commit().await?;
        Ok(materialized)
    }

    /// Opaque frames for SSE transport. Unlike projection, transport does not
    /// need to understand the event variant and can relay newer writers safely.
    pub async fn list_event_frames(
        &self,
        session_id: &str,
        after_seq: u64,
        limit: i64,
    ) -> RuntimeResult<Vec<SessionEventFrame>> {
        let rows = sqlx::query(
            r#"SELECT seq, event_type, event FROM session_events
               WHERE session_id = $1 AND seq > $2
               ORDER BY seq ASC LIMIT $3"#,
        )
        .bind(session_id)
        .bind(i64::try_from(after_seq).unwrap_or(i64::MAX))
        .bind(limit.clamp(1, 10_000))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_event_frame).collect()
    }

    pub async fn list_sessions(
        &self,
        limit: i64,
        offset: i64,
    ) -> RuntimeResult<Vec<SessionSummary>> {
        let rows = sqlx::query(
            r#"SELECT session_id, title, doc_id, linked_doc_id, kb_branch, status,
                      active_run_status, last_event_seq, context_usage, compaction_started_at,
                      compacted_at, pinned_at, created_at, updated_at
               FROM agent_sessions
               ORDER BY (pinned_at IS NOT NULL) DESC,
                        pinned_at DESC NULLS LAST,
                        updated_at DESC,
                        session_id ASC
               LIMIT $1 OFFSET $2"#,
        )
        .bind(limit.clamp(1, 200))
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_summary).collect()
    }

    pub async fn update_session(
        &self,
        session_id: &str,
        patch: SessionPatch<'_>,
    ) -> RuntimeResult<SessionSummary> {
        let updated = sqlx::query(
            r#"UPDATE agent_sessions
               SET title = COALESCE($2, title),
                   status = COALESCE($3, status),
                   pinned_at = CASE
                       WHEN $4::boolean IS NULL THEN pinned_at
                       WHEN $4 THEN NOW()
                       ELSE NULL
                   END,
                   updated_at = CASE
                       WHEN $2 IS NULL AND $3 IS NULL THEN updated_at
                       ELSE NOW()
                   END
               WHERE session_id = $1"#,
        )
        .bind(session_id)
        .bind(patch.title)
        .bind(patch.status.map(SessionStatus::as_str))
        .bind(patch.pinned)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(RuntimeError::SessionNotFound(session_id.to_string()));
        }
        self.get_session_summary(session_id).await
    }

    pub async fn delete_session(&self, session_id: &str) -> RuntimeResult<()> {
        let deleted = sqlx::query("DELETE FROM agent_sessions WHERE session_id = $1")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        if deleted.rows_affected() == 0 {
            return Err(RuntimeError::SessionNotFound(session_id.to_string()));
        }
        Ok(())
    }

    pub async fn bind_doc(&self, session_id: &str, doc_id: &str) -> RuntimeResult<()> {
        let updated = sqlx::query(
            r#"UPDATE agent_sessions
               SET doc_id = $2, linked_doc_id = $2, updated_at = NOW()
               WHERE session_id = $1"#,
        )
        .bind(session_id)
        .bind(doc_id)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(RuntimeError::SessionNotFound(session_id.to_string()));
        }
        Ok(())
    }

    pub async fn set_pinned_prompt(
        &self,
        session_id: &str,
        prompt: &str,
        kb_version: u64,
    ) -> RuntimeResult<()> {
        let updated = sqlx::query(
            r#"UPDATE agent_sessions
               SET pinned_system_prompt = $2, synced_kb_version = $3, updated_at = NOW()
               WHERE session_id = $1"#,
        )
        .bind(session_id)
        .bind(prompt)
        .bind(i64::try_from(kb_version).unwrap_or(i64::MAX))
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(RuntimeError::SessionNotFound(session_id.to_string()));
        }
        Ok(())
    }

    pub async fn begin_compaction(&self, session_id: &str) -> RuntimeResult<DateTime<Utc>> {
        let started: Option<DateTime<Utc>> = sqlx::query_scalar(
            r#"UPDATE agent_sessions
               SET compaction_started_at = NOW(), updated_at = NOW()
               WHERE session_id = $1
                 AND active_run_status <> 'running'
                 AND (
                     compaction_started_at IS NULL
                     OR compaction_started_at < NOW() - INTERVAL '10 minutes'
                 )
               RETURNING compaction_started_at"#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(started) = started {
            return Ok(started);
        }
        let session = self.get_session_summary(session_id).await?;
        if session.active_run_status == ActiveRunStatus::Running {
            return Err(RuntimeError::CompactionConflict(
                "session has an active run; wait for it to finish before compacting".into(),
            ));
        }
        Err(RuntimeError::CompactionConflict(
            "session compaction is already in progress".into(),
        ))
    }

    pub async fn clear_compaction(&self, session_id: &str) -> RuntimeResult<()> {
        sqlx::query(
            "UPDATE agent_sessions SET compaction_started_at = NULL, updated_at = NOW() WHERE session_id = $1",
        )
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn clear_compaction_if_started(
        &self,
        session_id: &str,
        started_at: DateTime<Utc>,
    ) -> RuntimeResult<()> {
        sqlx::query(
            r#"UPDATE agent_sessions
               SET compaction_started_at = NULL, updated_at = NOW()
               WHERE session_id = $1 AND compaction_started_at = $2"#,
        )
        .bind(session_id)
        .bind(started_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn compaction_state(&self, session_id: &str) -> RuntimeResult<CompactionState> {
        let row = sqlx::query(
            r#"SELECT compaction_summary, compacted_up_to_message_id, compacted_at
               FROM agent_sessions WHERE session_id = $1"#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| RuntimeError::SessionNotFound(session_id.to_string()))?;
        Ok(CompactionState {
            summary: row.try_get("compaction_summary")?,
            up_to_message_id: row.try_get("compacted_up_to_message_id")?,
            compacted_at: row.try_get("compacted_at")?,
        })
    }

    /// Atomically advance the compaction anchor, persist the marker message and
    /// switch the pinned prompt to the latest KB version. Queue items and
    /// questions are intentionally untouched.
    pub async fn apply_compaction(&self, write: &CompactionWrite<'_>) -> RuntimeResult<()> {
        let mut tx = self.pool.begin().await?;
        let status: Option<String> = sqlx::query_scalar(
            "SELECT active_run_status FROM agent_sessions WHERE session_id = $1 FOR UPDATE",
        )
        .bind(write.session_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(status) = status else {
            return Err(RuntimeError::SessionNotFound(write.session_id.to_string()));
        };
        if status == ActiveRunStatus::Running.as_str() {
            return Err(RuntimeError::CompactionConflict(
                "session has an active run; wait for it to finish before compacting".into(),
            ));
        }

        let usage = serde_json::to_value(write.context_usage)?;
        sqlx::query(
            r#"UPDATE agent_sessions
               SET compaction_summary = $2,
                   compacted_up_to_message_id = $3,
                   compacted_at = NOW(),
                   compaction_started_at = NULL,
                   pinned_system_prompt = $4,
                   synced_kb_version = $5,
                   context_usage = $6,
                   updated_at = NOW()
               WHERE session_id = $1"#,
        )
        .bind(write.session_id)
        .bind(write.summary)
        .bind(write.up_to_message_id)
        .bind(write.pinned_system_prompt)
        .bind(i64::try_from(write.kb_version).unwrap_or(i64::MAX))
        .bind(usage)
        .execute(&mut *tx)
        .await?;

        let message_id = short_id("msg");
        let client_msg_id = format!("compaction:{message_id}");
        sqlx::query(
            r#"INSERT INTO agent_messages
               (message_id, session_id, client_msg_id, role, content, comment_refs, attachment_ids, run_id, created_at)
               VALUES (
                   $1, $2, $3, 'compaction', $4, '[]'::jsonb, '[]'::jsonb, NULL,
                   COALESCE(
                       (SELECT created_at - INTERVAL '1 microsecond'
                        FROM agent_messages WHERE message_id = $5),
                       NOW()
                   )
               )"#,
        )
        .bind(&message_id)
        .bind(write.session_id)
        .bind(client_msg_id)
        .bind(write.summary)
        .bind(write.marker_before_message_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn pinned_prompt(&self, session_id: &str) -> RuntimeResult<(Option<String>, u64)> {
        let row = sqlx::query(
            "SELECT pinned_system_prompt, synced_kb_version FROM agent_sessions WHERE session_id = $1",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| RuntimeError::SessionNotFound(session_id.to_string()))?;
        let prompt: Option<String> = row.try_get("pinned_system_prompt")?;
        let version: i64 = row.try_get("synced_kb_version")?;
        Ok((prompt, u64::try_from(version).unwrap_or(0)))
    }

    /// Advance the version reflected by the frozen prompt without rewriting it.
    pub async fn advance_synced_kb_version(
        &self,
        session_id: &str,
        kb_version: u64,
    ) -> RuntimeResult<()> {
        let updated = sqlx::query(
            "UPDATE agent_sessions SET synced_kb_version = $2, updated_at = NOW() WHERE session_id = $1",
        )
        .bind(session_id)
        .bind(i64::try_from(kb_version).unwrap_or(i64::MAX))
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(RuntimeError::SessionNotFound(session_id.to_string()));
        }
        Ok(())
    }

    pub async fn insert_user_message(
        &self,
        session_id: &str,
        request: &SendMessageRequest,
    ) -> RuntimeResult<(SessionMessage, bool)> {
        self.insert_user_message_with_run(session_id, request, None)
            .await
    }

    pub async fn insert_injected_user_message(
        &self,
        session_id: &str,
        run_id: &str,
        request: &SendMessageRequest,
    ) -> RuntimeResult<(SessionMessage, bool)> {
        self.insert_user_message_with_run(session_id, request, Some(run_id))
            .await
    }

    async fn insert_user_message_with_run(
        &self,
        session_id: &str,
        request: &SendMessageRequest,
        run_id: Option<&str>,
    ) -> RuntimeResult<(SessionMessage, bool)> {
        let mut tx = self.pool.begin().await?;
        let message = self
            .insert_user_message_with_run_in(&mut tx, session_id, request, run_id)
            .await?;
        tx.commit().await?;
        Ok(message)
    }

    async fn insert_user_message_with_run_in(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        session_id: &str,
        request: &SendMessageRequest,
        run_id: Option<&str>,
    ) -> RuntimeResult<(SessionMessage, bool)> {
        if request.client_msg_id.trim().is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "client_msg_id must not be empty".into(),
            ));
        }
        validate_user_payload(
            &request.content,
            &request.comment_refs,
            &request.attachment_ids,
        )?;
        let message_id = short_id("msg");
        let attachment_ids = serde_json::to_value(&request.attachment_ids)?;
        let comment_refs = serde_json::to_value(&request.comment_refs)?;
        let inserted = sqlx::query(
            r#"INSERT INTO agent_messages
               (message_id, session_id, client_msg_id, role, content, comment_refs, attachment_ids, run_id)
               VALUES ($1, $2, $3, 'user', $4, $5, $6, $7)
               ON CONFLICT (session_id, client_msg_id) DO NOTHING
               RETURNING message_id, session_id, role, content, comment_refs, attachment_ids, run_id, created_at"#,
        )
        .bind(&message_id)
        .bind(session_id)
        .bind(&request.client_msg_id)
        .bind(&request.content)
        .bind(comment_refs)
        .bind(attachment_ids)
        .bind(run_id)
        .fetch_optional(&mut **tx)
        .await?;
        if let Some(row) = inserted {
            return Ok((row_to_message(&row)?, true));
        }
        let row = sqlx::query(
            r#"SELECT message_id, session_id, role, content, comment_refs, attachment_ids, run_id, created_at
               FROM agent_messages WHERE session_id = $1 AND client_msg_id = $2"#,
        )
        .bind(session_id)
        .bind(&request.client_msg_id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| RuntimeError::Internal("idempotent message vanished".into()))?;
        Ok((row_to_message(&row)?, false))
    }

    pub async fn find_message_by_client_id(
        &self,
        session_id: &str,
        client_msg_id: &str,
    ) -> RuntimeResult<Option<SessionMessage>> {
        let row = sqlx::query(
            r#"SELECT message_id, session_id, role, content, comment_refs, attachment_ids, run_id, created_at
               FROM agent_messages WHERE session_id = $1 AND client_msg_id = $2"#,
        )
        .bind(session_id)
        .bind(client_msg_id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(row_to_message).transpose()
    }

    pub async fn insert_assistant_message(
        &self,
        session_id: &str,
        content: &str,
        run_id: &str,
    ) -> RuntimeResult<SessionMessage> {
        let message_id = short_id("msg");
        let client_msg_id = format!("assistant:{message_id}");
        sqlx::query(
            r#"INSERT INTO agent_messages
               (message_id, session_id, client_msg_id, role, content, run_id)
               VALUES ($1, $2, $3, 'assistant', $4, $5)"#,
        )
        .bind(&message_id)
        .bind(session_id)
        .bind(client_msg_id)
        .bind(content)
        .bind(run_id)
        .execute(&self.pool)
        .await?;
        let row = sqlx::query(
            r#"SELECT message_id, session_id, role, content, comment_refs, attachment_ids, run_id, created_at
               FROM agent_messages WHERE message_id = $1"#,
        )
        .bind(&message_id)
        .fetch_one(&self.pool)
        .await?;
        row_to_message(&row)
    }

    pub async fn list_messages(&self, session_id: &str) -> RuntimeResult<Vec<SessionMessage>> {
        let rows = sqlx::query(
            r#"SELECT message_id, session_id, role, content, comment_refs, attachment_ids, run_id, created_at
               FROM agent_messages WHERE session_id = $1
               ORDER BY created_at ASC, message_id ASC"#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_message).collect()
    }

    pub async fn list_sessions_with_pending_queue(&self) -> RuntimeResult<Vec<String>> {
        let sessions = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT session_id FROM session_pending_queue ORDER BY session_id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(sessions)
    }

    pub async fn list_pending_queue(
        &self,
        session_id: &str,
    ) -> RuntimeResult<Vec<PendingQueueItem>> {
        self.get_session_summary(session_id).await?;
        let rows = sqlx::query(
            r#"SELECT queue_id, session_id, content, comment_refs, source, thread_id, attachment_ids, created_at
               FROM session_pending_queue
               WHERE session_id = $1
               ORDER BY sequence ASC"#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_pending_queue_item).collect()
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn enqueue_pending_message(
        &self,
        session_id: &str,
        client_msg_id: &str,
        content: &str,
        comment_refs: &[SessionCommentRef],
        source: PendingQueueSource,
        thread_id: Option<&str>,
        attachment_ids: &[String],
    ) -> RuntimeResult<PendingQueueItem> {
        if client_msg_id.trim().is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "pending queue client_msg_id must not be empty".into(),
            ));
        }
        validate_user_payload(content, comment_refs, attachment_ids)?;
        self.get_session_summary(session_id).await?;
        let queue_id = short_id("q");
        let attachments = serde_json::to_value(attachment_ids)?;
        let comment_refs = serde_json::to_value(comment_refs)?;
        let inserted = sqlx::query(
            r#"INSERT INTO session_pending_queue
               (queue_id, session_id, client_msg_id, content, comment_refs, source, thread_id, attachment_ids)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
               ON CONFLICT (session_id, client_msg_id) DO NOTHING
               RETURNING queue_id, session_id, content, comment_refs, source, thread_id, attachment_ids, created_at"#,
        )
        .bind(&queue_id)
        .bind(session_id)
        .bind(client_msg_id)
        .bind(content)
        .bind(comment_refs)
        .bind(source.as_str())
        .bind(thread_id)
        .bind(attachments)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = inserted {
            return row_to_pending_queue_item(&row);
        }
        let row = sqlx::query(
            r#"SELECT queue_id, session_id, content, comment_refs, source, thread_id, attachment_ids, created_at
               FROM session_pending_queue
               WHERE session_id = $1 AND client_msg_id = $2"#,
        )
        .bind(session_id)
        .bind(client_msg_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| RuntimeError::Internal("idempotent pending queue item vanished".into()))?;
        row_to_pending_queue_item(&row)
    }

    pub async fn update_pending_message(
        &self,
        session_id: &str,
        queue_id: &str,
        content: &str,
        comment_refs: &[SessionCommentRef],
    ) -> RuntimeResult<PendingQueueItem> {
        let existing = self.get_pending_queue_item(session_id, queue_id).await?;
        let attachment_ids = existing
            .as_ref()
            .map(|item| item.attachment_ids.as_slice())
            .unwrap_or(&[]);
        validate_user_payload(content, comment_refs, attachment_ids)?;
        if let Some(item) = existing
            && item.source == PendingQueueSource::Comment
            && (comment_refs.len() != 1
                || comment_refs
                    .first()
                    .map(|comment_ref| comment_ref.thread_id.as_str())
                    != item.thread_id.as_deref())
        {
            return Err(RuntimeError::InvalidRequest(
                "a comment queue item must keep exactly one matching comment reference".into(),
            ));
        }
        let comment_refs = serde_json::to_value(comment_refs)?;
        let row = sqlx::query(
            r#"UPDATE session_pending_queue
               SET content = $3, comment_refs = $4
               WHERE session_id = $1 AND queue_id = $2
               RETURNING queue_id, session_id, content, comment_refs, source, thread_id, attachment_ids, created_at"#,
        )
        .bind(session_id)
        .bind(queue_id)
        .bind(content)
        .bind(comment_refs)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| {
            RuntimeError::InvalidRequest(format!(
                "pending queue item `{queue_id}` was not found for session `{session_id}`"
            ))
        })?;
        row_to_pending_queue_item(&row)
    }

    pub async fn get_pending_queue_item(
        &self,
        session_id: &str,
        queue_id: &str,
    ) -> RuntimeResult<Option<PendingQueueItem>> {
        let row = sqlx::query(
            r#"SELECT queue_id, session_id, content, comment_refs, source, thread_id, attachment_ids, created_at
               FROM session_pending_queue
               WHERE session_id = $1 AND queue_id = $2"#,
        )
        .bind(session_id)
        .bind(queue_id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(row_to_pending_queue_item).transpose()
    }

    pub async fn pending_comment_item_for_thread(
        &self,
        session_id: &str,
        thread_id: &str,
    ) -> RuntimeResult<Option<PendingQueueItem>> {
        let row = sqlx::query(
            r#"SELECT queue_id, session_id, content, comment_refs, source, thread_id, attachment_ids, created_at
               FROM session_pending_queue
               WHERE session_id = $1 AND thread_id = $2 AND source = 'comment'
               ORDER BY sequence DESC
               LIMIT 1"#,
        )
        .bind(session_id)
        .bind(thread_id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(row_to_pending_queue_item).transpose()
    }

    pub async fn delete_pending_message(
        &self,
        session_id: &str,
        queue_id: &str,
    ) -> RuntimeResult<bool> {
        let deleted = sqlx::query_scalar::<_, String>(
            r#"DELETE FROM session_pending_queue
               WHERE session_id = $1 AND queue_id = $2
               RETURNING queue_id"#,
        )
        .bind(session_id)
        .bind(queue_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(deleted.is_some())
    }

    pub async fn remove_pending_items(
        &self,
        session_id: &str,
        queue_ids: &[String],
    ) -> RuntimeResult<u64> {
        if queue_ids.is_empty() {
            return Ok(0);
        }
        let deleted = sqlx::query(
            r#"DELETE FROM session_pending_queue
               WHERE session_id = $1 AND queue_id = ANY($2)"#,
        )
        .bind(session_id)
        .bind(queue_ids)
        .execute(&self.pool)
        .await?;
        Ok(deleted.rows_affected())
    }

    async fn list_pending_queue_in(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        session_id: &str,
    ) -> RuntimeResult<Vec<PendingQueueItem>> {
        let rows = sqlx::query(
            r#"SELECT queue_id, session_id, content, comment_refs, source, thread_id, attachment_ids, created_at
               FROM session_pending_queue
               WHERE session_id = $1
               ORDER BY sequence ASC"#,
        )
        .bind(session_id)
        .fetch_all(&mut **tx)
        .await?;
        rows.iter().map(row_to_pending_queue_item).collect()
    }

    async fn remove_pending_items_in(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        session_id: &str,
        queue_ids: &[String],
    ) -> RuntimeResult<u64> {
        if queue_ids.is_empty() {
            return Ok(0);
        }
        let deleted = sqlx::query(
            r#"DELETE FROM session_pending_queue
               WHERE session_id = $1 AND queue_id = ANY($2)"#,
        )
        .bind(session_id)
        .bind(queue_ids)
        .execute(&mut **tx)
        .await?;
        Ok(deleted.rows_affected())
    }

    /// Atomically persist a claimed queue batch, remove its items and emit the
    /// resulting queue/message events. A crash can no longer leave a persisted
    /// user message beside an unclaimed queue item.
    pub async fn claim_pending_batch(
        &self,
        session_id: &str,
        run_id: &str,
        request: &SendMessageRequest,
        queue_ids: &[String],
        inject_into_run: bool,
    ) -> RuntimeResult<Option<SessionMessage>> {
        if queue_ids.is_empty() {
            return Ok(None);
        }
        let mut tx = self.pool.begin().await?;
        let _status: String = sqlx::query_scalar(
            "SELECT active_run_status FROM agent_sessions WHERE session_id = $1 FOR UPDATE",
        )
        .bind(session_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| RuntimeError::SessionNotFound(session_id.to_string()))?;
        // Another runtime may have claimed the same batch before this lock was
        // acquired. Re-check ownership instead of emitting a duplicate message.
        let claimable: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM session_pending_queue WHERE session_id = $1 AND queue_id = ANY($2)",
        )
        .bind(session_id)
        .bind(queue_ids)
        .fetch_one(&mut *tx)
        .await?;
        if claimable != i64::try_from(queue_ids.len()).unwrap_or(i64::MAX) {
            tx.commit().await?;
            return Ok(None);
        }
        let run_id = inject_into_run.then_some(run_id);
        let (message, _inserted) = self
            .insert_user_message_with_run_in(&mut tx, session_id, request, run_id)
            .await?;
        self.remove_pending_items_in(&mut tx, session_id, queue_ids)
            .await?;
        let remaining = self.list_pending_queue_in(&mut tx, session_id).await?;
        let queue_event = self
            .append_event_in(
                &mut tx,
                session_id,
                "queue",
                "system",
                None,
                SessionEventKind::PendingQueueUpdated { items: remaining },
            )
            .await?;
        let added_event = if inject_into_run {
            None
        } else {
            Some(
                self.append_event_in(
                    &mut tx,
                    session_id,
                    run_id.unwrap_or("queue"),
                    "queue",
                    None,
                    SessionEventKind::UserMessageAdded {
                        message_id: message.message_id.clone(),
                    },
                )
                .await?,
            )
        };
        tx.commit().await?;
        self.publish_event(queue_event, session_id).await;
        if let Some(event) = added_event {
            self.publish_event(event, session_id).await;
        }
        Ok(Some(message))
    }

    pub async fn remove_pending_comment_thread(
        &self,
        session_id: &str,
        thread_id: &str,
    ) -> RuntimeResult<u64> {
        let deleted = sqlx::query(
            r#"DELETE FROM session_pending_queue
               WHERE session_id = $1 AND thread_id = $2"#,
        )
        .bind(session_id)
        .bind(thread_id)
        .execute(&self.pool)
        .await?;
        Ok(deleted.rows_affected())
    }

    pub async fn insert_context_message(
        &self,
        session_id: &str,
        role: MessageRole,
        content: &str,
        client_msg_id: &str,
        run_id: Option<&str>,
    ) -> RuntimeResult<SessionMessage> {
        let message_id = short_id("msg");
        let inserted = sqlx::query(
            r#"INSERT INTO agent_messages
               (message_id, session_id, client_msg_id, role, content, attachment_ids, run_id)
               VALUES ($1, $2, $3, $4, $5, '[]'::jsonb, $6)
               ON CONFLICT (session_id, client_msg_id) DO NOTHING
               RETURNING message_id, session_id, role, content, comment_refs, attachment_ids, run_id, created_at"#,
        )
        .bind(&message_id)
        .bind(session_id)
        .bind(client_msg_id)
        .bind(role.as_str())
        .bind(content)
        .bind(run_id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = inserted {
            return row_to_message(&row);
        }
        let row = sqlx::query(
            r#"SELECT message_id, session_id, role, content, comment_refs, attachment_ids, run_id, created_at
               FROM agent_messages WHERE session_id = $1 AND client_msg_id = $2"#,
        )
        .bind(session_id)
        .bind(client_msg_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| RuntimeError::Internal("question context message vanished".into()))?;
        row_to_message(&row)
    }

    pub async fn create_question(
        &self,
        session_id: &str,
        run_id: &str,
        tool_call_id: Option<&str>,
        questions: &[QuestionItem],
    ) -> RuntimeResult<PendingQuestion> {
        let mut tx = self.pool.begin().await?;
        let question = self
            .create_question_in(&mut tx, session_id, run_id, tool_call_id, questions)
            .await?;
        tx.commit().await?;
        Ok(question)
    }

    async fn create_question_in(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        session_id: &str,
        run_id: &str,
        tool_call_id: Option<&str>,
        questions: &[QuestionItem],
    ) -> RuntimeResult<PendingQuestion> {
        let question_id = short_id("q");
        let questions_value = serde_json::to_value(questions)?;
        let created_at: DateTime<Utc> = sqlx::query_scalar(
            r#"INSERT INTO agent_questions
               (question_id, session_id, run_id, tool_call_id, questions, status)
               VALUES ($1, $2, $3, $4, $5, 'pending')
               RETURNING created_at"#,
        )
        .bind(&question_id)
        .bind(session_id)
        .bind(run_id)
        .bind(tool_call_id)
        .bind(questions_value)
        .fetch_one(&mut **tx)
        .await?;
        Ok(PendingQuestion {
            question_id,
            run_id: run_id.to_string(),
            tool_call_id: tool_call_id.map(str::to_string),
            questions: questions.to_vec(),
            created_at,
        })
    }

    pub async fn pending_question(
        &self,
        session_id: &str,
    ) -> RuntimeResult<Option<PendingQuestion>> {
        let row = sqlx::query(
            r#"SELECT question_id, run_id, tool_call_id, questions, created_at
               FROM agent_questions
               WHERE session_id = $1 AND status = 'pending'
               ORDER BY created_at DESC LIMIT 1"#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(row_to_pending_question).transpose()
    }

    pub async fn pending_question_by_id(
        &self,
        session_id: &str,
        question_id: &str,
    ) -> RuntimeResult<Option<PendingQuestion>> {
        let row = sqlx::query(
            r#"SELECT question_id, run_id, tool_call_id, questions, created_at
               FROM agent_questions
               WHERE session_id = $1 AND question_id = $2 AND status = 'pending'"#,
        )
        .bind(session_id)
        .bind(question_id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(row_to_pending_question).transpose()
    }

    pub async fn list_resolved_questions(
        &self,
        session_id: &str,
    ) -> RuntimeResult<Vec<ResolvedQuestion>> {
        let rows = sqlx::query(
            r#"SELECT question_id, run_id, tool_call_id, questions, status, answers, created_at
               FROM agent_questions
               WHERE session_id = $1 AND status <> 'pending'
               ORDER BY created_at ASC"#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_resolved_question).collect()
    }

    pub async fn resolve_question(
        &self,
        session_id: &str,
        question_id: &str,
        answers: &[QuestionAnswer],
        skipped: bool,
    ) -> RuntimeResult<()> {
        let status = if skipped { "skipped" } else { "answered" };
        let answers = serde_json::to_value(answers)?;
        let updated = sqlx::query(
            r#"UPDATE agent_questions
               SET answers = $3, status = $4, resolved_at = NOW()
               WHERE session_id = $1 AND question_id = $2 AND status = 'pending'"#,
        )
        .bind(session_id)
        .bind(question_id)
        .bind(answers)
        .bind(status)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(RuntimeError::InvalidRequest(format!(
                "question `{question_id}` is no longer pending"
            )));
        }
        Ok(())
    }

    pub async fn dismiss_pending_questions(
        &self,
        session_id: &str,
    ) -> RuntimeResult<Vec<PendingQuestion>> {
        let rows = sqlx::query(
            r#"UPDATE agent_questions
               SET status = 'dismissed', resolved_at = NOW()
               WHERE session_id = $1 AND status = 'pending'
               RETURNING question_id, run_id, tool_call_id, questions, created_at"#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_pending_question).collect()
    }

    pub async fn mark_run_awaiting_input(&self, run_id: &str) -> RuntimeResult<()> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            r#"UPDATE agent_runs
               SET status = 'awaiting_input'
               WHERE run_id = $1 AND status = 'running'
               RETURNING session_id"#,
        )
        .bind(run_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            let exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM agent_runs WHERE run_id = $1)",
            )
            .bind(run_id)
            .fetch_one(&mut *tx)
            .await?;
            tx.commit().await?;
            return if exists {
                Ok(())
            } else {
                Err(RuntimeError::RunNotFound(run_id.to_string()))
            };
        };
        let session_id: String = row.try_get("session_id")?;
        sqlx::query(
            r#"UPDATE agent_sessions
               SET active_run_status = 'awaiting_input', updated_at = NOW()
               WHERE session_id = $1"#,
        )
        .bind(&session_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn resolve_suspended_run(&self, run_id: &str) -> RuntimeResult<Option<String>> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            r#"UPDATE agent_runs
               SET status = 'completed', finished_at = COALESCE(finished_at, NOW())
               WHERE run_id = $1 AND status = 'awaiting_input'
               RETURNING session_id, start_seq"#,
        )
        .bind(run_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        let session_id: String = row.try_get("session_id")?;
        let start_seq: Option<i64> = row.try_get("start_seq")?;
        sqlx::query(
            r#"UPDATE agent_sessions SET active_run_status = 'idle', updated_at = NOW()
               WHERE session_id = $1"#,
        )
        .bind(&session_id)
        .execute(&mut *tx)
        .await?;
        let end_seq = lock_event_seq(&mut tx, &session_id).await?;
        self.materialize_run_trace_in(
            &mut tx,
            &session_id,
            run_id,
            start_seq.map(|seq| u64::try_from(seq).unwrap_or(0)),
            end_seq,
        )
        .await?;
        tx.commit().await?;
        Ok(Some(session_id))
    }

    pub async fn insert_attachment(
        &self,
        attachment: AttachmentInsert<'_>,
    ) -> RuntimeResult<AttachmentMeta> {
        let attachment_id = attachment
            .attachment_id
            .map(str::to_string)
            .unwrap_or_else(|| short_id("att"));
        sqlx::query(
            r#"INSERT INTO agent_attachments
               (attachment_id, session_id, filename, mime_type, size_bytes, storage_path,
                storage_provider)
               VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
        )
        .bind(&attachment_id)
        .bind(attachment.session_id)
        .bind(attachment.filename)
        .bind(attachment.mime_type)
        .bind(i64::try_from(attachment.size_bytes).unwrap_or(i64::MAX))
        .bind(attachment.storage_path)
        .bind(attachment.storage_provider)
        .execute(&self.pool)
        .await?;
        Ok(AttachmentMeta {
            attachment_id,
            filename: attachment.filename.to_string(),
            mime_type: attachment.mime_type.to_string(),
            size_bytes: attachment.size_bytes,
        })
    }

    /// Load an attachment only when it belongs to the requested session.
    pub async fn get_session_attachment(
        &self,
        session_id: &str,
        attachment_id: &str,
    ) -> RuntimeResult<StoredAttachment> {
        let row = sqlx::query(
            r#"SELECT attachment_id, session_id, filename, mime_type, size_bytes,
                      storage_path, storage_provider
               FROM agent_attachments
               WHERE attachment_id = $1 AND session_id = $2"#,
        )
        .bind(attachment_id)
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| RuntimeError::AttachmentNotFound(attachment_id.to_string()))?;
        let size_bytes: i64 = row.try_get("size_bytes")?;
        Ok(StoredAttachment {
            attachment_id: row.try_get("attachment_id")?,
            session_id: row.try_get("session_id")?,
            filename: row.try_get("filename")?,
            mime_type: row.try_get("mime_type")?,
            size_bytes: u64::try_from(size_bytes).unwrap_or(0),
            storage_path: row.try_get("storage_path")?,
            storage_provider: row.try_get("storage_provider")?,
        })
    }

    pub async fn list_attachments(&self, session_id: &str) -> RuntimeResult<Vec<AttachmentMeta>> {
        let rows = sqlx::query(
            r#"SELECT attachment_id, filename, mime_type, size_bytes
               FROM agent_attachments WHERE session_id = $1 ORDER BY created_at ASC"#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                let size_bytes: i64 = row.try_get("size_bytes")?;
                Ok(AttachmentMeta {
                    attachment_id: row.try_get("attachment_id")?,
                    filename: row.try_get("filename")?,
                    mime_type: row.try_get("mime_type")?,
                    size_bytes: u64::try_from(size_bytes).unwrap_or(0),
                })
            })
            .collect()
    }

    pub async fn create_run(
        &self,
        session_id: &str,
        parent_run_id: Option<&str>,
        agent_id: &str,
        model: &str,
    ) -> RuntimeResult<RunRecord> {
        let mut tx = self.pool.begin().await?;
        if parent_run_id.is_none() {
            let status: Option<String> = sqlx::query_scalar(
                "SELECT active_run_status FROM agent_sessions WHERE session_id = $1 FOR UPDATE",
            )
            .bind(session_id)
            .fetch_optional(&mut *tx)
            .await?;
            let Some(status) = status else {
                return Err(RuntimeError::SessionNotFound(session_id.to_string()));
            };
            if matches!(status.as_str(), "running" | "awaiting_input") {
                return Err(RuntimeError::InvalidRequest(format!(
                    "session `{session_id}` already has an active run; queue the message instead"
                )));
            }
        }
        let run_id = short_id("run");
        sqlx::query(
            r#"INSERT INTO agent_runs (run_id, session_id, parent_run_id, agent_id, status, model)
               VALUES ($1, $2, $3, $4, 'running', $5)"#,
        )
        .bind(&run_id)
        .bind(session_id)
        .bind(parent_run_id)
        .bind(agent_id)
        .bind(model)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"UPDATE agent_sessions SET active_run_status = 'running', updated_at = NOW()
               WHERE session_id = $1"#,
        )
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.get_run(&run_id).await
    }

    /// Append `turn_started` and record its sequence as the run's hydration
    /// cursor in the same transaction.
    pub async fn start_run_with_event(
        &self,
        run_id: &str,
        kind: SessionEventKind,
    ) -> RuntimeResult<SessionEvent> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT session_id, agent_id, parent_run_id FROM agent_runs WHERE run_id = $1",
        )
        .bind(run_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| RuntimeError::RunNotFound(run_id.to_string()))?;
        let session_id: String = row.try_get("session_id")?;
        let agent_id: String = row.try_get("agent_id")?;
        let parent_run_id: Option<String> = row.try_get("parent_run_id")?;
        let event = self
            .append_event_in(
                &mut tx,
                &session_id,
                run_id,
                &agent_id,
                parent_run_id.as_deref(),
                kind,
            )
            .await?;
        sqlx::query("UPDATE agent_runs SET start_seq = $2 WHERE run_id = $1 AND start_seq IS NULL")
            .bind(run_id)
            .bind(i64::try_from(event.seq).unwrap_or(i64::MAX))
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.publish_event(event.clone(), &session_id).await;
        Ok(event)
    }

    pub async fn get_run(&self, run_id: &str) -> RuntimeResult<RunRecord> {
        let row = sqlx::query(
            r#"SELECT run_id, session_id, parent_run_id, agent_id, status, model, error,
                      started_at, finished_at, start_seq
               FROM agent_runs WHERE run_id = $1"#,
        )
        .bind(run_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| RuntimeError::RunNotFound(run_id.to_string()))?;
        row_to_run(&row)
    }

    pub async fn latest_running_run(&self, session_id: &str) -> RuntimeResult<Option<RunRecord>> {
        let row = sqlx::query(
            r#"SELECT run_id, session_id, parent_run_id, agent_id, status, model, error,
                      started_at, finished_at, start_seq
               FROM agent_runs
               WHERE session_id = $1 AND status = 'running'
               ORDER BY started_at DESC LIMIT 1"#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| row_to_run(&row)).transpose()
    }

    /// Latest run that still owns the live pane (`running` or `awaiting_input`).
    pub async fn latest_active_run(&self, session_id: &str) -> RuntimeResult<Option<RunRecord>> {
        let row = sqlx::query(
            r#"SELECT run_id, session_id, parent_run_id, agent_id, status, model, error,
                      started_at, finished_at, start_seq
               FROM agent_runs
               WHERE session_id = $1 AND status IN ('running', 'awaiting_input')
               ORDER BY started_at DESC LIMIT 1"#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| row_to_run(&row)).transpose()
    }

    pub async fn latest_run(&self, session_id: &str) -> RuntimeResult<Option<RunRecord>> {
        let row = sqlx::query(
            r#"SELECT run_id, session_id, parent_run_id, agent_id, status, model, error,
                      started_at, finished_at, start_seq
               FROM agent_runs
               WHERE session_id = $1
               ORDER BY started_at DESC LIMIT 1"#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| row_to_run(&row)).transpose()
    }

    /// Mark every surviving `running` run as interrupted after a runtime restart.
    pub async fn recover_running_runs(&self) -> RuntimeResult<usize> {
        let rows = sqlx::query(
            r#"SELECT run_id, session_id, parent_run_id, agent_id, status, model, error,
                      started_at, finished_at, start_seq
               FROM agent_runs WHERE status = 'running'"#,
        )
        .fetch_all(&self.pool)
        .await?;
        for row in &rows {
            let run = row_to_run(row)?;
            let _ = self
                .finish_run_with_event(
                    &run.run_id,
                    "interrupted",
                    Some("runtime restarted while run was active"),
                    SessionEventKind::TurnFailed {
                        error: "runtime restarted while run was active".into(),
                        recoverable: true,
                    },
                )
                .await?;
        }
        Ok(rows.len())
    }

    pub async fn update_context_usage(
        &self,
        session_id: &str,
        usage: &quire_session_protocol::ContextUsage,
    ) -> RuntimeResult<()> {
        sqlx::query(
            "UPDATE agent_sessions SET context_usage = $2, updated_at = NOW() WHERE session_id = $1",
        )
        .bind(session_id)
        .bind(serde_json::to_value(usage)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Append an event inside a caller-owned transaction. The session row lock
    /// allocates `seq`, so lifecycle status and event rows can commit atomically.
    pub async fn append_event_in(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        session_id: &str,
        run_id: &str,
        agent_id: &str,
        parent_run_id: Option<&str>,
        kind: SessionEventKind,
    ) -> RuntimeResult<SessionEvent> {
        let last_seq = lock_event_seq(tx, session_id).await?;
        let seq = last_seq.saturating_add(1);
        let event =
            insert_event(tx, session_id, seq, run_id, agent_id, parent_run_id, kind).await?;
        update_event_seq(tx, session_id, seq).await?;
        Ok(event)
    }

    pub async fn append_event(
        &self,
        session_id: &str,
        run_id: &str,
        agent_id: &str,
        parent_run_id: Option<&str>,
        kind: SessionEventKind,
    ) -> RuntimeResult<SessionEvent> {
        let mut tx = self.pool.begin().await?;
        let event = self
            .append_event_in(&mut tx, session_id, run_id, agent_id, parent_run_id, kind)
            .await?;
        tx.commit().await?;
        self.publish_event(event.clone(), session_id).await;
        Ok(event)
    }

    /// Append a coalesced batch with one sequence lock and one transaction.
    pub async fn append_event_batch(
        &self,
        session_id: &str,
        drafts: Vec<SessionEventDraft>,
    ) -> RuntimeResult<Vec<SessionEvent>> {
        if drafts.is_empty() {
            return Ok(Vec::new());
        }
        let mut tx = self.pool.begin().await?;
        let mut seq = lock_event_seq(&mut tx, session_id).await?;
        let mut events = Vec::with_capacity(drafts.len());
        for draft in drafts {
            seq = seq.saturating_add(1);
            let event = insert_event(
                &mut tx,
                session_id,
                seq,
                &draft.run_id,
                &draft.agent_id,
                draft.parent_run_id.as_deref(),
                draft.kind,
            )
            .await?;
            events.push(event);
        }
        update_event_seq(&mut tx, session_id, seq).await?;
        tx.commit().await?;
        for event in &events {
            self.publish_event(event.clone(), session_id).await;
        }
        Ok(events)
    }

    /// Persist a tool row and its wire event atomically.
    pub async fn save_tool_call_with_event(
        &self,
        detail: &ToolCallDetail,
        agent_id: &str,
        parent_run_id: Option<&str>,
        kind: SessionEventKind,
    ) -> RuntimeResult<SessionEvent> {
        let mut tx = self.pool.begin().await?;
        self.save_tool_call_in(&mut tx, detail).await?;
        let event = self
            .append_event_in(
                &mut tx,
                &detail.session_id,
                &detail.run_id,
                agent_id,
                parent_run_id,
                kind,
            )
            .await?;
        tx.commit().await?;
        self.publish_event(event.clone(), &detail.session_id).await;
        Ok(event)
    }

    /// Create an `ask_user` question and emit its request in one transaction.
    pub async fn create_question_with_event(
        &self,
        session_id: &str,
        run_id: &str,
        agent_id: &str,
        parent_run_id: Option<&str>,
        tool_call_id: Option<&str>,
        questions: &[QuestionItem],
    ) -> RuntimeResult<(PendingQuestion, SessionEvent)> {
        let mut tx = self.pool.begin().await?;
        let question = self
            .create_question_in(&mut tx, session_id, run_id, tool_call_id, questions)
            .await?;
        let kind = SessionEventKind::QuestionnaireRequested {
            question_id: question.question_id.clone(),
            tool_call_id: question.tool_call_id.clone(),
            questions: question.questions.clone(),
        };
        let event = self
            .append_event_in(&mut tx, session_id, run_id, agent_id, parent_run_id, kind)
            .await?;
        tx.commit().await?;
        self.publish_event(event.clone(), session_id).await;
        Ok((question, event))
    }

    /// Atomically transition a running run to a terminal state and append its
    /// terminal event. `None` means another writer already terminated the run,
    /// so no second terminal event may be emitted.
    pub async fn finish_run_with_event(
        &self,
        run_id: &str,
        status: &str,
        error: Option<&str>,
        kind: SessionEventKind,
    ) -> RuntimeResult<Option<SessionEvent>> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            r#"UPDATE agent_runs
               SET status = $2, error = $3, finished_at = NOW()
               WHERE run_id = $1 AND status = 'running'
               RETURNING session_id, agent_id, parent_run_id, start_seq"#,
        )
        .bind(run_id)
        .bind(status)
        .bind(error)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            let exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM agent_runs WHERE run_id = $1)",
            )
            .bind(run_id)
            .fetch_one(&mut *tx)
            .await?;
            tx.commit().await?;
            return if exists {
                Ok(None)
            } else {
                Err(RuntimeError::RunNotFound(run_id.to_string()))
            };
        };
        let session_id: String = row.try_get("session_id")?;
        let agent_id: String = row.try_get("agent_id")?;
        let parent_run_id: Option<String> = row.try_get("parent_run_id")?;
        let start_seq: Option<i64> = row.try_get("start_seq")?;
        let session_status = match status {
            "completed" => "idle",
            "cancelled" | "interrupted" => "interrupted",
            "failed" => "failed",
            _ => "idle",
        };
        let event = self
            .append_event_in(
                &mut tx,
                &session_id,
                run_id,
                &agent_id,
                parent_run_id.as_deref(),
                kind,
            )
            .await?;
        sqlx::query(
            "UPDATE agent_sessions SET active_run_status = $2, updated_at = NOW() WHERE session_id = $1",
        )
        .bind(&session_id)
        .bind(session_status)
        .execute(&mut *tx)
        .await?;
        self.materialize_run_trace_in(
            &mut tx,
            &session_id,
            run_id,
            start_seq.map(|seq| u64::try_from(seq).unwrap_or(0)),
            event.seq,
        )
        .await?;
        tx.commit().await?;
        self.publish_event(event.clone(), &session_id).await;
        Ok(Some(event))
    }

    async fn publish_event(&self, event: SessionEvent, session_id: &str) {
        let seq = event.seq;
        if let Err(error) = self.bus.publish_event(&event, session_id).await {
            warn!(%error, %session_id, seq, "failed to publish live session event");
        }
    }

    pub async fn list_events(
        &self,
        session_id: &str,
        after_seq: u64,
        limit: i64,
    ) -> RuntimeResult<Vec<SessionEvent>> {
        let rows = sqlx::query(
            r#"SELECT event FROM session_events
               WHERE session_id = $1 AND seq > $2
               ORDER BY seq ASC LIMIT $3"#,
        )
        .bind(session_id)
        .bind(i64::try_from(after_seq).unwrap_or(i64::MAX))
        .bind(limit.clamp(1, 10_000))
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                let value: Value = row.try_get("event")?;
                Ok(serde_json::from_value(value)?)
            })
            .collect()
    }

    /// Read materialized completed traces and aggregate only the active run.
    pub async fn list_run_traces(&self, session_id: &str) -> RuntimeResult<Vec<RunTraceSummary>> {
        let summary = self.get_session_summary(session_id).await?;
        let rows = sqlx::query(
            r#"SELECT run_id, status, start_seq, trace_summary FROM agent_runs
               WHERE session_id = $1 AND parent_run_id IS NULL
               ORDER BY started_at ASC, run_id ASC"#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        let mut traces = Vec::with_capacity(rows.len());
        for row in &rows {
            let run_id: String = row.try_get("run_id")?;
            let status: String = row.try_get("status")?;
            let start_seq: Option<i64> = row.try_get("start_seq")?;
            let stored: Option<Value> = row.try_get("trace_summary")?;
            if let Some(value) = stored {
                traces.push(serde_json::from_value(value)?);
                continue;
            }
            let events = self
                .events_between(
                    session_id,
                    start_seq
                        .map(|seq| u64::try_from(seq).unwrap_or(0))
                        .unwrap_or(0),
                    summary.last_event_seq,
                )
                .await?;
            let Some(trace) = aggregate_run_traces(&events)
                .into_iter()
                .find(|trace| trace.run_id == run_id)
            else {
                continue;
            };
            if !matches!(status.as_str(), "running" | "awaiting_input") {
                // Lazy backfill for runs completed before trace materialization.
                self.materialize_run_trace(&run_id).await?;
            }
            traces.push(trace);
        }

        // Some historical/test events have no agent_runs row. Keep their
        // run-scoped projection instead of silently dropping those traces.
        let orphan_rows = sqlx::query(
            r#"SELECT DISTINCT run_id FROM session_events
               WHERE session_id = $1 AND parent_run_id IS NULL
                 AND run_id NOT IN (SELECT run_id FROM agent_runs WHERE session_id = $1)
               ORDER BY run_id ASC"#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        for row in &orphan_rows {
            let run_id: String = row.try_get("run_id")?;
            let events = self
                .events_between(session_id, 0, summary.last_event_seq)
                .await?;
            if let Some(trace) = aggregate_run_traces(&events)
                .into_iter()
                .find(|trace| trace.run_id == run_id)
            {
                traces.push(trace);
            }
        }
        Ok(traces)
    }

    /// Oldest sequence still available for SSE replay.
    pub async fn min_available_seq(&self, session_id: &str) -> RuntimeResult<u64> {
        let value: i64 = sqlx::query_scalar(
            "SELECT min_available_seq FROM agent_sessions WHERE session_id = $1",
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(u64::try_from(value).unwrap_or(0))
    }

    /// Materialize completed traces, then prune old raw events while preserving
    /// the active run's complete history. Returns `(deleted, min_available_seq)`.
    pub async fn prune_session_events(
        &self,
        session_id: &str,
        keep_events: usize,
    ) -> RuntimeResult<(u64, u64)> {
        let keep = keep_events.clamp(1, 100_000);
        self.materialize_completed_run_traces(session_id).await?;

        let mut tx = self.pool.begin().await?;
        let last_seq = lock_event_seq(&mut tx, session_id).await?;
        let active_start: Option<Option<i64>> = sqlx::query_scalar(
            r#"SELECT start_seq FROM agent_runs
               WHERE session_id = $1 AND status IN ('running', 'awaiting_input')
               ORDER BY started_at DESC LIMIT 1"#,
        )
        .bind(session_id)
        .fetch_optional(&mut *tx)
        .await?;
        let rows = sqlx::query(
            r#"SELECT seq FROM session_events
               WHERE session_id = $1 ORDER BY seq DESC LIMIT $2"#,
        )
        .bind(session_id)
        .bind(i64::try_from(keep).unwrap_or(i64::MAX))
        .fetch_all(&mut *tx)
        .await?;
        let cutoff = if rows.len() == keep {
            rows.last()
                .map(|row| row.try_get::<i64, _>("seq"))
                .transpose()?
                .map(|seq| u64::try_from(seq).unwrap_or(0))
        } else {
            None
        };
        let mut min_available = cutoff.unwrap_or(last_seq);
        match active_start {
            Some(Some(start)) => {
                min_available = min_available.min(u64::try_from(start).unwrap_or(0));
            }
            // A run created before `start_run_with_event` has no start_seq yet.
            // Protect the high-water mark instead of treating it as no active run.
            Some(None) => {
                min_available = min_available.min(last_seq);
            }
            None => {}
        }

        let deleted = sqlx::query("DELETE FROM session_events WHERE session_id = $1 AND seq < $2")
            .bind(session_id)
            .bind(i64::try_from(min_available).unwrap_or(i64::MAX))
            .execute(&mut *tx)
            .await?
            .rows_affected();
        sqlx::query(
            "UPDATE agent_sessions SET min_available_seq = GREATEST(min_available_seq, $2) WHERE session_id = $1",
        )
        .bind(session_id)
        .bind(i64::try_from(min_available).unwrap_or(i64::MAX))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok((deleted, min_available))
    }

    pub async fn count_events_after(&self, session_id: &str, after_seq: u64) -> RuntimeResult<i64> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)::bigint FROM session_events WHERE session_id = $1 AND seq > $2",
        )
        .bind(session_id)
        .bind(i64::try_from(after_seq).unwrap_or(i64::MAX))
        .fetch_one(&self.pool)
        .await?;
        Ok(count)
    }

    pub async fn save_tool_call(&self, detail: &ToolCallDetail) -> RuntimeResult<()> {
        let mut tx = self.pool.begin().await?;
        self.save_tool_call_in(&mut tx, detail).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn save_tool_call_in(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        detail: &ToolCallDetail,
    ) -> RuntimeResult<()> {
        sqlx::query(
            r#"INSERT INTO agent_tool_calls
               (tool_call_id, session_id, run_id, tool_name, status, input, output, error,
                duration_ms, created_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
               ON CONFLICT (tool_call_id) DO UPDATE
               SET status = EXCLUDED.status,
                   output = EXCLUDED.output,
                   error = EXCLUDED.error,
                   duration_ms = EXCLUDED.duration_ms"#,
        )
        .bind(&detail.tool_call_id)
        .bind(&detail.session_id)
        .bind(&detail.run_id)
        .bind(&detail.tool_name)
        .bind(&detail.status)
        .bind(&detail.input)
        .bind(&detail.output)
        .bind(&detail.error)
        .bind(i64::try_from(detail.duration_ms).unwrap_or(i64::MAX))
        .bind(detail.created_at)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    pub async fn get_tool_call(
        &self,
        session_id: &str,
        tool_call_id: &str,
    ) -> RuntimeResult<ToolCallDetail> {
        let row = sqlx::query(
            r#"SELECT tool_call_id, session_id, run_id, tool_name, status, input, output, error,
                      duration_ms, created_at
               FROM agent_tool_calls WHERE session_id = $1 AND tool_call_id = $2"#,
        )
        .bind(session_id)
        .bind(tool_call_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| RuntimeError::ToolCallNotFound(tool_call_id.to_string()))?;
        let duration_ms: i64 = row.try_get("duration_ms")?;
        Ok(ToolCallDetail {
            tool_call_id: row.try_get("tool_call_id")?,
            session_id: row.try_get("session_id")?,
            run_id: row.try_get("run_id")?,
            tool_name: row.try_get("tool_name")?,
            status: row.try_get("status")?,
            input: row.try_get("input")?,
            output: row.try_get("output")?,
            error: row.try_get("error")?,
            duration_ms: u64::try_from(duration_ms).unwrap_or(0),
            created_at: row.try_get("created_at")?,
        })
    }

    pub async fn kb_version(&self) -> RuntimeResult<u64> {
        let version = sqlx::query_scalar::<_, i64>("SELECT version FROM kb_meta WHERE singleton")
            .fetch_one(&self.pool)
            .await?;
        Ok(u64::try_from(version).unwrap_or(0))
    }

    pub async fn bump_kb_version(&self) -> RuntimeResult<u64> {
        let version = sqlx::query_scalar::<_, i64>(
            "UPDATE kb_meta SET version = version + 1 WHERE singleton RETURNING version",
        )
        .fetch_one(&self.pool)
        .await?;
        let version = u64::try_from(version).unwrap_or(0);
        if let Err(error) = self.bus.publish_kb_invalidation(version).await {
            warn!(%error, version, "failed to publish KB invalidation");
        }
        Ok(version)
    }
}

async fn lock_event_seq(
    tx: &mut Transaction<'_, Postgres>,
    session_id: &str,
) -> RuntimeResult<u64> {
    let row =
        sqlx::query("SELECT last_event_seq FROM agent_sessions WHERE session_id = $1 FOR UPDATE")
            .bind(session_id)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or_else(|| RuntimeError::SessionNotFound(session_id.to_string()))?;
    let last_seq: i64 = row.try_get("last_event_seq")?;
    Ok(u64::try_from(last_seq).unwrap_or(0))
}

async fn insert_event(
    tx: &mut Transaction<'_, Postgres>,
    session_id: &str,
    seq: u64,
    run_id: &str,
    agent_id: &str,
    parent_run_id: Option<&str>,
    kind: SessionEventKind,
) -> RuntimeResult<SessionEvent> {
    let event = SessionEvent::new(
        seq,
        run_id,
        agent_id,
        parent_run_id.map(str::to_string),
        kind,
    );
    let event_value = serde_json::to_value(&event)?;
    sqlx::query(
        r#"INSERT INTO session_events
           (session_id, seq, run_id, agent_id, parent_run_id, event_type, event, created_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
    )
    .bind(session_id)
    .bind(i64::try_from(seq).unwrap_or(i64::MAX))
    .bind(run_id)
    .bind(agent_id)
    .bind(parent_run_id)
    .bind(event.event_name())
    .bind(event_value)
    .bind(event.timestamp)
    .execute(&mut **tx)
    .await?;
    Ok(event)
}

async fn update_event_seq(
    tx: &mut Transaction<'_, Postgres>,
    session_id: &str,
    seq: u64,
) -> RuntimeResult<()> {
    sqlx::query(
        "UPDATE agent_sessions SET last_event_seq = $2, updated_at = NOW() WHERE session_id = $1",
    )
    .bind(session_id)
    .bind(i64::try_from(seq).unwrap_or(i64::MAX))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn row_to_event_frame(row: &PgRow) -> RuntimeResult<SessionEventFrame> {
    let seq: i64 = row.try_get("seq")?;
    Ok(SessionEventFrame {
        seq: u64::try_from(seq).unwrap_or(0),
        event_type: row.try_get("event_type")?,
        event: row.try_get("event")?,
    })
}

/// Projection tolerates event variants written by a newer runtime. Transport
/// still relays them verbatim; trace aggregation simply ignores what it cannot
/// classify instead of failing the whole session detail request.
fn decode_projection_events(frames: Vec<SessionEventFrame>) -> Vec<SessionEvent> {
    let mut events = Vec::with_capacity(frames.len());
    for frame in frames {
        match serde_json::from_value::<SessionEvent>(frame.event) {
            Ok(event) => events.push(event),
            Err(error) => {
                warn!(%error, seq = frame.seq, event_type = %frame.event_type, "skipping session event unknown to this runtime during trace projection");
            }
        }
    }
    events
}

fn row_to_pending_question(row: &PgRow) -> RuntimeResult<PendingQuestion> {
    let questions: Value = row.try_get("questions")?;
    Ok(PendingQuestion {
        question_id: row.try_get("question_id")?,
        run_id: row.try_get("run_id")?,
        tool_call_id: row.try_get("tool_call_id")?,
        questions: serde_json::from_value(questions).map_err(|error| {
            RuntimeError::Internal(format!("corrupt question payload: {error}"))
        })?,
        created_at: row.try_get("created_at")?,
    })
}

fn row_to_resolved_question(row: &PgRow) -> RuntimeResult<ResolvedQuestion> {
    let answers: Option<Value> = row.try_get("answers")?;
    Ok(ResolvedQuestion {
        question: row_to_pending_question(row)?,
        status: row.try_get("status")?,
        answers: answers
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| RuntimeError::Internal(format!("corrupt question answers: {error}")))?
            .unwrap_or_default(),
    })
}

fn row_to_summary(row: &PgRow) -> RuntimeResult<SessionSummary> {
    let status_raw: String = row.try_get("status")?;
    let run_raw: String = row.try_get("active_run_status")?;
    let usage_raw: Value = row.try_get("context_usage")?;
    let last_seq: i64 = row.try_get("last_event_seq")?;
    Ok(SessionSummary {
        session_id: row.try_get("session_id")?,
        title: row.try_get("title")?,
        doc_id: row.try_get("doc_id")?,
        linked_doc_id: row.try_get("linked_doc_id")?,
        kb_branch: row.try_get("kb_branch")?,
        status: SessionStatus::parse(&status_raw).ok_or_else(|| {
            RuntimeError::Internal(format!("invalid session status `{status_raw}`"))
        })?,
        active_run_status: ActiveRunStatus::parse(&run_raw).ok_or_else(|| {
            RuntimeError::Internal(format!("invalid active run status `{run_raw}`"))
        })?,
        last_event_seq: u64::try_from(last_seq).unwrap_or(0),
        context_usage: serde_json::from_value(usage_raw).unwrap_or_default(),
        compacting: row
            .try_get::<Option<DateTime<Utc>>, _>("compaction_started_at")?
            .is_some_and(|started| {
                Utc::now().signed_duration_since(started) < chrono::Duration::minutes(10)
            }),
        compacted_at: row.try_get("compacted_at")?,
        pinned_at: row.try_get("pinned_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn row_to_message(row: &PgRow) -> RuntimeResult<SessionMessage> {
    let role_raw: String = row.try_get("role")?;
    let attachment_ids: Value = row.try_get("attachment_ids")?;
    Ok(SessionMessage {
        message_id: row.try_get("message_id")?,
        session_id: row.try_get("session_id")?,
        role: MessageRole::parse(&role_raw)
            .ok_or_else(|| RuntimeError::Internal(format!("invalid message role `{role_raw}`")))?,
        content: row.try_get("content")?,
        comment_refs: serde_json::from_value(row.try_get("comment_refs")?).unwrap_or_default(),
        attachment_ids: serde_json::from_value(attachment_ids).unwrap_or_default(),
        run_id: row.try_get("run_id")?,
        created_at: row.try_get("created_at")?,
    })
}

fn row_to_pending_queue_item(row: &PgRow) -> RuntimeResult<PendingQueueItem> {
    let source_raw: String = row.try_get("source")?;
    let attachment_ids: Value = row.try_get("attachment_ids")?;
    Ok(PendingQueueItem {
        queue_id: row.try_get("queue_id")?,
        session_id: row.try_get("session_id")?,
        content: row.try_get("content")?,
        comment_refs: serde_json::from_value(row.try_get("comment_refs")?).unwrap_or_default(),
        source: PendingQueueSource::parse(&source_raw).ok_or_else(|| {
            RuntimeError::Internal(format!("invalid pending queue source `{source_raw}`"))
        })?,
        thread_id: row.try_get("thread_id")?,
        attachment_ids: serde_json::from_value(attachment_ids).unwrap_or_default(),
        created_at: row.try_get("created_at")?,
    })
}

/// Select the trace owned by the active run. Never fall back to the most recent trace: session
/// control frames and later runs must not be mistaken for the active run's activity.
fn select_active_trace(
    traces: Vec<RunTraceSummary>,
    active_run_id: &str,
) -> Option<RunTraceSummary> {
    traces
        .into_iter()
        .find(|trace| trace.run_id == active_run_id && trace.status == "running")
}

fn active_trace_snapshot(
    summary: &SessionSummary,
    active_run_id: Option<&str>,
    traces: Vec<RunTraceSummary>,
) -> (Option<RunTraceSummary>, Vec<RunTraceSubagent>) {
    if !matches!(
        summary.active_run_status,
        ActiveRunStatus::Running | ActiveRunStatus::AwaitingInput
    ) {
        return (None, Vec::new());
    }
    let Some(trace) = active_run_id.and_then(|run_id| select_active_trace(traces, run_id)) else {
        return (None, Vec::new());
    };
    let mut subagents = Vec::new();
    for segment in &trace.segments {
        if let RunTraceSegment::Execution {
            subagents: items, ..
        } = segment
        {
            subagents.extend(items.iter().cloned());
        }
    }
    (Some(trace), subagents)
}

fn row_to_run(row: &PgRow) -> RuntimeResult<RunRecord> {
    Ok(RunRecord {
        run_id: row.try_get("run_id")?,
        session_id: row.try_get("session_id")?,
        parent_run_id: row.try_get("parent_run_id")?,
        agent_id: row.try_get("agent_id")?,
        status: row.try_get("status")?,
        model: row.try_get("model")?,
        error: row.try_get("error")?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
        start_seq: row
            .try_get::<Option<i64>, _>("start_seq")?
            .map(|seq| u64::try_from(seq).unwrap_or(0)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use quire_session_protocol::RunTraceToolItem;

    fn subagent_thinking(subagent: &RunTraceSubagent) -> String {
        subagent
            .segments
            .iter()
            .filter_map(|segment| match segment {
                RunTraceSegment::Thinking { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    fn subagent_tools(subagent: &RunTraceSubagent) -> Vec<RunTraceToolItem> {
        subagent
            .segments
            .iter()
            .flat_map(|segment| match segment {
                RunTraceSegment::Execution { tools, .. } => tools.clone(),
                _ => Vec::new(),
            })
            .collect()
    }

    fn subagent_thinking_duration_ms(subagent: &RunTraceSubagent) -> Option<u64> {
        let total = subagent
            .segments
            .iter()
            .filter_map(|segment| match segment {
                RunTraceSegment::Thinking { duration_ms, .. } => *duration_ms,
                _ => None,
            })
            .sum::<u64>();
        (total > 0).then_some(total)
    }

    #[test]
    fn role_and_status_mappings_are_total_for_known_rows() {
        assert_eq!(SessionStatus::parse("active"), Some(SessionStatus::Active));
        assert_eq!(
            ActiveRunStatus::parse("interrupted"),
            Some(ActiveRunStatus::Interrupted)
        );
        assert_eq!(MessageRole::parse("user"), Some(MessageRole::User));
    }

    #[test]
    fn aggregates_main_and_subagent_tool_traces() {
        let events = vec![
            SessionEvent::new(1, "run_main", "main", None, SessionEventKind::TurnStarted),
            SessionEvent::new(
                2,
                "run_main",
                "main",
                None,
                SessionEventKind::ThinkingDelta {
                    delta: "plan".into(),
                },
            ),
            SessionEvent::new(
                3,
                "run_main",
                "main",
                None,
                SessionEventKind::TaskFocus {
                    title: "Intro".into(),
                    detail: Some("write".into()),
                    section_id: Some("intro".into()),
                },
            ),
            SessionEvent::new(
                4,
                "run_main",
                "main",
                None,
                SessionEventKind::ToolCallStarted {
                    tool_call_id: "tc_main".into(),
                    tool_name: "apply_operations".into(),
                    input_preview: "{\"ops\":".into(),
                    truncated: false,
                },
            ),
            SessionEvent::new(
                5,
                "sub_1",
                "alpha",
                Some("run_main".into()),
                SessionEventKind::SubagentStarted {
                    subagent_run_id: "sub_1".into(),
                    agent_name: "alpha".into(),
                    display_name: None,
                    task: "draft intro".into(),
                    section_id: Some("intro".into()),
                },
            ),
            SessionEvent::new(
                6,
                "sub_1",
                "alpha",
                Some("run_main".into()),
                SessionEventKind::ThinkingDelta {
                    delta: "sub-thought".into(),
                },
            ),
            SessionEvent::new(
                7,
                "sub_1",
                "alpha",
                Some("run_main".into()),
                SessionEventKind::ToolCallStarted {
                    tool_call_id: "tc_sub".into(),
                    tool_name: "read_blocks".into(),
                    input_preview: "{}".into(),
                    truncated: false,
                },
            ),
            SessionEvent::new(
                8,
                "sub_1",
                "alpha",
                Some("run_main".into()),
                SessionEventKind::ToolCallFinished {
                    tool_call_id: "tc_sub".into(),
                    status: "completed".into(),
                    output_preview: "ok".into(),
                    truncated: false,
                    duration_ms: 42,
                },
            ),
            SessionEvent::new(
                9,
                "sub_1",
                "alpha",
                Some("run_main".into()),
                SessionEventKind::SubagentFinished {
                    subagent_run_id: "sub_1".into(),
                    status: "completed".into(),
                    summary: "intro drafted".into(),
                },
            ),
            SessionEvent::new(
                10,
                "run_main",
                "main",
                None,
                SessionEventKind::ToolCallFinished {
                    tool_call_id: "tc_main".into(),
                    status: "completed".into(),
                    output_preview: "revision 2".into(),
                    truncated: false,
                    duration_ms: 300,
                },
            ),
            SessionEvent::new(11, "run_main", "main", None, SessionEventKind::TurnFinished),
        ];

        let traces = aggregate_run_traces(&events);
        assert_eq!(traces.len(), 1);
        let trace = traces.first().expect("trace");
        assert_eq!(trace.run_id, "run_main");
        assert_eq!(trace.status, "completed");
        assert!(trace.duration_ms.is_some());
        assert_eq!(trace.segments.len(), 2);
        let Some(RunTraceSegment::Thinking { text, .. }) = trace.segments.first() else {
            panic!("first segment should be thinking");
        };
        assert_eq!(text, "plan");
        let focus = trace.task_focus.as_ref().expect("focus");
        assert_eq!(focus.title, "Intro");
        let Some(RunTraceSegment::Execution { tools, subagents }) = trace.segments.get(1) else {
            panic!("second segment should be execution");
        };
        assert_eq!(tools.len(), 1);
        let main_tool = tools.first().expect("main tool");
        assert_eq!(main_tool.status, "completed");
        assert_eq!(main_tool.duration_ms, Some(300));
        assert_eq!(subagents.len(), 1);
        let subagent = subagents.first().expect("subagent");
        assert_eq!(subagent.subagent_run_id, "sub_1");
        assert_eq!(subagent_thinking(subagent), "sub-thought");
        assert_eq!(subagent.status, "completed");
        assert_eq!(subagent.summary.as_deref(), Some("intro drafted"));
        assert_eq!(subagent_tools(subagent).len(), 1);
        assert_eq!(
            subagent_tools(subagent)
                .first()
                .expect("sub tool")
                .duration_ms,
            Some(42)
        );
    }

    #[test]
    fn interleaved_output_starts_new_segments_after_text() {
        let events = vec![
            SessionEvent::new(
                1,
                "run_segments",
                "main",
                None,
                SessionEventKind::TurnStarted,
            ),
            SessionEvent::new(
                2,
                "run_segments",
                "main",
                None,
                SessionEventKind::ThinkingDelta {
                    delta: "first thought".into(),
                },
            ),
            SessionEvent::new(
                3,
                "run_segments",
                "main",
                None,
                SessionEventKind::ToolCallStarted {
                    tool_call_id: "tc_1".into(),
                    tool_name: "apply_operations".into(),
                    input_preview: "{}".into(),
                    truncated: false,
                },
            ),
            SessionEvent::new(
                4,
                "run_segments",
                "main",
                None,
                SessionEventKind::ToolCallFinished {
                    tool_call_id: "tc_1".into(),
                    status: "completed".into(),
                    output_preview: "ok-1".into(),
                    truncated: false,
                    duration_ms: 10,
                },
            ),
            SessionEvent::new(
                5,
                "run_segments",
                "main",
                None,
                SessionEventKind::TextDelta {
                    delta: "first answer".into(),
                },
            ),
            SessionEvent::new(
                6,
                "run_segments",
                "main",
                None,
                SessionEventKind::ThinkingDelta {
                    delta: "second thought".into(),
                },
            ),
            SessionEvent::new(
                7,
                "run_segments",
                "main",
                None,
                SessionEventKind::ToolCallStarted {
                    tool_call_id: "tc_2".into(),
                    tool_name: "lint_document".into(),
                    input_preview: "{}".into(),
                    truncated: false,
                },
            ),
            SessionEvent::new(
                8,
                "run_segments",
                "main",
                None,
                SessionEventKind::ToolCallFinished {
                    tool_call_id: "tc_2".into(),
                    status: "completed".into(),
                    output_preview: "ok-2".into(),
                    truncated: false,
                    duration_ms: 20,
                },
            ),
            SessionEvent::new(
                9,
                "run_segments",
                "main",
                None,
                SessionEventKind::TextDelta {
                    delta: "second answer".into(),
                },
            ),
            SessionEvent::new(
                10,
                "run_segments",
                "main",
                None,
                SessionEventKind::TurnFinished,
            ),
        ];

        let traces = aggregate_run_traces(&events);
        let trace = traces.first().expect("trace");
        assert_eq!(trace.segments.len(), 6);
        assert!(matches!(
            trace.segments.first(),
            Some(RunTraceSegment::Thinking { text, .. }) if text == "first thought"
        ));
        let Some(RunTraceSegment::Execution { tools, .. }) = trace.segments.get(1) else {
            panic!("second segment should be execution");
        };
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools.first().expect("first tool").output_preview.as_deref(),
            Some("ok-1")
        );
        assert!(matches!(
            trace.segments.get(2),
            Some(RunTraceSegment::Text { text }) if text == "first answer"
        ));
        assert!(matches!(
            trace.segments.get(3),
            Some(RunTraceSegment::Thinking { text, .. }) if text == "second thought"
        ));
        let Some(RunTraceSegment::Execution { tools, .. }) = trace.segments.get(4) else {
            panic!("fifth segment should be execution");
        };
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools
                .first()
                .expect("second tool")
                .output_preview
                .as_deref(),
            Some("ok-2")
        );
        assert!(matches!(
            trace.segments.get(5),
            Some(RunTraceSegment::Text { text }) if text == "second answer"
        ));
    }

    #[test]
    fn thinking_durations_use_event_timestamps_not_run_total() {
        let base = Utc::now();
        let event_at = |seq: u64,
                        run_id: &str,
                        agent_id: &str,
                        parent_run_id: Option<&str>,
                        kind: SessionEventKind,
                        offset_ms: i64| {
            let mut event = SessionEvent::new(
                seq,
                run_id,
                agent_id,
                parent_run_id.map(str::to_string),
                kind,
            );
            event.timestamp = base + chrono::Duration::milliseconds(offset_ms);
            event
        };

        let events = vec![
            event_at(
                1,
                "run_duration",
                "main",
                None,
                SessionEventKind::TurnStarted,
                0,
            ),
            event_at(
                2,
                "run_duration",
                "main",
                None,
                SessionEventKind::ThinkingDelta {
                    delta: "plan".into(),
                },
                0,
            ),
            event_at(
                3,
                "run_duration",
                "main",
                None,
                SessionEventKind::ToolCallStarted {
                    tool_call_id: "tc_main".into(),
                    tool_name: "apply_operations".into(),
                    input_preview: "{}".into(),
                    truncated: false,
                },
                4_000,
            ),
            event_at(
                4,
                "sub_duration",
                "analyst",
                Some("run_duration"),
                SessionEventKind::SubagentStarted {
                    subagent_run_id: "sub_duration".into(),
                    agent_name: "analyst".into(),
                    display_name: None,
                    task: "analyse".into(),
                    section_id: None,
                },
                5_000,
            ),
            event_at(
                5,
                "sub_duration",
                "analyst",
                Some("run_duration"),
                SessionEventKind::ThinkingDelta {
                    delta: "first".into(),
                },
                6_000,
            ),
            event_at(
                6,
                "sub_duration",
                "analyst",
                Some("run_duration"),
                SessionEventKind::ToolCallStarted {
                    tool_call_id: "tc_sub".into(),
                    tool_name: "read_blocks".into(),
                    input_preview: "{}".into(),
                    truncated: false,
                },
                8_000,
            ),
            event_at(
                7,
                "sub_duration",
                "analyst",
                Some("run_duration"),
                SessionEventKind::ToolCallFinished {
                    tool_call_id: "tc_sub".into(),
                    status: "completed".into(),
                    output_preview: "ok".into(),
                    truncated: false,
                    duration_ms: 1_000,
                },
                9_000,
            ),
            event_at(
                8,
                "sub_duration",
                "analyst",
                Some("run_duration"),
                SessionEventKind::ThinkingDelta {
                    delta: "second".into(),
                },
                10_000,
            ),
            event_at(
                9,
                "sub_duration",
                "analyst",
                Some("run_duration"),
                SessionEventKind::TextDelta {
                    delta: "answer".into(),
                },
                13_000,
            ),
            event_at(
                10,
                "sub_duration",
                "analyst",
                Some("run_duration"),
                SessionEventKind::SubagentFinished {
                    subagent_run_id: "sub_duration".into(),
                    status: "completed".into(),
                    summary: "done".into(),
                },
                14_000,
            ),
            event_at(
                11,
                "run_duration",
                "main",
                None,
                SessionEventKind::ToolCallFinished {
                    tool_call_id: "tc_main".into(),
                    status: "completed".into(),
                    output_preview: "revision 2".into(),
                    truncated: false,
                    duration_ms: 50_000,
                },
                54_000,
            ),
            event_at(
                12,
                "run_duration",
                "main",
                None,
                SessionEventKind::TextDelta {
                    delta: "final answer".into(),
                },
                54_000,
            ),
            event_at(
                13,
                "run_duration",
                "main",
                None,
                SessionEventKind::TurnFinished,
                60_000,
            ),
        ];

        let traces = aggregate_run_traces(&events);
        let trace = traces.first().expect("trace");
        assert_eq!(trace.duration_ms, Some(60_000));

        let Some(RunTraceSegment::Thinking { duration_ms, .. }) = trace.segments.first() else {
            panic!("first segment should be thinking");
        };
        assert_eq!(*duration_ms, Some(4_000));
        assert_ne!(*duration_ms, trace.duration_ms);

        let Some(RunTraceSegment::Execution { subagents, .. }) = trace.segments.get(1) else {
            panic!("second segment should be execution");
        };
        let subagent = subagents.first().expect("subagent");
        assert_eq!(subagent_thinking_duration_ms(subagent), Some(5_000));
    }

    #[test]
    fn same_agent_name_runs_stay_as_separate_subagents() {
        let events = vec![
            SessionEvent::new(1, "run_dup", "main", None, SessionEventKind::TurnStarted),
            SessionEvent::new(
                2,
                "run_dup",
                "main",
                None,
                SessionEventKind::SubagentStarted {
                    subagent_run_id: "run_sub_a".into(),
                    agent_name: "main".into(),
                    display_name: None,
                    task: "task A".into(),
                    section_id: None,
                },
            ),
            SessionEvent::new(
                3,
                "run_dup",
                "main",
                None,
                SessionEventKind::SubagentStarted {
                    subagent_run_id: "run_sub_b".into(),
                    agent_name: "main".into(),
                    display_name: None,
                    task: "task B".into(),
                    section_id: None,
                },
            ),
            SessionEvent::new(
                4,
                "run_dup",
                "main",
                None,
                SessionEventKind::SubagentFinished {
                    subagent_run_id: "run_sub_a".into(),
                    status: "completed".into(),
                    summary: "done A".into(),
                },
            ),
            SessionEvent::new(
                5,
                "run_dup",
                "main",
                None,
                SessionEventKind::SubagentFinished {
                    subagent_run_id: "run_sub_b".into(),
                    status: "completed".into(),
                    summary: "done B".into(),
                },
            ),
            SessionEvent::new(6, "run_dup", "main", None, SessionEventKind::TurnFinished),
        ];

        let traces = aggregate_run_traces(&events);
        let trace = traces.first().expect("trace");
        let Some(RunTraceSegment::Execution { subagents, .. }) = trace.segments.first() else {
            panic!("first segment should be execution");
        };
        assert_eq!(subagents.len(), 2);
        assert_eq!(subagents.first().expect("sub A").task, "task A");
        assert_eq!(
            subagents.first().expect("sub A").summary.as_deref(),
            Some("done A")
        );
        assert_eq!(subagents.get(1).expect("sub B").task, "task B");
        assert_eq!(
            subagents.get(1).expect("sub B").summary.as_deref(),
            Some("done B")
        );
    }

    #[test]
    fn recoverable_turn_failure_marks_trace_interrupted() {
        let events = vec![
            SessionEvent::new(
                1,
                "run_interrupted",
                "main",
                None,
                SessionEventKind::TurnStarted,
            ),
            SessionEvent::new(
                2,
                "run_interrupted",
                "main",
                None,
                SessionEventKind::TurnFailed {
                    error: "cancelled".into(),
                    recoverable: true,
                },
            ),
        ];
        let traces = aggregate_run_traces(&events);
        let trace = traces.first().expect("trace");
        assert_eq!(trace.status, "interrupted");
        assert!(trace.duration_ms.is_some());
    }

    fn trace_summary(run_id: &str, status: &str) -> RunTraceSummary {
        RunTraceSummary {
            run_id: run_id.into(),
            status: status.into(),
            started_at: None,
            duration_ms: None,
            task_focus: None,
            segments: Vec::new(),
        }
    }

    fn running_summary(status: ActiveRunStatus) -> SessionSummary {
        SessionSummary {
            session_id: "sess_1".into(),
            title: "title".into(),
            doc_id: None,
            linked_doc_id: None,
            kb_branch: None,
            status: SessionStatus::Active,
            active_run_status: status,
            last_event_seq: 0,
            context_usage: quire_session_protocol::ContextUsage::default(),
            compacting: false,
            compacted_at: None,
            pinned_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn session_control_frames_do_not_create_phantom_traces() {
        let events = vec![
            SessionEvent::new(1, "run_main", "main", None, SessionEventKind::TurnStarted),
            SessionEvent::new(
                2,
                "queue",
                "system",
                None,
                SessionEventKind::PendingQueueUpdated { items: vec![] },
            ),
            SessionEvent::new(
                3,
                "compaction",
                "main",
                None,
                SessionEventKind::CompactionStarted,
            ),
            SessionEvent::new(
                4,
                "compaction",
                "main",
                None,
                SessionEventKind::CompactionFinished { success: true },
            ),
            SessionEvent::new(
                5,
                "run_main",
                "queue",
                None,
                SessionEventKind::UserMessageAdded {
                    message_id: "msg_1".into(),
                },
            ),
            SessionEvent::new(
                6,
                "run_main",
                "main",
                None,
                SessionEventKind::DocLinked {
                    doc_id: "doc_1".into(),
                    title: "Report".into(),
                },
            ),
            SessionEvent::new(
                7,
                "run_main",
                "main",
                None,
                SessionEventKind::SessionTitleUpdated {
                    title: "Report".into(),
                },
            ),
            SessionEvent::new(
                8,
                "run_main",
                "main",
                None,
                SessionEventKind::ThinkingDelta {
                    delta: "plan".into(),
                },
            ),
            SessionEvent::new(9, "run_main", "main", None, SessionEventKind::TurnFinished),
        ];

        let traces = aggregate_run_traces(&events);
        assert_eq!(traces.len(), 1, "control frames must not create traces");
        let trace = traces.first().expect("run trace");
        assert_eq!(trace.run_id, "run_main");
        assert_eq!(
            trace.started_at,
            events.first().map(|event| event.timestamp),
            "the run clock starts at turn_started, not at a queue signal"
        );
    }

    #[test]
    fn active_trace_is_selected_by_run_id_not_by_recency() {
        let traces = vec![
            trace_summary("run_main", "running"),
            trace_summary("queue", "running"),
        ];
        let selected = select_active_trace(traces.clone(), "run_main").expect("active trace");
        assert_eq!(selected.run_id, "run_main");
        assert!(select_active_trace(traces.clone(), "run_missing").is_none());

        let completed = vec![trace_summary("run_main", "completed")];
        assert!(select_active_trace(completed, "run_main").is_none());
    }

    #[test]
    fn active_snapshot_requires_the_active_run_id() {
        let summary = running_summary(ActiveRunStatus::Running);
        let traces = vec![trace_summary("queue", "running")];

        let (trace, subagents) = active_trace_snapshot(&summary, None, traces.clone());
        assert!(trace.is_none());
        assert!(subagents.is_empty());

        let (trace, _) = active_trace_snapshot(&summary, Some("run_main"), traces);
        assert!(trace.is_none(), "a queue trace must not hydrate a run");

        let traces = vec![trace_summary("run_main", "running")];
        let (trace, _) = active_trace_snapshot(&summary, Some("run_main"), traces);
        assert_eq!(trace.map(|trace| trace.run_id), Some("run_main".into()));

        let idle = running_summary(ActiveRunStatus::Idle);
        let traces = vec![trace_summary("run_main", "running")];
        let (trace, _) = active_trace_snapshot(&idle, Some("run_main"), traces);
        assert!(trace.is_none());
    }
}
