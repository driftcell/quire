//! PostgreSQL persistence for sessions, messages, runs, events and tool calls.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use quire_protocol::short_id;
use quire_session_protocol::{
    ActiveRunStatus, AttachmentMeta, MessageRole, PendingQuestion, PendingQueueItem,
    PendingQueueSource, QuestionAnswer, QuestionItem, ResolvedQuestion, RunTaskFocus,
    RunTraceSegment, RunTraceSubagent, RunTraceSummary, RunTraceToolItem, SendMessageRequest,
    SessionDetail, SessionEvent, SessionEventKind, SessionMessage, SessionStatus, SessionSummary,
    ToolCallDetail,
};
use serde_json::Value;
use sqlx::{PgPool, Row, postgres::PgRow};
use tracing::warn;

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
                      active_run_status, last_event_seq, context_usage, created_at, updated_at
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
        })
    }

    pub async fn list_sessions(
        &self,
        limit: i64,
        offset: i64,
    ) -> RuntimeResult<Vec<SessionSummary>> {
        let rows = sqlx::query(
            r#"SELECT session_id, title, doc_id, linked_doc_id, kb_branch, status,
                      active_run_status, last_event_seq, context_usage, created_at, updated_at
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
        let message_id = short_id("msg");
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

    pub async fn find_message_by_client_id(
        &self,
        session_id: &str,
        client_msg_id: &str,
    ) -> RuntimeResult<Option<SessionMessage>> {
        let row = sqlx::query(
            r#"SELECT message_id, session_id, role, content, attachment_ids, run_id, created_at
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
            r#"SELECT queue_id, session_id, content, source, thread_id, attachment_ids, created_at
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
        source: PendingQueueSource,
        thread_id: Option<&str>,
        attachment_ids: &[String],
    ) -> RuntimeResult<PendingQueueItem> {
        if client_msg_id.trim().is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "pending queue client_msg_id must not be empty".into(),
            ));
        }
        if content.trim().is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "pending queue content must not be empty".into(),
            ));
        }
        self.get_session_summary(session_id).await?;
        let queue_id = short_id("q");
        let attachments = serde_json::to_value(attachment_ids)?;
        let inserted = sqlx::query(
            r#"INSERT INTO session_pending_queue
               (queue_id, session_id, client_msg_id, content, source, thread_id, attachment_ids)
               VALUES ($1, $2, $3, $4, $5, $6, $7)
               ON CONFLICT (session_id, client_msg_id) DO NOTHING
               RETURNING queue_id, session_id, content, source, thread_id, attachment_ids, created_at"#,
        )
        .bind(&queue_id)
        .bind(session_id)
        .bind(client_msg_id)
        .bind(content)
        .bind(source.as_str())
        .bind(thread_id)
        .bind(attachments)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = inserted {
            return row_to_pending_queue_item(&row);
        }
        let row = sqlx::query(
            r#"SELECT queue_id, session_id, content, source, thread_id, attachment_ids, created_at
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
    ) -> RuntimeResult<PendingQueueItem> {
        if content.trim().is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "pending queue content must not be empty".into(),
            ));
        }
        let row = sqlx::query(
            r#"UPDATE session_pending_queue
               SET content = $3
               WHERE session_id = $1 AND queue_id = $2
               RETURNING queue_id, session_id, content, source, thread_id, attachment_ids, created_at"#,
        )
        .bind(session_id)
        .bind(queue_id)
        .bind(content)
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
            r#"SELECT queue_id, session_id, content, source, thread_id, attachment_ids, created_at
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
            r#"SELECT queue_id, session_id, content, source, thread_id, attachment_ids, created_at
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
               RETURNING message_id, session_id, role, content, attachment_ids, run_id, created_at"#,
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
            r#"SELECT message_id, session_id, role, content, attachment_ids, run_id, created_at
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
        .fetch_one(&self.pool)
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
               RETURNING session_id"#,
        )
        .bind(run_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        let session_id: String = row.try_get("session_id")?;
        sqlx::query(
            r#"UPDATE agent_sessions SET active_run_status = 'idle', updated_at = NOW()
               WHERE session_id = $1"#,
        )
        .bind(&session_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(session_id))
    }

    pub async fn insert_attachment(
        &self,
        session_id: &str,
        filename: &str,
        mime_type: &str,
        size_bytes: u64,
        storage_path: &str,
    ) -> RuntimeResult<AttachmentMeta> {
        let attachment_id = short_id("att");
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

    pub async fn latest_run(&self, session_id: &str) -> RuntimeResult<Option<RunRecord>> {
        let row = sqlx::query(
            r#"SELECT run_id, session_id, parent_run_id, agent_id, status, model, error,
                      started_at, finished_at
               FROM agent_runs
               WHERE session_id = $1
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

    /// Aggregate persisted `session_events` into lightweight per-run traces.
    pub async fn list_run_traces(&self, session_id: &str) -> RuntimeResult<Vec<RunTraceSummary>> {
        self.get_session_summary(session_id).await?;
        let rows = sqlx::query(
            r#"SELECT event FROM session_events
               WHERE session_id = $1
               ORDER BY seq ASC"#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        let mut events = Vec::with_capacity(rows.len());
        for row in &rows {
            let value: Value = row.try_get("event")?;
            events.push(serde_json::from_value::<SessionEvent>(value)?);
        }
        Ok(aggregate_run_traces(&events))
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

fn row_to_pending_queue_item(row: &PgRow) -> RuntimeResult<PendingQueueItem> {
    let source_raw: String = row.try_get("source")?;
    let attachment_ids: Value = row.try_get("attachment_ids")?;
    Ok(PendingQueueItem {
        queue_id: row.try_get("queue_id")?,
        session_id: row.try_get("session_id")?,
        content: row.try_get("content")?,
        source: PendingQueueSource::parse(&source_raw).ok_or_else(|| {
            RuntimeError::Internal(format!("invalid pending queue source `{source_raw}`"))
        })?,
        thread_id: row.try_get("thread_id")?,
        attachment_ids: serde_json::from_value(attachment_ids).unwrap_or_default(),
        created_at: row.try_get("created_at")?,
    })
}

struct RunTraceAccumulator {
    summary: RunTraceSummary,
    started_at: Option<DateTime<Utc>>,
}

fn ensure_run_trace(
    traces: &mut Vec<RunTraceAccumulator>,
    index: &mut HashMap<String, usize>,
    run_id: &str,
    timestamp: DateTime<Utc>,
) -> usize {
    if let Some(position) = index.get(run_id) {
        return *position;
    }
    let position = traces.len();
    traces.push(RunTraceAccumulator {
        summary: RunTraceSummary {
            run_id: run_id.to_string(),
            status: "running".to_string(),
            duration_ms: None,
            task_focus: None,
            segments: Vec::new(),
        },
        started_at: Some(timestamp),
    });
    index.insert(run_id.to_string(), position);
    position
}

fn is_subagent_event(event: &SessionEvent) -> bool {
    event.parent_run_id.is_some() || event.agent_id != "main"
}

fn append_thinking_segment(summary: &mut RunTraceSummary, delta: &str) {
    if let Some(RunTraceSegment::Thinking { text }) = summary.segments.last_mut() {
        text.push_str(delta);
        return;
    }
    summary.segments.push(RunTraceSegment::Thinking {
        text: delta.to_string(),
    });
}

fn append_text_segment(summary: &mut RunTraceSummary, delta: &str) {
    if let Some(RunTraceSegment::Text { text }) = summary.segments.last_mut() {
        text.push_str(delta);
        return;
    }
    summary.segments.push(RunTraceSegment::Text {
        text: delta.to_string(),
    });
}

fn append_execution_tool(summary: &mut RunTraceSummary, item: RunTraceToolItem) {
    if let Some(RunTraceSegment::Execution { tools, .. }) = summary.segments.last_mut() {
        tools.push(item);
        return;
    }
    summary.segments.push(RunTraceSegment::Execution {
        tools: vec![item],
        subagents: Vec::new(),
    });
}

fn append_execution_subagent(
    summary: &mut RunTraceSummary,
    subagent: RunTraceSubagent,
) -> Option<(usize, usize)> {
    if let Some(segment_index) = summary.segments.len().checked_sub(1)
        && let Some(RunTraceSegment::Execution { subagents, .. }) =
            summary.segments.get_mut(segment_index)
    {
        subagents.push(subagent);
        return Some((segment_index, subagents.len().saturating_sub(1)));
    }
    summary.segments.push(RunTraceSegment::Execution {
        tools: Vec::new(),
        subagents: vec![subagent],
    });
    Some((summary.segments.len().saturating_sub(1), 0))
}

fn find_tool_location(summary: &RunTraceSummary, tool_call_id: &str) -> Option<(usize, usize)> {
    for (segment_index, segment) in summary.segments.iter().enumerate().rev() {
        if let RunTraceSegment::Execution { tools, .. } = segment
            && let Some(tool_index) = tools
                .iter()
                .rposition(|tool| tool.tool_call_id == tool_call_id)
        {
            return Some((segment_index, tool_index));
        }
    }
    None
}

fn finish_tool(
    summary: &mut RunTraceSummary,
    tool_call_id: &str,
    status: &str,
    output_preview: &str,
    truncated: bool,
    duration_ms: u64,
) {
    let Some((segment_index, tool_index)) = find_tool_location(summary, tool_call_id) else {
        return;
    };
    let Some(RunTraceSegment::Execution { tools, .. }) = summary.segments.get_mut(segment_index)
    else {
        return;
    };
    let Some(tool) = tools.get_mut(tool_index) else {
        return;
    };
    tool.status = status.to_string();
    tool.output_preview = Some(output_preview.to_string());
    tool.duration_ms = Some(duration_ms);
    tool.truncated = truncated;
}

fn find_subagent_location<F>(summary: &RunTraceSummary, matches: F) -> Option<(usize, usize)>
where
    F: Fn(&RunTraceSubagent) -> bool,
{
    for (segment_index, segment) in summary.segments.iter().enumerate().rev() {
        if let RunTraceSegment::Execution { subagents, .. } = segment
            && let Some(subagent_index) = subagents.iter().rposition(&matches)
        {
            return Some((segment_index, subagent_index));
        }
    }
    None
}

fn subagent_mut(
    summary: &mut RunTraceSummary,
    (segment_index, subagent_index): (usize, usize),
) -> Option<&mut RunTraceSubagent> {
    let Some(RunTraceSegment::Execution { subagents, .. }) =
        summary.segments.get_mut(segment_index)
    else {
        return None;
    };
    subagents.get_mut(subagent_index)
}

fn ensure_subagent(summary: &mut RunTraceSummary, event: &SessionEvent) -> Option<(usize, usize)> {
    if let Some(location) =
        find_subagent_location(summary, |sub| sub.subagent_run_id == event.run_id)
    {
        return Some(location);
    }
    append_execution_subagent(
        summary,
        RunTraceSubagent {
            subagent_run_id: event.run_id.clone(),
            agent_name: event.agent_id.clone(),
            display_name: None,
            task: String::new(),
            section_id: None,
            status: "running".to_string(),
            summary: None,
            thinking: String::new(),
            task_focus: None,
            tools: Vec::new(),
        },
    )
}

fn elapsed_ms(started_at: Option<DateTime<Utc>>, finished_at: DateTime<Utc>) -> Option<u64> {
    started_at.and_then(|start| u64::try_from((finished_at - start).num_milliseconds().max(0)).ok())
}

fn upsert_tool_finished(
    tools: &mut Vec<RunTraceToolItem>,
    tool_call_id: &str,
    status: &str,
    output_preview: &str,
    truncated: bool,
    duration_ms: u64,
) {
    if let Some(tool) = tools
        .iter_mut()
        .find(|item| item.tool_call_id == tool_call_id)
    {
        tool.status = status.to_string();
        tool.output_preview = Some(output_preview.to_string());
        tool.duration_ms = Some(duration_ms);
        tool.truncated = truncated;
        return;
    }
    tools.push(RunTraceToolItem {
        tool_call_id: tool_call_id.to_string(),
        tool_name: String::new(),
        status: status.to_string(),
        input_preview: String::new(),
        output_preview: Some(output_preview.to_string()),
        duration_ms: Some(duration_ms),
        truncated,
    });
}

/// Pure aggregation over persisted session events, ordered by their SSE sequence.
pub(crate) fn aggregate_run_traces(events: &[SessionEvent]) -> Vec<RunTraceSummary> {
    let mut traces: Vec<RunTraceAccumulator> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();

    for event in events {
        let trace_run_id = event.parent_run_id.as_deref().unwrap_or(&event.run_id);
        let position = ensure_run_trace(&mut traces, &mut index, trace_run_id, event.timestamp);
        let Some(trace) = traces.get_mut(position) else {
            continue;
        };
        let subagent_event = is_subagent_event(event);

        match &event.kind {
            SessionEventKind::TurnStarted => {
                trace.summary.status = "running".to_string();
                if trace.started_at.is_none() {
                    trace.started_at = Some(event.timestamp);
                }
            }
            SessionEventKind::TurnFinished => {
                trace.summary.status = "completed".to_string();
                trace.summary.duration_ms = elapsed_ms(trace.started_at, event.timestamp);
            }
            SessionEventKind::TurnFailed { recoverable, .. } => {
                trace.summary.status = if *recoverable {
                    "interrupted".to_string()
                } else {
                    "failed".to_string()
                };
                trace.summary.duration_ms = elapsed_ms(trace.started_at, event.timestamp);
            }
            SessionEventKind::ThinkingDelta { delta } => {
                if subagent_event {
                    if let Some(location) = ensure_subagent(&mut trace.summary, event)
                        && let Some(record) = subagent_mut(&mut trace.summary, location)
                    {
                        record.thinking.push_str(delta);
                    }
                } else {
                    append_thinking_segment(&mut trace.summary, delta);
                }
            }
            SessionEventKind::TextDelta { delta } => {
                if !subagent_event {
                    append_text_segment(&mut trace.summary, delta);
                }
            }
            SessionEventKind::TaskFocus {
                title,
                detail,
                section_id,
            } => {
                let focus = RunTaskFocus {
                    title: title.clone(),
                    detail: detail.clone(),
                    section_id: section_id.clone(),
                };
                if subagent_event {
                    if let Some(location) = ensure_subagent(&mut trace.summary, event)
                        && let Some(record) = subagent_mut(&mut trace.summary, location)
                    {
                        record.task_focus = Some(focus);
                    }
                } else {
                    trace.summary.task_focus = Some(focus);
                }
            }
            SessionEventKind::ToolCallStarted {
                tool_call_id,
                tool_name,
                input_preview,
                truncated,
            } => {
                let item = RunTraceToolItem {
                    tool_call_id: tool_call_id.clone(),
                    tool_name: tool_name.clone(),
                    status: "running".to_string(),
                    input_preview: input_preview.clone(),
                    output_preview: None,
                    duration_ms: None,
                    truncated: *truncated,
                };
                if subagent_event {
                    if let Some(location) = ensure_subagent(&mut trace.summary, event)
                        && let Some(record) = subagent_mut(&mut trace.summary, location)
                    {
                        record.tools.push(item);
                    }
                } else {
                    append_execution_tool(&mut trace.summary, item);
                }
            }
            SessionEventKind::ToolCallFinished {
                tool_call_id,
                status,
                output_preview,
                truncated,
                duration_ms,
            } => {
                if subagent_event {
                    if let Some(location) = ensure_subagent(&mut trace.summary, event)
                        && let Some(record) = subagent_mut(&mut trace.summary, location)
                    {
                        upsert_tool_finished(
                            &mut record.tools,
                            tool_call_id,
                            status,
                            output_preview,
                            *truncated,
                            *duration_ms,
                        );
                    }
                } else {
                    finish_tool(
                        &mut trace.summary,
                        tool_call_id,
                        status,
                        output_preview,
                        *truncated,
                        *duration_ms,
                    );
                }
            }
            SessionEventKind::SubagentStarted {
                subagent_run_id,
                agent_name,
                display_name,
                task,
                section_id,
            } => {
                let existing = find_subagent_location(&trace.summary, |sub| {
                    sub.subagent_run_id == *subagent_run_id
                });
                if let Some(location) = existing
                    && let Some(subagent) = subagent_mut(&mut trace.summary, location)
                {
                    subagent.subagent_run_id = subagent_run_id.clone();
                    subagent.agent_name = agent_name.clone();
                    subagent.display_name = display_name.clone();
                    subagent.task = task.clone();
                    subagent.section_id = section_id.clone();
                    subagent.status = "running".to_string();
                } else {
                    append_execution_subagent(
                        &mut trace.summary,
                        RunTraceSubagent {
                            subagent_run_id: subagent_run_id.clone(),
                            agent_name: agent_name.clone(),
                            display_name: display_name.clone(),
                            task: task.clone(),
                            section_id: section_id.clone(),
                            status: "running".to_string(),
                            summary: None,
                            thinking: String::new(),
                            task_focus: None,
                            tools: Vec::new(),
                        },
                    );
                }
            }
            SessionEventKind::SubagentFinished {
                subagent_run_id,
                status,
                summary,
            } => {
                let existing = find_subagent_location(&trace.summary, |sub| {
                    sub.subagent_run_id == *subagent_run_id
                });
                if let Some(location) = existing
                    && let Some(subagent) = subagent_mut(&mut trace.summary, location)
                {
                    subagent.status = status.clone();
                    if !summary.is_empty() {
                        subagent.summary = Some(summary.clone());
                    }
                } else {
                    append_execution_subagent(
                        &mut trace.summary,
                        RunTraceSubagent {
                            subagent_run_id: subagent_run_id.clone(),
                            agent_name: event.agent_id.clone(),
                            display_name: None,
                            task: String::new(),
                            section_id: None,
                            status: status.clone(),
                            summary: if summary.is_empty() {
                                None
                            } else {
                                Some(summary.clone())
                            },
                            thinking: String::new(),
                            task_focus: None,
                            tools: Vec::new(),
                        },
                    );
                }
            }
            SessionEventKind::SyncRequired { .. }
            | SessionEventKind::ContextUsage { .. }
            | SessionEventKind::QuestionnaireRequested { .. }
            | SessionEventKind::QuestionnaireAnswered { .. }
            | SessionEventKind::QuestionnaireDismissed { .. }
            | SessionEventKind::DocLinked { .. }
            | SessionEventKind::SessionTitleUpdated { .. }
            | SessionEventKind::UserMessageAdded { .. }
            | SessionEventKind::PendingQueueUpdated { .. } => {}
        }
    }

    traces.into_iter().map(|trace| trace.summary).collect()
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
        let Some(RunTraceSegment::Thinking { text }) = trace.segments.first() else {
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
        assert_eq!(subagent.thinking, "sub-thought");
        assert_eq!(subagent.status, "completed");
        assert_eq!(subagent.summary.as_deref(), Some("intro drafted"));
        assert_eq!(subagent.tools.len(), 1);
        assert_eq!(
            subagent.tools.first().expect("sub tool").duration_ms,
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
            Some(RunTraceSegment::Thinking { text }) if text == "first thought"
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
            Some(RunTraceSegment::Thinking { text }) if text == "second thought"
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
}
