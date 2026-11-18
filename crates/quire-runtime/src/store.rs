//! PostgreSQL persistence for sessions, messages, runs, events and tool calls.

use chrono::{DateTime, Utc};
use quire_session_protocol::{
    ActiveRunStatus, AttachmentMeta, MessageRole, SendMessageRequest, SessionDetail, SessionEvent,
    SessionEventKind, SessionMessage, SessionStatus, SessionSummary, ToolCallDetail,
};
use serde_json::Value;
use sqlx::{PgPool, Row, postgres::PgRow};
use tracing::warn;
use uuid::Uuid;

use crate::{
    bus::RedisBus,
    error::{RuntimeError, RuntimeResult},
};

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
        let title = title.trim();
        if title.is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "session title must not be empty".into(),
            ));
        }
        let session_id = format!("sess_{}", Uuid::new_v4().simple());
        sqlx::query(
            r#"INSERT INTO agent_sessions
               (session_id, title, doc_id, linked_doc_id, status, active_run_status)
               VALUES ($1, $2, $3, $3, 'active', 'idle')"#,
        )
        .bind(&session_id)
        .bind(title)
        .bind(doc_id)
        .execute(&self.pool)
        .await?;
        self.get_session_summary(&session_id).await
    }

    pub async fn get_session_summary(&self, session_id: &str) -> RuntimeResult<SessionSummary> {
        let row = sqlx::query(
            r#"SELECT session_id, title, doc_id, linked_doc_id, status, active_run_status,
                      last_event_seq, context_usage, created_at, updated_at
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
        let messages = self.list_messages(session_id).await?;
        let attachments = self.list_attachments(session_id).await?;
        Ok(SessionDetail {
            summary,
            messages,
            attachments,
        })
    }

    pub async fn list_sessions(
        &self,
        limit: i64,
        offset: i64,
    ) -> RuntimeResult<Vec<SessionSummary>> {
        let rows = sqlx::query(
            r#"SELECT session_id, title, doc_id, linked_doc_id, status, active_run_status,
                      last_event_seq, context_usage, created_at, updated_at
               FROM agent_sessions
               ORDER BY updated_at DESC, session_id ASC
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
        title: Option<&str>,
        status: Option<SessionStatus>,
    ) -> RuntimeResult<SessionSummary> {
        let updated = sqlx::query(
            r#"UPDATE agent_sessions
               SET title = COALESCE($2, title),
                   status = COALESCE($3, status),
                   updated_at = NOW()
               WHERE session_id = $1"#,
        )
        .bind(session_id)
        .bind(title)
        .bind(status.map(SessionStatus::as_str))
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
        if request.client_msg_id.trim().is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "client_msg_id must not be empty".into(),
            ));
        }
        if request.content.trim().is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "message content must not be empty".into(),
            ));
        }
        let message_id = format!("msg_{}", Uuid::new_v4().simple());
        let attachment_ids = serde_json::to_value(&request.attachment_ids)?;
        let inserted = sqlx::query(
            r#"INSERT INTO agent_messages
               (message_id, session_id, client_msg_id, role, content, attachment_ids)
               VALUES ($1, $2, $3, 'user', $4, $5)
               ON CONFLICT (session_id, client_msg_id) DO NOTHING
               RETURNING message_id, session_id, role, content, attachment_ids, run_id, created_at"#,
        )
        .bind(&message_id)
        .bind(session_id)
        .bind(&request.client_msg_id)
        .bind(&request.content)
        .bind(attachment_ids)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = inserted {
            return Ok((row_to_message(&row)?, true));
        }
        let row = sqlx::query(
            r#"SELECT message_id, session_id, role, content, attachment_ids, run_id, created_at
               FROM agent_messages WHERE session_id = $1 AND client_msg_id = $2"#,
        )
        .bind(session_id)
        .bind(&request.client_msg_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| RuntimeError::Internal("idempotent message vanished".into()))?;
        Ok((row_to_message(&row)?, false))
    }

    pub async fn insert_assistant_message(
        &self,
        session_id: &str,
        content: &str,
        run_id: &str,
    ) -> RuntimeResult<SessionMessage> {
        let message_id = format!("msg_{}", Uuid::new_v4().simple());
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
            r#"SELECT message_id, session_id, role, content, attachment_ids, run_id, created_at
               FROM agent_messages WHERE message_id = $1"#,
        )
        .bind(&message_id)
        .fetch_one(&self.pool)
        .await?;
        row_to_message(&row)
    }

    pub async fn list_messages(&self, session_id: &str) -> RuntimeResult<Vec<SessionMessage>> {
        let rows = sqlx::query(
            r#"SELECT message_id, session_id, role, content, attachment_ids, run_id, created_at
               FROM agent_messages WHERE session_id = $1
               ORDER BY created_at ASC, message_id ASC"#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_message).collect()
    }

    pub async fn insert_attachment(
        &self,
        session_id: &str,
        filename: &str,
        mime_type: &str,
        size_bytes: u64,
        storage_path: &str,
    ) -> RuntimeResult<AttachmentMeta> {
        let attachment_id = format!("att_{}", Uuid::new_v4().simple());
        sqlx::query(
            r#"INSERT INTO agent_attachments
               (attachment_id, session_id, filename, mime_type, size_bytes, storage_path)
               VALUES ($1, $2, $3, $4, $5, $6)"#,
        )
        .bind(&attachment_id)
        .bind(session_id)
        .bind(filename)
        .bind(mime_type)
        .bind(i64::try_from(size_bytes).unwrap_or(i64::MAX))
        .bind(storage_path)
        .execute(&self.pool)
        .await?;
        Ok(AttachmentMeta {
            attachment_id,
            filename: filename.to_string(),
            mime_type: mime_type.to_string(),
            size_bytes,
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
        let run_id = format!("run_{}", Uuid::new_v4().simple());
        sqlx::query(
            r#"INSERT INTO agent_runs (run_id, session_id, parent_run_id, agent_id, status, model)
               VALUES ($1, $2, $3, $4, 'running', $5)"#,
        )
        .bind(&run_id)
        .bind(session_id)
        .bind(parent_run_id)
        .bind(agent_id)
        .bind(model)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            r#"UPDATE agent_sessions SET active_run_status = 'running', updated_at = NOW()
               WHERE session_id = $1"#,
        )
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        self.get_run(&run_id).await
    }

    pub async fn get_run(&self, run_id: &str) -> RuntimeResult<RunRecord> {
        let row = sqlx::query(
            r#"SELECT run_id, session_id, parent_run_id, agent_id, status, model, error,
                      started_at, finished_at
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
                      started_at, finished_at
               FROM agent_runs
               WHERE session_id = $1 AND status = 'running'
               ORDER BY started_at DESC LIMIT 1"#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| row_to_run(&row)).transpose()
    }

    pub async fn finish_run(
        &self,
        run_id: &str,
        status: &str,
        error: Option<&str>,
    ) -> RuntimeResult<()> {
        let row = sqlx::query(
            r#"UPDATE agent_runs
               SET status = $2, error = $3, finished_at = NOW()
               WHERE run_id = $1 AND status = 'running'
               RETURNING session_id"#,
        )
        .bind(run_id)
        .bind(status)
        .bind(error)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            let exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM agent_runs WHERE run_id = $1)",
            )
            .bind(run_id)
            .fetch_one(&self.pool)
            .await?;
            return if exists {
                Ok(())
            } else {
                Err(RuntimeError::RunNotFound(run_id.to_string()))
            };
        };
        let session_id: String = row.try_get("session_id")?;
        let session_status = match status {
            "completed" => "idle",
            "cancelled" | "interrupted" => "interrupted",
            "failed" => "failed",
            _ => "idle",
        };
        sqlx::query(
            r#"UPDATE agent_sessions SET active_run_status = $2, updated_at = NOW()
               WHERE session_id = $1"#,
        )
        .bind(&session_id)
        .bind(session_status)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn interrupt_run(
        &self,
        run_id: &str,
        reason: &str,
    ) -> RuntimeResult<Option<RunRecord>> {
        let row = sqlx::query(
            r#"UPDATE agent_runs
               SET status = 'cancelled', error = $2, finished_at = NOW()
               WHERE run_id = $1 AND status = 'running'
               RETURNING run_id, session_id, parent_run_id, agent_id, status, model, error,
                         started_at, finished_at"#,
        )
        .bind(run_id)
        .bind(reason)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let run = row_to_run(&row)?;
        sqlx::query(
            r#"UPDATE agent_sessions SET active_run_status = 'interrupted', updated_at = NOW()
               WHERE session_id = $1"#,
        )
        .bind(&run.session_id)
        .execute(&self.pool)
        .await?;
        Ok(Some(run))
    }

    /// Mark every surviving `running` run as interrupted after a runtime restart.
    pub async fn recover_running_runs(&self) -> RuntimeResult<usize> {
        let rows = sqlx::query(
            r#"UPDATE agent_runs
               SET status = 'interrupted',
                   error = COALESCE(error, 'runtime restarted while run was active'),
                   finished_at = NOW()
               WHERE status = 'running'
               RETURNING run_id, session_id, parent_run_id, agent_id, status, model, error,
                         started_at, finished_at"#,
        )
        .fetch_all(&self.pool)
        .await?;
        for row in &rows {
            let run = row_to_run(row)?;
            sqlx::query(
                r#"UPDATE agent_sessions SET active_run_status = 'interrupted', updated_at = NOW()
                   WHERE session_id = $1"#,
            )
            .bind(&run.session_id)
            .execute(&self.pool)
            .await?;
            let _ = self
                .append_event(
                    &run.session_id,
                    &run.run_id,
                    &run.agent_id,
                    run.parent_run_id.as_deref(),
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

    pub async fn append_event(
        &self,
        session_id: &str,
        run_id: &str,
        agent_id: &str,
        parent_run_id: Option<&str>,
        kind: SessionEventKind,
    ) -> RuntimeResult<SessionEvent> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT last_event_seq FROM agent_sessions WHERE session_id = $1 FOR UPDATE",
        )
        .bind(session_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| RuntimeError::SessionNotFound(session_id.to_string()))?;
        let last_seq: i64 = row.try_get("last_event_seq")?;
        let seq = u64::try_from(last_seq).unwrap_or(0).saturating_add(1);
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
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE agent_sessions SET last_event_seq = $2, updated_at = NOW() WHERE session_id = $1",
        )
        .bind(session_id)
        .bind(i64::try_from(seq).unwrap_or(i64::MAX))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        if let Err(error) = self.bus.publish_event(&event, session_id).await {
            warn!(%error, %session_id, seq, "failed to publish live session event");
        }
        Ok(event)
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
        .execute(&self.pool)
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
        status: SessionStatus::parse(&status_raw).ok_or_else(|| {
            RuntimeError::Internal(format!("invalid session status `{status_raw}`"))
        })?,
        active_run_status: ActiveRunStatus::parse(&run_raw).ok_or_else(|| {
            RuntimeError::Internal(format!("invalid active run status `{run_raw}`"))
        })?,
        last_event_seq: u64::try_from(last_seq).unwrap_or(0),
        context_usage: serde_json::from_value(usage_raw).unwrap_or_default(),
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
        attachment_ids: serde_json::from_value(attachment_ids).unwrap_or_default(),
        run_id: row.try_get("run_id")?,
        created_at: row.try_get("created_at")?,
    })
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_and_status_mappings_are_total_for_known_rows() {
        assert_eq!(SessionStatus::parse("active"), Some(SessionStatus::Active));
        assert_eq!(
            ActiveRunStatus::parse("interrupted"),
            Some(ActiveRunStatus::Interrupted)
        );
        assert_eq!(MessageRole::parse("user"), Some(MessageRole::User));
    }
}
