use super::*;
use quire_session_protocol::{MessageRole, RunTraceToolItem};

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
                todos: Vec::new(),
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
