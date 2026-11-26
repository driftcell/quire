mod acl;

use super::*;

impl RuntimeStore {
    pub async fn create_session(
        &self,
        title: &str,
        doc_id: Option<&str>,
    ) -> RuntimeResult<SessionSummary> {
        let branch: String = sqlx::query_scalar(
            "SELECT name FROM kb.branches WHERE status = 'active' ORDER BY name ASC LIMIT 1",
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
        self.create_session_for_user(None, title, doc_id, kb_branch)
            .await
    }

    pub async fn create_session_for_user(
        &self,
        user_id: Option<&str>,
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
        let doc_id = doc_id.map(str::trim).filter(|s| !s.is_empty());
        let session_id = short_id("sess");
        sqlx::query(
            r#"INSERT INTO runtime.sessions
               (session_id, user_id, title, doc_id, linked_doc_id, kb_branch, status, active_run_status)
               VALUES ($1, $2, $3, $4, $4, $5, 'active', 'idle')"#,
        )
        .bind(&session_id)
        .bind(user_id)
        .bind(title)
        .bind(doc_id)
        .bind(kb_branch)
        .execute(&self.pool)
        .await?;
        self.get_session_summary(&session_id).await
    }

    pub async fn get_session_owner(&self, session_id: &str) -> RuntimeResult<Option<String>> {
        let owner: Option<Option<String>> =
            sqlx::query_scalar("SELECT user_id FROM runtime.sessions WHERE session_id = $1")
                .bind(session_id)
                .fetch_optional(&self.pool)
                .await?;
        owner.ok_or_else(|| RuntimeError::SessionNotFound(session_id.to_string()))
    }

    pub async fn get_session_summary(&self, session_id: &str) -> RuntimeResult<SessionSummary> {
        let row = sqlx::query(
            r#"SELECT session_id, user_id, title, doc_id, linked_doc_id, kb_branch, status,
                      active_run_status, context_usage, compaction_started_at,
                      compacted_at, pinned_at, created_via, created_at, updated_at
               FROM runtime.sessions WHERE session_id = $1"#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| RuntimeError::SessionNotFound(session_id.to_string()))?;
        let mut summary = row_to_summary(&row)?;
        summary.last_event_seq = self.bus.current_event_seq(session_id).await?;
        Ok(summary)
    }

    pub async fn get_session_detail(&self, session_id: &str) -> RuntimeResult<SessionDetail> {
        let summary = self.get_session_summary(session_id).await?;
        let active_run = self.latest_running_run(session_id).await?;
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

    pub(crate) async fn events_through(
        &self,
        session_id: &str,
        max_seq: u64,
    ) -> RuntimeResult<Vec<SessionEvent>> {
        let frames = self.event_frames_between(session_id, 0, max_seq).await?;
        Ok(decode_projection_events(frames))
    }

    /// Read one active run's events through the `(session_id, seq)` primary key.
    pub(crate) async fn events_between(
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

    pub(crate) async fn event_frames_between(
        &self,
        session_id: &str,
        start_seq: u64,
        end_seq: u64,
    ) -> RuntimeResult<Vec<SessionEventFrame>> {
        Ok(self
            .bus
            .hot_events(session_id)
            .await?
            .into_iter()
            .filter(|event| event.seq >= start_seq && event.seq <= end_seq)
            .map(event_to_frame)
            .collect())
    }

    pub(crate) async fn event_frames_between_in(
        &self,
        _tx: &mut Transaction<'_, Postgres>,
        session_id: &str,
        start_seq: u64,
        end_seq: u64,
    ) -> RuntimeResult<Vec<SessionEventFrame>> {
        self.event_frames_between(session_id, start_seq, end_seq)
            .await
    }

    /// Aggregate and store one run's trace inside a lifecycle transaction.
    pub async fn list_event_frames(
        &self,
        session_id: &str,
        after_seq: u64,
        limit: i64,
    ) -> RuntimeResult<Vec<SessionEventFrame>> {
        Ok(self
            .bus
            .hot_event_payloads(session_id)
            .await?
            .into_iter()
            .filter_map(|payload| serde_json::from_str::<Value>(&payload).ok())
            .filter(|event| {
                event
                    .get("seq")
                    .and_then(Value::as_u64)
                    .is_some_and(|seq| seq > after_seq)
            })
            .take(usize::try_from(limit.clamp(1, 10_000)).unwrap_or(10_000))
            .filter_map(|event| {
                Some(SessionEventFrame {
                    seq: event.get("seq")?.as_u64()?,
                    event_type: event.get("event")?.as_str()?.to_string(),
                    event,
                })
            })
            .collect())
    }

    pub async fn list_sessions(
        &self,
        limit: i64,
        offset: i64,
    ) -> RuntimeResult<Vec<SessionSummary>> {
        self.list_sessions_for_user(None, true, limit, offset).await
    }

    pub async fn list_sessions_for_user(
        &self,
        user_id: Option<&str>,
        include_unowned: bool,
        limit: i64,
        offset: i64,
    ) -> RuntimeResult<Vec<SessionSummary>> {
        let rows =
            match user_id {
                Some(uid) if include_unowned => sqlx::query(
                    r#"SELECT session_id, user_id, title, doc_id, linked_doc_id, kb_branch, status,
                              active_run_status, context_usage, compaction_started_at,
                              compacted_at, pinned_at, created_via, created_at, updated_at
                       FROM runtime.sessions
                       WHERE user_id = $1 OR user_id IS NULL
                       ORDER BY (pinned_at IS NOT NULL) DESC,
                                pinned_at DESC NULLS LAST,
                                updated_at DESC,
                                session_id ASC
                       LIMIT $2 OFFSET $3"#,
                )
                .bind(uid)
                .bind(limit.clamp(1, 200))
                .bind(offset.max(0))
                .fetch_all(&self.pool)
                .await?,
                Some(uid) => sqlx::query(
                    r#"SELECT session_id, user_id, title, doc_id, linked_doc_id, kb_branch, status,
                              active_run_status, context_usage, compaction_started_at,
                              compacted_at, pinned_at, created_via, created_at, updated_at
                       FROM runtime.sessions
                       WHERE user_id = $1
                       ORDER BY (pinned_at IS NOT NULL) DESC,
                                pinned_at DESC NULLS LAST,
                                updated_at DESC,
                                session_id ASC
                       LIMIT $2 OFFSET $3"#,
                )
                .bind(uid)
                .bind(limit.clamp(1, 200))
                .bind(offset.max(0))
                .fetch_all(&self.pool)
                .await?,
                None => sqlx::query(
                    r#"SELECT session_id, user_id, title, doc_id, linked_doc_id, kb_branch, status,
                              active_run_status, context_usage, compaction_started_at,
                              compacted_at, pinned_at, created_via, created_at, updated_at
                       FROM runtime.sessions
                       ORDER BY (pinned_at IS NOT NULL) DESC,
                                pinned_at DESC NULLS LAST,
                                updated_at DESC,
                                session_id ASC
                       LIMIT $1 OFFSET $2"#,
                )
                .bind(limit.clamp(1, 200))
                .bind(offset.max(0))
                .fetch_all(&self.pool)
                .await?,
            };
        let mut sessions = Vec::with_capacity(rows.len());
        for row in &rows {
            sessions.push(row_to_summary(row)?);
        }
        let session_ids: Vec<String> = sessions
            .iter()
            .map(|summary| summary.session_id.clone())
            .collect();
        let seqs = self.bus.current_event_seqs(&session_ids).await?;
        for (summary, seq) in sessions.iter_mut().zip(seqs) {
            summary.last_event_seq = seq;
        }
        Ok(sessions)
    }

    pub async fn update_session(
        &self,
        session_id: &str,
        patch: SessionPatch<'_>,
    ) -> RuntimeResult<SessionSummary> {
        let updated = sqlx::query(
            r#"UPDATE runtime.sessions
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
        let memory = self.memory().await?;
        memory.delete_session(session_id).await?;
        let deleted = sqlx::query("DELETE FROM runtime.sessions WHERE session_id = $1")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        if deleted.rows_affected() == 0 {
            return Err(RuntimeError::SessionNotFound(session_id.to_string()));
        }
        if let Err(error) = self.bus.delete_session_hot_state(session_id).await {
            warn!(%error, session_id, "failed to delete Redis hot session state");
        }
        Ok(())
    }

    pub async fn bind_doc(&self, session_id: &str, doc_id: &str) -> RuntimeResult<()> {
        let updated = sqlx::query(
            r#"UPDATE runtime.sessions
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
        sqlx::query(
            r#"UPDATE report.documents d
               SET owner_user_id = COALESCE(d.owner_user_id, s.user_id),
                   session_id = COALESCE(d.session_id, $1)
               FROM runtime.sessions s
               WHERE d.doc_id = $2
                 AND s.session_id = $1"#,
        )
        .bind(session_id)
        .bind(doc_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_pinned_prompt(
        &self,
        session_id: &str,
        prompt: &str,
        kb_version: u64,
    ) -> RuntimeResult<()> {
        let updated = sqlx::query(
            r#"UPDATE runtime.sessions
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
            r#"UPDATE runtime.sessions
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
            "UPDATE runtime.sessions SET compaction_started_at = NULL, updated_at = NOW() WHERE session_id = $1",
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
            r#"UPDATE runtime.sessions
               SET compaction_started_at = NULL, updated_at = NOW()
               WHERE session_id = $1 AND compaction_started_at = $2"#,
        )
        .bind(session_id)
        .bind(started_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Apply canonical rig-memory compaction, then update run-independent session metadata.
    pub async fn apply_compaction(&self, write: &CompactionWrite<'_>) -> RuntimeResult<()> {
        let mut tx = self.pool.begin().await?;
        let status: Option<String> = sqlx::query_scalar(
            "SELECT active_run_status FROM runtime.sessions WHERE session_id = $1 FOR UPDATE",
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
        self.memory()
            .await?
            .apply_compaction(write.session_id, write.summary, write.exclude_message_id)
            .await?;
        let usage = serde_json::to_value(write.context_usage)?;
        sqlx::query(
            r#"UPDATE runtime.sessions
               SET compacted_at = NOW(),
                   compaction_started_at = NULL,
                   pinned_system_prompt = $2,
                   synced_kb_version = $3,
                   context_usage = $4,
                   updated_at = NOW()
               WHERE session_id = $1"#,
        )
        .bind(write.session_id)
        .bind(write.pinned_system_prompt)
        .bind(i64::try_from(write.kb_version).unwrap_or(i64::MAX))
        .bind(usage)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn pinned_prompt(&self, session_id: &str) -> RuntimeResult<(Option<String>, u64)> {
        let row = sqlx::query(
            "SELECT pinned_system_prompt, synced_kb_version FROM runtime.sessions WHERE session_id = $1",
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
            "UPDATE runtime.sessions SET synced_kb_version = $2, updated_at = NOW() WHERE session_id = $1",
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
        validate_user_payload(
            &request.content,
            &request.comment_refs,
            &request.attachment_ids,
        )?;
        let (message, replayed) = self
            .memory()
            .await?
            .append_prompt(session_id, request, true)
            .await?;
        Ok((message, !replayed))
    }

    pub async fn find_message_by_client_id(
        &self,
        session_id: &str,
        client_msg_id: &str,
    ) -> RuntimeResult<Option<SessionMessage>> {
        self.memory()
            .await?
            .find_by_client_id(session_id, client_msg_id)
            .await
    }

    pub async fn insert_assistant_message(
        &self,
        session_id: &str,
        content: &str,
        run_id: &str,
    ) -> RuntimeResult<SessionMessage> {
        self.memory()
            .await?
            .append_assistant_if_missing(session_id, run_id, content)
            .await
    }

    pub async fn insert_question_message(
        &self,
        session_id: &str,
        run_id: &str,
        question: &PendingQuestion,
    ) -> RuntimeResult<SessionMessage> {
        self.memory()
            .await?
            .append_question_message(session_id, run_id, question)
            .await
    }

    pub async fn list_messages(&self, session_id: &str) -> RuntimeResult<Vec<SessionMessage>> {
        self.memory().await?.list_messages(session_id).await
    }
}
