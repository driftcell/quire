use super::*;

impl RuntimeStore {
    pub async fn list_sessions_with_pending_queue(&self) -> RuntimeResult<Vec<String>> {
        let sessions = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT session_id FROM runtime.pending_queue ORDER BY session_id",
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
               FROM runtime.pending_queue
               WHERE session_id = $1
               ORDER BY sequence ASC"#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_pending_queue_item).collect()
    }

    pub async fn enqueue_pending_message(
        &self,
        command: EnqueuePendingMessage<'_>,
    ) -> RuntimeResult<PendingQueueItem> {
        let EnqueuePendingMessage {
            session_id,
            client_msg_id,
            content,
            comment_refs,
            source,
            thread_id,
            attachment_ids,
        } = command;
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
            r#"INSERT INTO runtime.pending_queue
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
               FROM runtime.pending_queue
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
            r#"UPDATE runtime.pending_queue
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
               FROM runtime.pending_queue
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
               FROM runtime.pending_queue
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
            r#"DELETE FROM runtime.pending_queue
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
            r#"DELETE FROM runtime.pending_queue
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
               FROM runtime.pending_queue
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
            r#"DELETE FROM runtime.pending_queue
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
        request: &SendMessageRequest,
        queue_ids: &[String],
    ) -> RuntimeResult<Option<SessionMessage>> {
        if queue_ids.is_empty() {
            return Ok(None);
        }
        let mut tx = self.pool.begin().await?;
        let _status: String = sqlx::query_scalar(
            "SELECT active_run_status FROM runtime.sessions WHERE session_id = $1 FOR UPDATE",
        )
        .bind(session_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| RuntimeError::SessionNotFound(session_id.to_string()))?;
        // Another runtime may have claimed the same batch before this lock was
        // acquired. Re-check ownership instead of emitting a duplicate message.
        let claimable: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM runtime.pending_queue WHERE session_id = $1 AND queue_id = ANY($2)",
        )
        .bind(session_id)
        .bind(queue_ids)
        .fetch_one(&mut *tx)
        .await?;
        if claimable != i64::try_from(queue_ids.len()).unwrap_or(i64::MAX) {
            tx.commit().await?;
            return Ok(None);
        }
        let (message, _replayed) = self
            .memory()
            .await?
            .append_prompt(session_id, request, true)
            .await?;
        self.remove_pending_items_in(&mut tx, session_id, queue_ids)
            .await?;
        let remaining = self.list_pending_queue_in(&mut tx, session_id).await?;
        tx.commit().await?;
        if let Err(error) = self
            .append_event(
                session_id,
                "queue",
                "system",
                None,
                SessionEventKind::PendingQueueUpdated { items: remaining },
            )
            .await
        {
            warn!(%error, %session_id, "failed to append PendingQueueUpdated after claiming batch");
        }
        if let Err(error) = self
            .append_event(
                session_id,
                "queue",
                "queue",
                None,
                SessionEventKind::UserMessageAdded {
                    message_id: message.message_id.clone(),
                },
            )
            .await
        {
            warn!(%error, %session_id, message_id = %message.message_id, "failed to append UserMessageAdded after claiming batch");
        }
        Ok(Some(message))
    }

    /// Atomically claim the next queue batch, persist the user message, and
    /// create + lease the run in one PostgreSQL transaction so concurrent
    /// runtime instances cannot race between claim and `create_run`, and a
    /// transient Redis failure after commit cannot strand the message.
    #[expect(clippy::too_many_arguments, reason = "atomic queue-to-run transition")]
    pub async fn claim_pending_batch_and_create_run(
        &self,
        session_id: &str,
        request: &SendMessageRequest,
        queue_ids: &[String],
        instance_id: &str,
        model: &str,
        lease_ttl_secs: i64,
        allow_awaiting_input: bool,
    ) -> RuntimeResult<Option<(SessionMessage, RunRecord)>> {
        if queue_ids.is_empty() {
            return Ok(None);
        }
        let mut tx = self.pool.begin().await?;
        let status: String = sqlx::query_scalar(
            "SELECT active_run_status FROM runtime.sessions WHERE session_id = $1 FOR UPDATE",
        )
        .bind(session_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| RuntimeError::SessionNotFound(session_id.to_string()))?;
        if status == "running" || (!allow_awaiting_input && status == "awaiting_input") {
            tx.commit().await?;
            return Ok(None);
        }
        let has_running: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM runtime.runs WHERE session_id = $1 AND status = 'running')",
        )
        .bind(session_id)
        .fetch_one(&mut *tx)
        .await?;
        if has_running {
            tx.commit().await?;
            return Ok(None);
        }
        let claimable: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM runtime.pending_queue WHERE session_id = $1 AND queue_id = ANY($2)",
        )
        .bind(session_id)
        .bind(queue_ids)
        .fetch_one(&mut *tx)
        .await?;
        if claimable != i64::try_from(queue_ids.len()).unwrap_or(i64::MAX) {
            tx.commit().await?;
            return Ok(None);
        }
        let (message, _replayed) = self
            .memory()
            .await?
            .append_prompt(session_id, request, true)
            .await?;
        self.remove_pending_items_in(&mut tx, session_id, queue_ids)
            .await?;
        let remaining = self.list_pending_queue_in(&mut tx, session_id).await?;
        let run_id = short_id("run");
        sqlx::query(
            r#"INSERT INTO runtime.runs
               (run_id, session_id, parent_run_id, agent_id, status, model, trigger_message_id, lease_owner, lease_expires_at)
               VALUES ($1, $2, NULL, 'main', 'running', $3, $4, $5, NOW() + ($6 * INTERVAL '1 second'))"#,
        )
        .bind(&run_id)
        .bind(session_id)
        .bind(model)
        .bind(&message.message_id)
        .bind(instance_id)
        .bind(lease_ttl_secs.max(1))
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"UPDATE runtime.sessions SET active_run_status = 'running', updated_at = NOW()
               WHERE session_id = $1"#,
        )
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        if let Err(error) = self
            .append_event(
                session_id,
                "queue",
                "system",
                None,
                SessionEventKind::PendingQueueUpdated { items: remaining },
            )
            .await
        {
            warn!(%error, %session_id, "failed to append PendingQueueUpdated after claiming batch");
        }
        if let Err(error) = self
            .append_event(
                session_id,
                "queue",
                "queue",
                None,
                SessionEventKind::UserMessageAdded {
                    message_id: message.message_id.clone(),
                },
            )
            .await
        {
            warn!(%error, %session_id, message_id = %message.message_id, "failed to append UserMessageAdded after claiming batch");
        }
        let run = self.get_run(&run_id).await?;
        Ok(Some((message, run)))
    }

    pub async fn remove_pending_comment_thread(
        &self,
        session_id: &str,
        thread_id: &str,
    ) -> RuntimeResult<u64> {
        let deleted = sqlx::query(
            r#"DELETE FROM runtime.pending_queue
               WHERE session_id = $1 AND thread_id = $2"#,
        )
        .bind(session_id)
        .bind(thread_id)
        .execute(&self.pool)
        .await?;
        Ok(deleted.rows_affected())
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

    pub(crate) async fn create_question_in(
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
            r#"INSERT INTO runtime.questions
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
               FROM runtime.questions
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
               FROM runtime.questions
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
               FROM runtime.questions
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
            r#"UPDATE runtime.questions
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
            r#"UPDATE runtime.questions
               SET status = 'dismissed', resolved_at = NOW()
               WHERE session_id = $1 AND status = 'pending'
               RETURNING question_id, run_id, tool_call_id, questions, created_at"#,
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_pending_question).collect()
    }

    pub async fn clear_session_awaiting_input(&self, session_id: &str) -> RuntimeResult<()> {
        sqlx::query(
            r#"UPDATE runtime.sessions
               SET active_run_status = 'idle', updated_at = NOW()
               WHERE session_id = $1 AND active_run_status = 'awaiting_input'"#,
        )
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
