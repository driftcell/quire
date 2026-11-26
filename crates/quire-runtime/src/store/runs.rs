use super::*;

#[derive(Debug, Clone)]
pub struct RecoverableRun {
    pub run_id: String,
    pub session_id: String,
}

impl RuntimeStore {
    pub(crate) async fn materialize_run_trace_in(
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
        sqlx::query("UPDATE runtime.runs SET trace_summary = $2 WHERE run_id = $1")
            .bind(run_id)
            .bind(value)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    /// Lazy backfill for a completed run whose trace has not been materialized.
    pub async fn materialize_run_trace(&self, run_id: &str) -> RuntimeResult<()> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query("SELECT session_id, start_seq FROM runtime.runs WHERE run_id = $1")
            .bind(run_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| RuntimeError::RunNotFound(run_id.to_string()))?;
        let session_id: String = row.try_get("session_id")?;
        let start_seq: Option<i64> = row.try_get("start_seq")?;
        let end_seq = self.bus.current_event_seq(&session_id).await?;
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
        let end_seq = self.bus.current_event_seq(session_id).await?;
        let rows = sqlx::query(
            r#"SELECT run_id, start_seq FROM runtime.runs
               WHERE session_id = $1 AND parent_run_id IS NULL
                 AND status <> 'running' AND trace_summary IS NULL"#,
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
    pub async fn create_run(
        &self,
        session_id: &str,
        parent_run_id: Option<&str>,
        agent_id: &str,
        model: &str,
    ) -> RuntimeResult<RunRecord> {
        self.create_run_for_message(session_id, parent_run_id, agent_id, model, None)
            .await
    }

    pub async fn create_run_for_message(
        &self,
        session_id: &str,
        parent_run_id: Option<&str>,
        agent_id: &str,
        model: &str,
        trigger_message_id: Option<&str>,
    ) -> RuntimeResult<RunRecord> {
        let mut tx = self.pool.begin().await?;
        if parent_run_id.is_none() {
            let status: Option<String> = sqlx::query_scalar(
                "SELECT active_run_status FROM runtime.sessions WHERE session_id = $1 FOR UPDATE",
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
            r#"INSERT INTO runtime.runs
               (run_id, session_id, parent_run_id, agent_id, status, model, trigger_message_id)
               VALUES ($1, $2, $3, $4, 'running', $5, $6)"#,
        )
        .bind(&run_id)
        .bind(session_id)
        .bind(parent_run_id)
        .bind(agent_id)
        .bind(model)
        .bind(trigger_message_id)
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
        self.get_run(&run_id).await
    }

    pub async fn has_run_for_message(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> RuntimeResult<bool> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM runtime.runs WHERE session_id = $1 AND trigger_message_id = $2)",
        )
        .bind(session_id)
        .bind(message_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    /// Append `turn_started` and retain its hot sequence for trace materialization.
    pub async fn start_run_with_event(
        &self,
        run_id: &str,
        kind: SessionEventKind,
    ) -> RuntimeResult<SessionEvent> {
        let row = sqlx::query(
            "SELECT session_id, agent_id, parent_run_id FROM runtime.runs WHERE run_id = $1",
        )
        .bind(run_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| RuntimeError::RunNotFound(run_id.to_string()))?;
        let session_id: String = row.try_get("session_id")?;
        let agent_id: String = row.try_get("agent_id")?;
        let parent_run_id: Option<String> = row.try_get("parent_run_id")?;
        let event = self
            .append_event(
                &session_id,
                run_id,
                &agent_id,
                parent_run_id.as_deref(),
                kind,
            )
            .await?;
        sqlx::query(
            "UPDATE runtime.runs SET start_seq = $2 WHERE run_id = $1 AND start_seq IS NULL",
        )
        .bind(run_id)
        .bind(i64::try_from(event.seq).unwrap_or(i64::MAX))
        .execute(&self.pool)
        .await?;
        Ok(event)
    }

    pub async fn get_run(&self, run_id: &str) -> RuntimeResult<RunRecord> {
        let row = sqlx::query(
            r#"SELECT run_id, session_id, parent_run_id, agent_id, status, model, error,
                      started_at, finished_at, start_seq
               FROM runtime.runs WHERE run_id = $1"#,
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
               FROM runtime.runs
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
                      started_at, finished_at, start_seq
               FROM runtime.runs
               WHERE session_id = $1
               ORDER BY started_at DESC LIMIT 1"#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| row_to_run(&row)).transpose()
    }

    /// Mark orphaned subagent runs as interrupted and return surviving main-agent runs
    /// whose lease is unowned or expired so `boot()` can resume them without stealing active runs.
    pub async fn list_recoverable_running_runs(&self) -> RuntimeResult<Vec<RecoverableRun>> {
        self.claim_recoverable_running_runs(None, 15).await
    }

    pub async fn claim_recoverable_running_runs(
        &self,
        instance_id: Option<&str>,
        lease_ttl_secs: i64,
    ) -> RuntimeResult<Vec<RecoverableRun>> {
        let orphaned_subs = sqlx::query(
            r#"SELECT r.run_id
               FROM runtime.runs r
               WHERE r.status = 'running'
                 AND r.parent_run_id IS NOT NULL
                 AND (r.lease_expires_at IS NULL
                      OR r.lease_expires_at <= NOW()
                      OR ($1::text IS NOT NULL AND r.lease_owner = $1))
                 AND NOT EXISTS (
                     SELECT 1 FROM runtime.runs p
                     WHERE p.run_id = r.parent_run_id
                       AND p.status = 'running'
                       AND p.lease_expires_at > NOW()
                       AND ($1::text IS NULL OR p.lease_owner IS DISTINCT FROM $1)
                 )"#,
        )
        .bind(instance_id)
        .fetch_all(&self.pool)
        .await?;
        for row in &orphaned_subs {
            let run_id: String = row.try_get("run_id")?;
            let _ = self
                .finish_run_with_event(
                    &run_id,
                    "interrupted",
                    Some("runtime restarted while subagent was active"),
                    SessionEventKind::TurnFailed {
                        error: "runtime restarted while subagent was active".into(),
                        recoverable: true,
                    },
                )
                .await;
        }

        let rows = sqlx::query(
            r#"UPDATE runtime.runs
               SET lease_owner = COALESCE($1, lease_owner),
                   lease_expires_at = CASE
                       WHEN $1 IS NOT NULL THEN NOW() + ($2 * INTERVAL '1 second')
                       ELSE lease_expires_at
                   END
               WHERE run_id IN (
                   SELECT run_id
                   FROM runtime.runs
                   WHERE status = 'running'
                     AND parent_run_id IS NULL
                     AND (lease_expires_at IS NULL
                          OR lease_expires_at <= NOW()
                          OR ($1::text IS NOT NULL AND lease_owner = $1))
                   ORDER BY started_at ASC
               )
               RETURNING run_id, session_id"#,
        )
        .bind(instance_id)
        .bind(lease_ttl_secs.max(1))
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            out.push(RecoverableRun {
                run_id: row.try_get("run_id")?,
                session_id: row.try_get("session_id")?,
            });
        }
        Ok(out)
    }

    pub async fn acquire_run_lease(
        &self,
        run_id: &str,
        instance_id: &str,
        lease_ttl_secs: i64,
    ) -> RuntimeResult<bool> {
        let updated = sqlx::query(
            r#"UPDATE runtime.runs
               SET lease_owner = $2,
                   lease_expires_at = NOW() + ($3 * INTERVAL '1 second')
               WHERE run_id = $1
                 AND status = 'running'
                 AND (lease_owner IS NULL OR lease_owner = $2 OR lease_expires_at IS NULL OR lease_expires_at <= NOW())"#,
        )
        .bind(run_id)
        .bind(instance_id)
        .bind(lease_ttl_secs.max(1))
        .execute(&self.pool)
        .await?;
        Ok(updated.rows_affected() > 0)
    }

    pub async fn renew_run_lease(
        &self,
        run_id: &str,
        instance_id: &str,
        lease_ttl_secs: i64,
    ) -> RuntimeResult<bool> {
        let updated = sqlx::query(
            r#"UPDATE runtime.runs
               SET lease_expires_at = NOW() + ($3 * INTERVAL '1 second')
               WHERE run_id = $1 AND status = 'running' AND lease_owner = $2"#,
        )
        .bind(run_id)
        .bind(instance_id)
        .bind(lease_ttl_secs.max(1))
        .execute(&self.pool)
        .await?;
        Ok(updated.rows_affected() > 0)
    }

    pub async fn list_running_run_ids(&self, session_id: &str) -> RuntimeResult<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT run_id FROM runtime.runs WHERE session_id = $1 AND status = 'running'",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Mark every surviving `running` run as interrupted after a runtime restart.
    pub async fn recover_running_runs(&self) -> RuntimeResult<usize> {
        let rows = sqlx::query(
            r#"SELECT run_id, session_id, parent_run_id, agent_id, status, model, error,
                      started_at, finished_at, start_seq
               FROM runtime.runs WHERE status = 'running'"#,
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
            "UPDATE runtime.sessions SET context_usage = $2, updated_at = NOW() WHERE session_id = $1",
        )
        .bind(session_id)
        .bind(serde_json::to_value(usage)?)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_run_traces(&self, session_id: &str) -> RuntimeResult<Vec<RunTraceSummary>> {
        let summary = self.get_session_summary(session_id).await?;
        let rows = sqlx::query(
            r#"SELECT run_id, status, start_seq, trace_summary FROM runtime.runs
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
            if status != "running" {
                // Lazy backfill for runs completed before trace materialization.
                self.materialize_run_trace(&run_id).await?;
            }
            traces.push(trace);
        }

        let hot_events = self.bus.hot_events(session_id).await?;
        for trace in aggregate_run_traces(&hot_events) {
            if !traces
                .iter()
                .any(|existing| existing.run_id == trace.run_id)
            {
                traces.push(trace);
            }
        }
        Ok(traces)
    }
}
