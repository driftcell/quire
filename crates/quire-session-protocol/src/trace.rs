//! Pure projection of ordered session events into run traces.
//!
//! This is the canonical Rust projection used by runtime snapshots and by the
//! shared frontend/backend golden fixtures. It performs no I/O.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::{
    RunTaskFocus, RunTraceSegment, RunTraceSubagent, RunTraceSummary, RunTraceToolItem,
    SessionEvent, SessionEventKind, TaskTodoItem, TaskTodoStatus,
};

struct RunTraceAccumulator {
    summary: RunTraceSummary,
    started_at: Option<DateTime<Utc>>,
    active_main_thinking_start: Option<DateTime<Utc>>,
    active_sub_thinking_start: HashMap<String, DateTime<Utc>>,
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
            started_at: Some(timestamp),
            duration_ms: None,
            task_focus: None,
            segments: Vec::new(),
        },
        started_at: Some(timestamp),
        active_main_thinking_start: None,
        active_sub_thinking_start: HashMap::new(),
    });
    index.insert(run_id.to_string(), position);
    position
}

fn is_subagent_event(event: &SessionEvent) -> bool {
    event.parent_run_id.is_some() || event.agent_id != "main"
}

fn append_thinking_segment(
    segments: &mut Vec<RunTraceSegment>,
    delta: &str,
    started_at: DateTime<Utc>,
) {
    if let Some(RunTraceSegment::Thinking {
        text,
        started_at: current,
        ..
    }) = segments.last_mut()
    {
        text.push_str(delta);
        if current.is_none() {
            *current = Some(started_at);
        }
        return;
    }
    segments.push(RunTraceSegment::Thinking {
        text: delta.to_string(),
        duration_ms: None,
        started_at: Some(started_at),
    });
}

fn append_text_segment(segments: &mut Vec<RunTraceSegment>, delta: &str) {
    if let Some(RunTraceSegment::Text { text }) = segments.last_mut() {
        text.push_str(delta);
        return;
    }
    segments.push(RunTraceSegment::Text {
        text: delta.to_string(),
    });
}

fn append_execution_tool(segments: &mut Vec<RunTraceSegment>, item: RunTraceToolItem) {
    if let Some(RunTraceSegment::Execution { tools, .. }) = segments.last_mut() {
        tools.push(item);
        return;
    }
    segments.push(RunTraceSegment::Execution {
        tools: vec![item],
        subagents: Vec::new(),
    });
}

fn append_execution_subagent(
    segments: &mut Vec<RunTraceSegment>,
    subagent: RunTraceSubagent,
) -> Option<(usize, usize)> {
    if let Some(segment_index) = segments.len().checked_sub(1)
        && let Some(RunTraceSegment::Execution { subagents, .. }) = segments.get_mut(segment_index)
    {
        subagents.push(subagent);
        return Some((segment_index, subagents.len().saturating_sub(1)));
    }
    segments.push(RunTraceSegment::Execution {
        tools: Vec::new(),
        subagents: vec![subagent],
    });
    Some((segments.len().saturating_sub(1), 0))
}

fn find_tool_location(segments: &[RunTraceSegment], tool_call_id: &str) -> Option<(usize, usize)> {
    for (segment_index, segment) in segments.iter().enumerate().rev() {
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
    segments: &mut [RunTraceSegment],
    tool_call_id: &str,
    status: &str,
    output_preview: &str,
    truncated: bool,
    duration_ms: u64,
) {
    let Some((segment_index, tool_index)) = find_tool_location(segments, tool_call_id) else {
        return;
    };
    let Some(RunTraceSegment::Execution { tools, .. }) = segments.get_mut(segment_index) else {
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

fn find_subagent_location<F>(segments: &[RunTraceSegment], matches: F) -> Option<(usize, usize)>
where
    F: Fn(&RunTraceSubagent) -> bool,
{
    for (segment_index, segment) in segments.iter().enumerate().rev() {
        if let RunTraceSegment::Execution { subagents, .. } = segment
            && let Some(subagent_index) = subagents.iter().rposition(&matches)
        {
            return Some((segment_index, subagent_index));
        }
    }
    None
}

fn remove_subagent_by_id(
    segments: &mut Vec<RunTraceSegment>,
    subagent_run_id: &str,
) -> Option<RunTraceSubagent> {
    let (segment_index, subagent_index) =
        find_subagent_location(segments, |sub| sub.subagent_run_id == subagent_run_id)?;
    let Some(RunTraceSegment::Execution { tools, subagents }) = segments.get_mut(segment_index)
    else {
        return None;
    };
    let removed = subagents.remove(subagent_index);
    if tools.is_empty() && subagents.is_empty() {
        segments.remove(segment_index);
    }
    Some(removed)
}

fn subagent_mut(
    segments: &mut [RunTraceSegment],
    (segment_index, subagent_index): (usize, usize),
) -> Option<&mut RunTraceSubagent> {
    let Some(RunTraceSegment::Execution { subagents, .. }) = segments.get_mut(segment_index) else {
        return None;
    };
    subagents.get_mut(subagent_index)
}

fn ensure_subagent(
    segments: &mut Vec<RunTraceSegment>,
    event: &SessionEvent,
) -> Option<(usize, usize)> {
    if let Some(location) =
        find_subagent_location(segments, |sub| sub.subagent_run_id == event.run_id)
    {
        return Some(location);
    }
    append_execution_subagent(
        segments,
        RunTraceSubagent {
            subagent_run_id: event.run_id.clone(),
            agent_name: event.agent_id.clone(),
            display_name: None,
            task: String::new(),
            section_id: None,
            status: "running".to_string(),
            started_at: Some(event.timestamp),
            summary: None,
            segments: Vec::new(),
            task_focus: None,
        },
    )
}

fn elapsed_ms(started_at: Option<DateTime<Utc>>, finished_at: DateTime<Utc>) -> Option<u64> {
    started_at.and_then(|start| u64::try_from((finished_at - start).num_milliseconds().max(0)).ok())
}

impl RunTraceAccumulator {
    fn settle_main_thinking(&mut self, at: DateTime<Utc>) {
        let Some(started) = self.active_main_thinking_start.take() else {
            return;
        };
        if let Some(RunTraceSegment::Thinking { duration_ms, .. }) =
            self.summary.segments.last_mut()
        {
            *duration_ms = elapsed_ms(Some(started), at);
        }
    }

    fn settle_sub_thinking(&mut self, run_id: &str, at: DateTime<Utc>) {
        let Some(started) = self.active_sub_thinking_start.remove(run_id) else {
            return;
        };
        let Some(location) =
            find_subagent_location(&self.summary.segments, |sub| sub.subagent_run_id == run_id)
        else {
            return;
        };
        let Some(record) = subagent_mut(&mut self.summary.segments, location) else {
            return;
        };
        if let Some(RunTraceSegment::Thinking { duration_ms, .. }) = record.segments.last_mut() {
            *duration_ms = elapsed_ms(Some(started), at);
        }
    }

    fn finalize_active_clocks(&mut self) {
        if let Some(started) = self.active_main_thinking_start.take()
            && let Some(RunTraceSegment::Thinking { started_at, .. }) =
                self.summary.segments.last_mut()
        {
            *started_at = Some(started);
        }
        let active_sub_starts = std::mem::take(&mut self.active_sub_thinking_start);
        for (run_id, started) in active_sub_starts {
            let Some(location) =
                find_subagent_location(&self.summary.segments, |sub| sub.subagent_run_id == run_id)
            else {
                continue;
            };
            let Some(record) = subagent_mut(&mut self.summary.segments, location) else {
                continue;
            };
            if let Some(RunTraceSegment::Thinking { started_at, .. }) = record.segments.last_mut() {
                *started_at = Some(started);
            }
        }
    }
}

fn settles_main_thinking(kind: &SessionEventKind) -> bool {
    matches!(
        kind,
        SessionEventKind::TextDelta { .. }
            | SessionEventKind::ToolCallStarted { .. }
            | SessionEventKind::SubagentStarted { .. }
            | SessionEventKind::QuestionnaireRequested { .. }
            | SessionEventKind::TurnFinished
            | SessionEventKind::TurnSuperseded
            | SessionEventKind::TurnFailed { .. }
    )
}

fn settles_sub_thinking(kind: &SessionEventKind) -> bool {
    matches!(
        kind,
        SessionEventKind::ToolCallStarted { .. }
            | SessionEventKind::TextDelta { .. }
            | SessionEventKind::SubagentStarted { .. }
            | SessionEventKind::SubagentFinished { .. }
            | SessionEventKind::TurnFinished
            | SessionEventKind::TurnFailed { .. }
    )
}

fn upsert_tool_finished(
    segments: &mut Vec<RunTraceSegment>,
    tool_call_id: &str,
    status: &str,
    output_preview: &str,
    truncated: bool,
    duration_ms: u64,
) {
    if let Some((segment_index, tool_index)) = find_tool_location(segments, tool_call_id) {
        let Some(RunTraceSegment::Execution { tools, .. }) = segments.get_mut(segment_index) else {
            return;
        };
        let Some(tool) = tools.get_mut(tool_index) else {
            return;
        };
        tool.status = status.to_string();
        tool.output_preview = Some(output_preview.to_string());
        tool.duration_ms = Some(duration_ms);
        tool.truncated = truncated;
        return;
    }
    append_execution_tool(
        segments,
        RunTraceToolItem {
            tool_call_id: tool_call_id.to_string(),
            tool_name: String::new(),
            status: status.to_string(),
            input_preview: String::new(),
            output_preview: Some(output_preview.to_string()),
            duration_ms: Some(duration_ms),
            truncated,
        },
    );
}

/// Pure aggregation over persisted session events, ordered by their SSE sequence.
pub fn aggregate_run_traces(events: &[SessionEvent]) -> Vec<RunTraceSummary> {
    let mut traces: Vec<RunTraceAccumulator> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();

    for event in events {
        // Session control/signal frames never own a trace, so synthetic run IDs such as `queue`
        // or `compaction` cannot appear as phantom runs.
        let Some(trace_run_id) = event.trace_run_id() else {
            continue;
        };
        let position = ensure_run_trace(&mut traces, &mut index, trace_run_id, event.timestamp);
        let Some(trace) = traces.get_mut(position) else {
            continue;
        };
        let subagent_event = is_subagent_event(event);

        if subagent_event {
            if settles_sub_thinking(&event.kind) {
                trace.settle_sub_thinking(&event.run_id, event.timestamp);
            }
            if matches!(event.kind, SessionEventKind::SubagentStarted { .. }) {
                trace.settle_main_thinking(event.timestamp);
            }
        } else if settles_main_thinking(&event.kind) {
            trace.settle_main_thinking(event.timestamp);
        }

        match &event.kind {
            SessionEventKind::TurnStarted => {
                trace.summary.status = "running".to_string();
                if trace.started_at.is_none() {
                    trace.started_at = Some(event.timestamp);
                }
                if trace.summary.started_at.is_none() {
                    trace.summary.started_at = Some(event.timestamp);
                }
            }
            SessionEventKind::TurnFinished => {
                trace.summary.status = "completed".to_string();
                trace.summary.duration_ms = elapsed_ms(trace.started_at, event.timestamp);
            }
            SessionEventKind::TurnSuperseded => {
                trace.summary.status = "superseded".to_string();
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
                    if let Some(location) = ensure_subagent(&mut trace.summary.segments, event)
                        && let Some(record) = subagent_mut(&mut trace.summary.segments, location)
                    {
                        if !matches!(
                            record.segments.last(),
                            Some(RunTraceSegment::Thinking { .. })
                        ) {
                            trace
                                .active_sub_thinking_start
                                .entry(event.run_id.clone())
                                .or_insert(event.timestamp);
                        }
                        append_thinking_segment(&mut record.segments, delta, event.timestamp);
                    }
                } else {
                    if !matches!(
                        trace.summary.segments.last(),
                        Some(RunTraceSegment::Thinking { .. })
                    ) {
                        trace.active_main_thinking_start = Some(event.timestamp);
                    }
                    append_thinking_segment(&mut trace.summary.segments, delta, event.timestamp);
                }
            }
            SessionEventKind::TextDelta { delta } => {
                if subagent_event {
                    if let Some(location) = ensure_subagent(&mut trace.summary.segments, event)
                        && let Some(record) = subagent_mut(&mut trace.summary.segments, location)
                    {
                        append_text_segment(&mut record.segments, delta);
                    }
                } else {
                    append_text_segment(&mut trace.summary.segments, delta);
                }
            }
            SessionEventKind::TaskFocus {
                title,
                detail,
                section_id,
                todos,
            } => {
                if subagent_event {
                    if let Some(location) = ensure_subagent(&mut trace.summary.segments, event)
                        && let Some(record) = subagent_mut(&mut trace.summary.segments, location)
                    {
                        let merged_todos = merge_task_todos(
                            record.task_focus.as_ref().map(|f| f.todos.as_slice()),
                            title,
                            detail.as_deref(),
                            section_id.as_deref(),
                            todos,
                        );
                        record.task_focus = Some(RunTaskFocus {
                            title: title.clone(),
                            detail: detail.clone(),
                            section_id: section_id.clone(),
                            todos: merged_todos,
                        });
                    }
                } else {
                    let merged_todos = merge_task_todos(
                        trace
                            .summary
                            .task_focus
                            .as_ref()
                            .map(|f| f.todos.as_slice()),
                        title,
                        detail.as_deref(),
                        section_id.as_deref(),
                        todos,
                    );
                    trace.summary.task_focus = Some(RunTaskFocus {
                        title: title.clone(),
                        detail: detail.clone(),
                        section_id: section_id.clone(),
                        todos: merged_todos,
                    });
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
                    if let Some(location) = ensure_subagent(&mut trace.summary.segments, event)
                        && let Some(record) = subagent_mut(&mut trace.summary.segments, location)
                    {
                        append_execution_tool(&mut record.segments, item);
                    }
                } else {
                    append_execution_tool(&mut trace.summary.segments, item);
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
                    if let Some(location) = ensure_subagent(&mut trace.summary.segments, event)
                        && let Some(record) = subagent_mut(&mut trace.summary.segments, location)
                    {
                        upsert_tool_finished(
                            &mut record.segments,
                            tool_call_id,
                            status,
                            output_preview,
                            *truncated,
                            *duration_ms,
                        );
                    }
                } else {
                    finish_tool(
                        &mut trace.summary.segments,
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
                if let Some(mut subagent) =
                    remove_subagent_by_id(&mut trace.summary.segments, subagent_run_id)
                {
                    subagent.subagent_run_id = subagent_run_id.clone();
                    subagent.agent_name = agent_name.clone();
                    subagent.display_name = display_name.clone();
                    subagent.task = task.clone();
                    subagent.section_id = section_id.clone();
                    subagent.status = "running".to_string();
                    // A resumed subagent starts a new round in the latest execution
                    // block: its previous final summary must not survive into the new
                    // round, while its accumulated segments and start clock are kept.
                    subagent.summary = None;
                    if subagent.started_at.is_none() {
                        subagent.started_at = Some(event.timestamp);
                    }
                    append_execution_subagent(&mut trace.summary.segments, subagent);
                } else {
                    append_execution_subagent(
                        &mut trace.summary.segments,
                        RunTraceSubagent {
                            subagent_run_id: subagent_run_id.clone(),
                            agent_name: agent_name.clone(),
                            display_name: display_name.clone(),
                            task: task.clone(),
                            section_id: section_id.clone(),
                            status: "running".to_string(),
                            started_at: Some(event.timestamp),
                            summary: None,
                            segments: Vec::new(),
                            task_focus: None,
                        },
                    );
                }
            }
            SessionEventKind::SubagentFinished {
                subagent_run_id,
                status,
                summary,
            } => {
                let existing = find_subagent_location(&trace.summary.segments, |sub| {
                    sub.subagent_run_id == *subagent_run_id
                });
                if let Some(location) = existing
                    && let Some(subagent) = subagent_mut(&mut trace.summary.segments, location)
                {
                    subagent.status = status.clone();
                    if !summary.is_empty() {
                        subagent.summary = Some(summary.clone());
                    }
                } else {
                    append_execution_subagent(
                        &mut trace.summary.segments,
                        RunTraceSubagent {
                            subagent_run_id: subagent_run_id.clone(),
                            agent_name: event.agent_id.clone(),
                            display_name: None,
                            task: String::new(),
                            section_id: None,
                            status: status.clone(),
                            started_at: Some(event.timestamp),
                            summary: if summary.is_empty() {
                                None
                            } else {
                                Some(summary.clone())
                            },
                            segments: Vec::new(),
                            task_focus: None,
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
            | SessionEventKind::PendingQueueUpdated { .. }
            | SessionEventKind::CompactionStarted
            | SessionEventKind::CompactionFinished { .. } => {}
        }
    }

    for trace in &mut traces {
        trace.finalize_active_clocks();
    }
    traces.into_iter().map(|trace| trace.summary).collect()
}

fn merge_task_todos(
    previous: Option<&[TaskTodoItem]>,
    title: &str,
    detail: Option<&str>,
    section_id: Option<&str>,
    incoming: &[TaskTodoItem],
) -> Vec<TaskTodoItem> {
    if !incoming.is_empty() {
        return incoming.to_vec();
    }
    let Some(prev) = previous.filter(|items| !items.is_empty()) else {
        return Vec::new();
    };
    let mut updated: Vec<TaskTodoItem> = prev
        .iter()
        .map(|item| {
            let mut next = item.clone();
            if next.status == TaskTodoStatus::InProgress && next.title != title {
                next.status = TaskTodoStatus::Completed;
            }
            next
        })
        .collect();
    if let Some(matched) = updated.iter_mut().find(|item| item.title == title) {
        matched.status = TaskTodoStatus::InProgress;
        if let Some(d) = detail {
            matched.detail = Some(d.to_string());
        }
        if let Some(s) = section_id {
            matched.section_id = Some(s.to_string());
        }
    } else {
        updated.push(TaskTodoItem {
            id: None,
            title: title.to_string(),
            status: TaskTodoStatus::InProgress,
            detail: detail.map(ToOwned::to_owned),
            section_id: section_id.map(ToOwned::to_owned),
        });
    }
    updated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct TraceFixture {
        events: Vec<SessionEvent>,
        expected_traces: Vec<RunTraceSummary>,
    }

    #[test]
    fn golden_trace_fixtures_pin_the_projection_contract() {
        for (name, source) in [
            (
                "main_run",
                include_str!("../../../contracts/fixtures/session-events/main_run.json"),
            ),
            (
                "subagent_run",
                include_str!("../../../contracts/fixtures/session-events/subagent_run.json"),
            ),
            (
                "subagent_resume",
                include_str!("../../../contracts/fixtures/session-events/subagent_resume.json"),
            ),
            (
                "session_control_frames",
                include_str!(
                    "../../../contracts/fixtures/session-events/session_control_frames.json"
                ),
            ),
        ] {
            let fixture: TraceFixture = serde_json::from_str(source).unwrap();
            let actual = aggregate_run_traces(&fixture.events);
            assert_eq!(actual, fixture.expected_traces, "fixture {name}");
        }
    }
}
