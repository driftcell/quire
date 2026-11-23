use super::*;

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
