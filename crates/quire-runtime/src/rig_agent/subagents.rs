//! Subagent scheduling, lifecycle and outcome aggregation.

use std::{collections::HashMap, sync::Arc};

use rig::{
    message::Message,
    rig_tool,
    tool::{ToolContext, ToolExecutionError},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::task::JoinSet;
use tracing::warn;

use quire_protocol::short_id;

use crate::{error::RuntimeError, model_stream::ModelStreamEvent};

use super::{
    AgentExecution, RigModelCore, SharedDocId, StreamEventSender,
    builtin_tools::{quire_turn_meta, runtime_tool_scope, tool_other},
};

/// One subagent task dispatched by `spawn_subagents`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, rig::schemars::JsonSchema)]
pub struct SubagentTask {
    /// Enabled subagent name on the current KB branch (e.g. `"analyst"`, `"writer"`, `"reviewer"`, `"designer"`). Call `list_subagents` if unsure.
    pub agent_name: String,
    /// Short human-readable task title displayed on the UI trace card (e.g. `"撰写第二章：师资梯队对标"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Optional registered model route ID override (defaults to the subagent's configured model).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Self-contained task brief including `doc_id`, `expected_revision`, assigned `block_id` prefix, verified `source_ids`, and visual anchor IDs. Subagents cannot call `ask_user` or `spawn_subagents`.
    pub task: String,
    /// Optional Heading `block_id` (e.g. `"ch2"`) linking the UI subagent card directly to that report chapter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_id: Option<String>,
    /// Pass a previous `subagent_run_id` to resume that child run's persistent multi-turn conversation memory (e.g. re-invoking the same `reviewer` after fixes).
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
    doc_id: SharedDocId,
    cancellation: tokio_util::sync::CancellationToken,
}

/// Launch one or more specialized subagents concurrently in a JoinSet and wait for their structured outcomes.
///
/// Call `list_subagents` first if you need valid subagent names. Outcomes are returned in the exact
/// input `tasks` order with `agent_name`, `status` (`"completed" | "failed" | "cancelled"`), `summary`,
/// and `subagent_run_id` (passable as `resume_run_id` on follow-up calls). Always verify actual document
/// changes via `get_document_digest` or `lint_document` after subagents finish.
#[rig_tool(name = "spawn_subagents")]
pub(super) async fn run_subagents_tool(
    #[rig(context)] context: &mut ToolContext,
    /// Subagent tasks to execute concurrently. Assign disjoint `block_id` prefixes when multiple subagents write to the same report in parallel.
    tasks: Vec<SubagentTask>,
) -> Result<Value, ToolExecutionError> {
    let runtime = runtime_tool_scope(context)?;
    let turn = quire_turn_meta(context)?;
    let parent = SubagentParent {
        session_id: turn.session_id,
        kb_branch: turn.kb_branch,
        doc_id: Arc::clone(&runtime.doc_id),
        cancellation: runtime.cancellation.clone(),
    };
    let task_count = tasks.len();
    let mut joinset = JoinSet::new();
    let mut metadata = HashMap::new();
    let mut assignments = Vec::with_capacity(task_count);
    let mut slots: Vec<Option<SubagentOutcome>> = Vec::with_capacity(task_count);
    slots.resize_with(task_count, || None);
    for (index, task) in tasks.into_iter().enumerate() {
        let engine = Arc::clone(&runtime.engine);
        let scope = runtime.event_scope.clone();
        let parent = parent.clone();
        let agent_name = task.agent_name.clone();
        let subagent_run_id = task
            .resume_run_id
            .clone()
            .unwrap_or_else(|| short_id("run_sub"));
        assignments.push((agent_name.clone(), subagent_run_id.clone()));
        let child_id = subagent_run_id.clone();
        let handle =
            joinset.spawn(async move { engine.run_subagent(scope, parent, task, child_id).await });
        metadata.insert(handle.id(), (index, agent_name, subagent_run_id));
    }
    while let Some(joined) = joinset.join_next_with_id().await {
        match joined {
            Ok((id, outcome)) => {
                if let Some((index, _, _)) = metadata.remove(&id)
                    && let Some(slot) = slots.get_mut(index)
                {
                    *slot = Some(outcome);
                }
            }
            Err(error) => {
                if let Some((index, agent_name, subagent_run_id)) = metadata.remove(&error.id())
                    && let Some(slot) = slots.get_mut(index)
                {
                    *slot = Some(SubagentOutcome {
                        agent_name,
                        status: "failed".into(),
                        summary: format!("subagent join error: {error}"),
                        subagent_run_id,
                    });
                }
            }
        }
    }
    let outcomes = ordered_outcomes(slots, assignments);
    serde_json::to_value(outcomes).map_err(tool_other)
}

fn ordered_outcomes(
    slots: Vec<Option<SubagentOutcome>>,
    assignments: Vec<(String, String)>,
) -> Vec<SubagentOutcome> {
    slots
        .into_iter()
        .zip(assignments)
        .map(|(outcome, (agent_name, subagent_run_id))| {
            outcome.unwrap_or(SubagentOutcome {
                agent_name,
                status: "failed".into(),
                summary: "subagent ended without an outcome".into(),
                subagent_run_id,
            })
        })
        .collect()
}

impl RigModelCore {
    pub(super) async fn run_subagent(
        self: Arc<Self>,
        parent_scope: StreamEventSender,
        parent: SubagentParent,
        task: SubagentTask,
        subagent_run_id: String,
    ) -> SubagentOutcome {
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
                system_prompt: definition.system_prompt,
                kb_branch: parent.kb_branch.clone(),
                doc_id: Arc::clone(&parent.doc_id),
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
                    None,
                    scope,
                    false,
                    true,
                )
                .await?;
            Ok::<String, RuntimeError>(outcome)
        }
        .await;
        let (status, summary) = match result {
            Ok(_) if parent.cancellation.is_cancelled() => {
                ("cancelled".to_string(), "cancelled".to_string())
            }
            Ok(summary) if summary.trim().is_empty() => {
                ("completed".to_string(), "completed".to_string())
            }
            Ok(summary) => ("completed".to_string(), summary),
            Err(RuntimeError::InvalidRequest(message))
                if parent.cancellation.is_cancelled() || message == "run cancelled" =>
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

    #[test]
    fn outcomes_keep_input_order_when_later_tasks_finish_first() {
        let outcome = |name: &str| SubagentOutcome {
            agent_name: name.into(),
            status: "completed".into(),
            summary: format!("done: {name}"),
            subagent_run_id: format!("run_sub_{name}"),
        };
        let assignments = vec![
            ("a".into(), "run_sub_a".into()),
            ("b".into(), "run_sub_b".into()),
        ];
        let ordered = ordered_outcomes(vec![Some(outcome("a")), Some(outcome("b"))], assignments);
        assert_eq!(
            ordered
                .iter()
                .map(|outcome| outcome.agent_name.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );

        let missing = ordered_outcomes(
            vec![None, Some(outcome("b"))],
            vec![
                ("a".into(), "run_sub_a".into()),
                ("b".into(), "run_sub_b".into()),
            ],
        );
        assert_eq!(
            missing.first().map(|item| item.agent_name.as_str()),
            Some("a")
        );
        assert_eq!(
            missing.first().map(|item| item.subagent_run_id.as_str()),
            Some("run_sub_a")
        );
        assert_eq!(
            missing.first().map(|item| item.status.as_str()),
            Some("failed")
        );
    }
}
