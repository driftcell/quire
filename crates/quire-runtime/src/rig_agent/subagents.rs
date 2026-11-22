//! Subagent scheduling, lifecycle and outcome aggregation.

use std::sync::Arc;

use rig_core::{
    message::Message,
    rig_tool,
    tool::{ToolContext, ToolExecutionError},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::task::JoinSet;
use tracing::warn;

use quire_protocol::short_id;

use crate::{
    error::{RuntimeError, RuntimeResult},
    model_stream::ModelStreamEvent,
};

use super::{
    AgentExecution, EventScope, RigModelCore,
    builtin_tools::{quire_turn_meta, runtime_tool_scope, tool_other},
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
    /// Resume the canonical rig-memory conversation of an earlier child run.
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

#[derive(Clone)]
pub(super) struct SubagentParent {
    session_id: String,
    kb_branch: Option<String>,
    doc_id: Option<String>,
    cancellation: tokio_util::sync::CancellationToken,
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
    let turn = quire_turn_meta(context)?;
    let parent = SubagentParent {
        session_id: turn.session_id,
        kb_branch: turn.kb_branch,
        doc_id: runtime.doc_id.lock().await.clone(),
        cancellation: runtime.cancellation.clone(),
    };
    let mut joinset = JoinSet::new();
    for task in tasks {
        let engine = Arc::clone(&runtime.engine);
        let scope = runtime.event_scope.clone();
        let parent = parent.clone();
        joinset.spawn(async move { engine.run_subagent(scope, parent, task).await });
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

/// Run subagent closures concurrently with a JoinSet.
pub async fn spawn_subagents<F, Fut>(
    tasks: Vec<SubagentTask>,
    runner: F,
) -> RuntimeResult<Vec<SubagentOutcome>>
where
    F: Fn(SubagentTask) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = SubagentOutcome> + Send + 'static,
{
    let mut joinset = JoinSet::new();
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
        parent: SubagentParent,
        task: SubagentTask,
    ) -> SubagentOutcome {
        let subagent_run_id = task
            .resume_run_id
            .clone()
            .unwrap_or_else(|| short_id("run_sub"));
        if task.resume_run_id.is_some() {
            match self.kb.store().memory().await {
                Ok(memory) => match memory
                    .load(&crate::memory::ConversationMemory::subagent_log(
                        &parent.session_id,
                        &subagent_run_id,
                    ))
                    .await
                {
                    Ok(conversation) if conversation.messages().is_empty() => {
                        warn!(%subagent_run_id, "subagent resume history is empty; starting a fresh canonical conversation");
                    }
                    Ok(_) => {}
                    Err(error) => {
                        warn!(%error, %subagent_run_id, "failed to inspect subagent resume history")
                    }
                },
                Err(error) => warn!(%error, %subagent_run_id, "failed to open subagent memory"),
            }
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
            let (outcome, _) = self
                .clone()
                .run_turn(
                    execution,
                    Message::user(task.task.clone()),
                    Vec::new(),
                    scope,
                    false,
                    true,
                )
                .await?;
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
                summary: format!("done: {}", task.task),
                subagent_run_id: format!("run_sub_{}", task.agent_name),
            }
        })
        .await
        .unwrap();
        assert_eq!(outcomes.len(), 2);
    }
}
