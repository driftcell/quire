//! Subagent scheduling, lifecycle and outcome aggregation.

use std::{collections::HashMap, sync::Arc, time::Instant};

use tracing::warn;

use quire_protocol::short_id;
use rig_core::{
    completion::Message,
    rig_tool,
    tool::{ToolContext, ToolExecutionError},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::task::JoinSet;

use crate::{
    error::{RuntimeError, RuntimeResult},
    model_stream::ModelStreamEvent,
};

use super::{
    AgentExecution, EventScope, RigModelCore,
    builtin_tools::{runtime_tool_scope, tool_other},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, rig_core::schemars::JsonSchema)]
pub struct SubagentTask {
    pub agent_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_id: Option<String>,
    /// Resume an existing subagent from an earlier round in the same session by
    /// passing its `subagent_run_id`. History is kept in memory per runtime process.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_run_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentOutcome {
    pub agent_name: String,
    pub status: String,
    pub summary: String,
    pub subagent_run_id: String,
}

const SUBAGENT_HISTORY_CAPACITY: usize = 128;

#[derive(Clone)]
struct SubagentHistoryEntry {
    agent_name: String,
    messages: Vec<Message>,
    updated_at: Instant,
}

#[derive(Default)]
pub(super) struct SubagentHistory {
    entries: HashMap<(String, String), SubagentHistoryEntry>,
}

struct ResumePlan {
    run_id: String,
    messages: Vec<Message>,
    resumed: bool,
}

impl SubagentHistory {
    fn resume_messages(&self, session_id: &str, task: &SubagentTask) -> ResumePlan {
        if let Some(resume_id) = task.resume_run_id.as_deref()
            && let Some(entry) = self
                .entries
                .get(&(session_id.to_string(), resume_id.to_string()))
        {
            if entry.agent_name == task.agent_name {
                let mut messages = entry.messages.clone();
                messages.push(Message::user(task.task.clone()));
                return ResumePlan {
                    run_id: resume_id.to_string(),
                    messages,
                    resumed: true,
                };
            }
            warn!(%resume_id, requested_agent = %task.agent_name, cached_agent = %entry.agent_name, "subagent resume agent mismatch; starting a fresh subagent");
        } else if task.resume_run_id.is_some() {
            warn!(session_id, resume_run_id = ?task.resume_run_id, "subagent resume history not found; starting a fresh subagent");
        }
        ResumePlan {
            run_id: short_id("run_sub"),
            messages: vec![Message::user(task.task.clone())],
            resumed: false,
        }
    }

    fn insert(&mut self, session_id: &str, run_id: &str, agent_name: &str, messages: Vec<Message>) {
        self.entries.insert(
            (session_id.to_string(), run_id.to_string()),
            SubagentHistoryEntry {
                agent_name: agent_name.to_string(),
                messages,
                updated_at: Instant::now(),
            },
        );
        if self.entries.len() > SUBAGENT_HISTORY_CAPACITY
            && let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.updated_at)
                .map(|(key, _)| key.clone())
        {
            self.entries.remove(&oldest);
        }
    }
}

#[rig_tool(
    name = "spawn_subagents",
    description = "Run multiple subagents concurrently with a JoinSet. Call list_subagents first to get valid subagent names."
)]
pub(super) async fn run_subagents_tool(
    #[rig(context)] context: &mut ToolContext,
    tasks: Vec<SubagentTask>,
) -> Result<Value, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let bound_doc_id = runtime.doc_id.lock().await.clone();
    let mut joinset = JoinSet::new();
    for task in tasks {
        let engine = Arc::clone(&runtime.engine);
        let scope = runtime.event_scope.clone();
        let mut execution = runtime.execution.clone();
        execution.doc_id = bound_doc_id.clone();
        joinset.spawn(async move { engine.run_subagent(scope, execution, task).await });
    }
    let mut outcomes = Vec::new();
    while let Some(joined) = joinset.join_next().await {
        match joined {
            Ok(outcome) => outcomes.push(outcome),
            Err(error) => outcomes.push(SubagentOutcome {
                agent_name: "unknown".into(),
                status: "failed".into(),
                summary: format!("subagent join error: {error}"),
                subagent_run_id: "unknown".into(),
            }),
        }
    }
    serde_json::to_value(outcomes).map_err(tool_other)
}

/// Run subagent closures concurrently with a bounded JoinSet.
pub async fn spawn_subagents<F, Fut>(
    tasks: Vec<SubagentTask>,
    runner: F,
) -> RuntimeResult<Vec<SubagentOutcome>>
where
    F: Fn(SubagentTask) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = SubagentOutcome> + Send + 'static,
{
    let mut joinset = tokio::task::JoinSet::new();
    for task in tasks {
        joinset.spawn(runner(task));
    }
    let mut outcomes = Vec::new();
    while let Some(result) = joinset.join_next().await {
        outcomes
            .push(result.map_err(|error| {
                RuntimeError::Internal(format!("subagent join error: {error}"))
            })?);
    }
    Ok(outcomes)
}

impl RigModelCore {
    pub(super) async fn run_subagent(
        self: Arc<Self>,
        parent_scope: EventScope,
        parent: AgentExecution,
        task: SubagentTask,
    ) -> SubagentOutcome {
        let plan = self
            .subagent_history
            .lock()
            .await
            .resume_messages(&parent.session_id, &task);
        let subagent_run_id = plan.run_id.clone();
        if plan.resumed {
            tracing::debug!(%subagent_run_id, "resuming subagent with preserved history");
        }
        parent_scope
            .send_raw(ModelStreamEvent::SubagentStarted {
                subagent_run_id: subagent_run_id.clone(),
                agent_name: task.agent_name.clone(),
                display_name: task.display_name.clone(),
                task: task.task.clone(),
                section_id: task.section_id.clone(),
            })
            .await;
        let result = async {
            if parent.cancellation.is_cancelled() {
                return Err(RuntimeError::InvalidRequest("run cancelled".into()));
            }
            let definition = self
                .kb
                .get_agent_on(parent.kb_branch.as_deref(), &task.agent_name)
                .await?
                .ok_or_else(|| {
                    RuntimeError::InvalidRequest(format!(
                        "subagent `{}` is not defined on branch `{}`; call list_subagents to inspect available subagents",
                        task.agent_name,
                        parent.kb_branch.as_deref().unwrap_or("active")
                    ))
                })?;
            let execution = AgentExecution {
                agent_id: task.agent_name.clone(),
                model_route: task.model.clone().unwrap_or(definition.model),
                allowed_tools: definition.allowed_tools,
                disable_tools: false,
                system_prompt: definition.system_prompt,
                kb_branch: parent.kb_branch.clone(),
                doc_id: parent.doc_id.clone(),
                session_id: parent.session_id.clone(),
                run_id: subagent_run_id.clone(),
                cancellation: parent.cancellation.clone(),
            };
            let scope = parent_scope.child(subagent_run_id.clone(), task.agent_name.clone());
            let (outcome, history) = self.clone().run_turn(execution, plan.messages, scope).await?;
            self.subagent_history.lock().await.insert(
                &parent.session_id,
                &subagent_run_id,
                &task.agent_name,
                history,
            );
            Ok::<String, RuntimeError>(outcome)
        }
        .await;
        let (status, summary) = match result {
            Ok(_) if parent_scope.cancellation.is_cancelled() => {
                ("cancelled".to_string(), "cancelled".to_string())
            }
            Ok(summary) if summary.trim().is_empty() => {
                ("completed".to_string(), "completed".to_string())
            }
            Ok(summary) => ("completed".to_string(), summary),
            Err(RuntimeError::InvalidRequest(message))
                if parent_scope.cancellation.is_cancelled() || message == "run cancelled" =>
            {
                ("cancelled".to_string(), "cancelled".to_string())
            }
            Err(error) => ("failed".to_string(), error.to_string()),
        };
        parent_scope
            .send_raw(ModelStreamEvent::SubagentFinished {
                subagent_run_id: subagent_run_id.clone(),
                status: status.clone(),
                summary: summary.clone(),
            })
            .await;
        SubagentOutcome {
            agent_name: task.agent_name,
            status,
            summary,
            subagent_run_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subagents_run_concurrently_and_preserve_outcomes() {
        let tasks = vec![
            SubagentTask {
                agent_name: "a".into(),
                display_name: None,
                model: None,
                task: "one".into(),
                section_id: None,
                resume_run_id: None,
            },
            SubagentTask {
                agent_name: "b".into(),
                display_name: None,
                model: Some("subagent/model-b".into()),
                task: "two".into(),
                section_id: Some("ch2".into()),
                resume_run_id: None,
            },
        ];
        let outcomes = spawn_subagents(tasks, |task| async move {
            SubagentOutcome {
                agent_name: task.agent_name.clone(),
                status: "completed".into(),
                summary: task.task.clone(),
                subagent_run_id: format!("run_sub_{}", task.agent_name),
            }
        })
        .await
        .unwrap();
        assert_eq!(outcomes.len(), 2);
    }

    #[test]
    fn resume_reuses_the_previous_subagent_history() {
        let mut history = SubagentHistory::default();
        history.insert(
            "sess_1",
            "run_sub_1",
            "reviewer",
            vec![Message::user("first round")],
        );
        let task = SubagentTask {
            agent_name: "reviewer".into(),
            display_name: None,
            model: None,
            task: "second round".into(),
            section_id: None,
            resume_run_id: Some("run_sub_1".into()),
        };

        let plan = history.resume_messages("sess_1", &task);
        assert!(plan.resumed);
        assert_eq!(plan.run_id, "run_sub_1");
        assert_eq!(plan.messages.len(), 2);
        let rendered = format!("{:?}", plan.messages);
        assert!(rendered.contains("first round"));
        assert!(rendered.contains("second round"));
    }

    #[test]
    fn missing_resume_history_starts_a_fresh_subagent() {
        let history = SubagentHistory::default();
        let task = SubagentTask {
            agent_name: "reviewer".into(),
            display_name: None,
            model: None,
            task: "second round".into(),
            section_id: None,
            resume_run_id: Some("run_sub_missing".into()),
        };

        let plan = history.resume_messages("sess_1", &task);
        assert!(!plan.resumed);
        assert_ne!(plan.run_id, "run_sub_missing");
        assert_eq!(plan.messages.len(), 1);
    }

    #[test]
    fn resume_rejects_a_different_agent_name() {
        let mut history = SubagentHistory::default();
        history.insert(
            "sess_1",
            "run_sub_1",
            "analyst",
            vec![Message::user("first round")],
        );
        let task = SubagentTask {
            agent_name: "reviewer".into(),
            display_name: None,
            model: None,
            task: "second round".into(),
            section_id: None,
            resume_run_id: Some("run_sub_1".into()),
        };

        let plan = history.resume_messages("sess_1", &task);
        assert!(!plan.resumed);
        assert_ne!(plan.run_id, "run_sub_1");
        assert_eq!(plan.messages.len(), 1);
    }
}
