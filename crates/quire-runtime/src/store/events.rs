use super::*;

impl RuntimeStore {
    pub async fn append_event(
        &self,
        session_id: &str,
        run_id: &str,
        agent_id: &str,
        parent_run_id: Option<&str>,
        kind: SessionEventKind,
    ) -> RuntimeResult<SessionEvent> {
        self.bus
            .append_session_event(
                session_id,
                run_id,
                agent_id,
                parent_run_id.map(str::to_string),
                kind,
            )
            .await
    }

    /// Append a coalesced batch to the ordered Redis hot projection.
    pub async fn append_event_batch(
        &self,
        session_id: &str,
        drafts: Vec<SessionEventDraft>,
    ) -> RuntimeResult<Vec<SessionEvent>> {
        let mut events = Vec::with_capacity(drafts.len());
        for draft in drafts {
            events.push(
                self.bus
                    .append_session_event(
                        session_id,
                        &draft.run_id,
                        &draft.agent_id,
                        draft.parent_run_id,
                        draft.kind,
                    )
                    .await?,
            );
        }
        Ok(events)
    }

    /// Persist a tool row before publishing its display event.
    pub async fn save_tool_call_with_event(
        &self,
        detail: &ToolCallDetail,
        agent_id: &str,
        parent_run_id: Option<&str>,
        kind: SessionEventKind,
    ) -> RuntimeResult<SessionEvent> {
        let mut tx = self.pool.begin().await?;
        self.save_tool_call_in(&mut tx, detail).await?;
        tx.commit().await?;
        self.append_event(
            &detail.session_id,
            &detail.run_id,
            agent_id,
            parent_run_id,
            kind,
        )
        .await
    }

    /// Create an `ask_user` question before publishing its request.
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
        tx.commit().await?;
        let kind = SessionEventKind::QuestionnaireRequested {
            question_id: question.question_id.clone(),
            tool_call_id: question.tool_call_id.clone(),
            questions: question.questions.clone(),
        };
        let event = self
            .append_event(session_id, run_id, agent_id, parent_run_id, kind)
            .await?;
        Ok((question, event))
    }

    /// Transition the run in PostgreSQL, then append its terminal hot event and
    /// materialize the complete cold trace projection.
    pub async fn finish_run_with_event(
        &self,
        run_id: &str,
        status: &str,
        error: Option<&str>,
        kind: SessionEventKind,
    ) -> RuntimeResult<Option<SessionEvent>> {
        let session_status = match status {
            "completed" => "idle",
            "cancelled" | "interrupted" => "interrupted",
            "failed" => "failed",
            _ => "idle",
        };
        self.finish_run_inner(run_id, status, session_status, error, kind)
            .await
    }

    /// Close a run cleanly as `completed` while keeping the session input gate in `awaiting_input`.
    pub async fn finish_run_for_question(
        &self,
        run_id: &str,
    ) -> RuntimeResult<Option<SessionEvent>> {
        self.finish_run_inner(
            run_id,
            "completed",
            "awaiting_input",
            None,
            SessionEventKind::TurnFinished,
        )
        .await
    }

    async fn finish_run_inner(
        &self,
        run_id: &str,
        status: &str,
        session_status: &str,
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
        sqlx::query(
            "UPDATE agent_sessions SET active_run_status = $2, updated_at = NOW() WHERE session_id = $1",
        )
        .bind(&session_id)
        .bind(session_status)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        let event = self
            .append_event(
                &session_id,
                run_id,
                &agent_id,
                parent_run_id.as_deref(),
                kind,
            )
            .await?;
        let events = self.bus.hot_events(&session_id).await?;
        let terminal_seq = event.seq;
        let start = start_seq
            .and_then(|seq| u64::try_from(seq).ok())
            .unwrap_or(0);
        let events: Vec<SessionEvent> = events
            .into_iter()
            .filter(|event| event.seq >= start && event.seq <= terminal_seq)
            .collect();
        if let Some(trace) = aggregate_run_traces(&events)
            .into_iter()
            .find(|trace| trace.run_id == run_id)
        {
            sqlx::query("UPDATE agent_runs SET trace_summary = $2 WHERE run_id = $1")
                .bind(run_id)
                .bind(serde_json::to_value(trace)?)
                .execute(&self.pool)
                .await?;
        }
        Ok(Some(event))
    }

    pub async fn list_events(
        &self,
        session_id: &str,
        after_seq: u64,
        limit: i64,
    ) -> RuntimeResult<Vec<SessionEvent>> {
        Ok(self
            .bus
            .hot_events(session_id)
            .await?
            .into_iter()
            .filter(|event| event.seq > after_seq)
            .take(usize::try_from(limit.clamp(1, 10_000)).unwrap_or(10_000))
            .collect())
    }

    /// Read materialized completed traces and aggregate only the active run.
    pub async fn min_available_seq(&self, session_id: &str) -> RuntimeResult<u64> {
        let events = self.list_event_frames(session_id, 0, 100_000).await?;
        if let Some(first) = events.first() {
            Ok(first.seq)
        } else {
            let last = self.bus.current_event_seq(session_id).await?;
            Ok(if last > 0 { last.saturating_add(1) } else { 0 })
        }
    }

    /// Materialize completed traces, then trim only the Redis hot projection.
    pub async fn prune_session_events(
        &self,
        session_id: &str,
        keep_events: usize,
    ) -> RuntimeResult<(u64, u64)> {
        let keep = keep_events.clamp(1, 100_000);
        self.materialize_completed_run_traces(session_id).await?;
        let events = self.bus.hot_events(session_id).await?;
        let last_seq = self.bus.current_event_seq(session_id).await?;
        let cutoff = if events.len() > keep {
            events
                .get(events.len().saturating_sub(keep))
                .map(|event| event.seq)
        } else {
            None
        };
        let active_start: Option<Option<i64>> = sqlx::query_scalar(
            r#"SELECT start_seq FROM agent_runs
               WHERE session_id = $1 AND status = 'running'
               ORDER BY started_at DESC LIMIT 1"#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        let mut min_available = cutoff.unwrap_or(last_seq);
        match active_start {
            Some(Some(start)) => {
                min_available = min_available.min(u64::try_from(start).unwrap_or(0));
            }
            Some(None) => {
                min_available = min_available.min(last_seq);
            }
            None => {}
        }
        let deleted = self.bus.trim_hot_events(session_id, min_available).await?;
        Ok((deleted, min_available))
    }

    pub async fn count_events_after(&self, session_id: &str, after_seq: u64) -> RuntimeResult<i64> {
        Ok(i64::try_from(
            self.list_event_frames(session_id, after_seq, 100_000)
                .await?
                .len(),
        )
        .unwrap_or(i64::MAX))
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
}
