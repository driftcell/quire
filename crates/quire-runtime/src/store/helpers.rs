use super::*;

impl RuntimeStore {
    pub async fn kb_version(&self) -> RuntimeResult<u64> {
        let version = sqlx::query_scalar::<_, i64>("SELECT version FROM kb.meta WHERE singleton")
            .fetch_one(&self.pool)
            .await?;
        Ok(u64::try_from(version).unwrap_or(0))
    }

    pub async fn bump_kb_version(&self) -> RuntimeResult<u64> {
        let version = sqlx::query_scalar::<_, i64>(
            "UPDATE kb.meta SET version = version + 1 WHERE singleton RETURNING version",
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

pub(crate) fn row_to_pending_question(row: &PgRow) -> RuntimeResult<PendingQuestion> {
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

pub(crate) fn row_to_resolved_question(row: &PgRow) -> RuntimeResult<ResolvedQuestion> {
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

pub(crate) fn row_to_summary(row: &PgRow) -> RuntimeResult<SessionSummary> {
    let status_raw: String = row.try_get("status")?;
    let run_raw: String = row.try_get("active_run_status")?;
    let usage_raw: Value = row.try_get("context_usage")?;
    Ok(SessionSummary {
        session_id: row.try_get("session_id")?,
        user_id: row.try_get("user_id").ok().flatten(),
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
        last_event_seq: 0,
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

pub(crate) fn row_to_pending_queue_item(row: &PgRow) -> RuntimeResult<PendingQueueItem> {
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
pub(crate) fn select_active_trace(
    traces: Vec<RunTraceSummary>,
    active_run_id: &str,
) -> Option<RunTraceSummary> {
    traces
        .into_iter()
        .find(|trace| trace.run_id == active_run_id && trace.status == "running")
}

pub(crate) fn active_trace_snapshot(
    summary: &SessionSummary,
    active_run_id: Option<&str>,
    traces: Vec<RunTraceSummary>,
) -> (Option<RunTraceSummary>, Vec<RunTraceSubagent>) {
    if summary.active_run_status != ActiveRunStatus::Running {
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

pub(crate) fn row_to_run(row: &PgRow) -> RuntimeResult<RunRecord> {
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

pub(crate) fn event_to_frame(event: SessionEvent) -> SessionEventFrame {
    SessionEventFrame {
        seq: event.seq,
        event_type: event.event_name().to_string(),
        event: serde_json::to_value(event).unwrap_or(Value::Null),
    }
}

/// Projection tolerates event variants written by a newer runtime. Transport
/// still relays them verbatim; trace aggregation simply ignores what it cannot
/// classify instead of failing the whole session detail request.
pub(crate) fn decode_projection_events(frames: Vec<SessionEventFrame>) -> Vec<SessionEvent> {
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
