//! Structured, prompt-cache-friendly context compaction over canonical rig-memory history.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const COMPACTION_TIMEOUT_SECS: u64 = 120;
pub const COMPACTION_INSTRUCTION: &str = include_str!("compaction_prompt.md");
pub const COMPACTION_MODE_MESSAGE: &str = "<compaction_mode>Tools are disabled for this request. This is not a task continuation. Do not emit tool calls, XML tags or DSML. Return only the requested structured handoff.</compaction_mode>";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CompactionHandoff {
    pub task_and_goal: String,
    pub confirmed_scope_and_metrics: String,
    pub document_state: String,
    pub next_steps: String,
}

impl CompactionHandoff {
    pub fn to_markdown(&self) -> String {
        format!(
            "# 会话交接摘要\n\n## 1. 任务与目标\n{}\n\n## 2. 已确认口径\n{}\n\n## 3. 文档状态\n{}\n\n## 4. 下一步\n{}",
            self.task_and_goal.trim(),
            self.confirmed_scope_and_metrics.trim(),
            self.document_state.trim(),
            self.next_steps.trim()
        )
    }
}

pub fn estimate_context_tokens(text: &str) -> u64 {
    let chars = text.chars().count();
    u64::try_from(chars.div_ceil(3)).unwrap_or(u64::MAX).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoff_projects_the_stable_markdown_contract() {
        let handoff = CompactionHandoff {
            task_and_goal: "goal".into(),
            confirmed_scope_and_metrics: "scope".into(),
            document_state: "doc".into(),
            next_steps: "next".into(),
        };
        let markdown = handoff.to_markdown();
        assert!(markdown.contains("## 1. 任务与目标\ngoal"));
        assert!(markdown.contains("## 4. 下一步\nnext"));
    }

    #[tokio::test]
    async fn prompt_typed_recovers_the_structured_handoff() {
        let payload = serde_json::json!({
            "task_and_goal": "goal",
            "confirmed_scope_and_metrics": "scope",
            "document_state": "doc",
            "next_steps": "next"
        });
        let model =
            rig::test_utils::MockCompletionModel::from_turns([rig::test_utils::MockTurn::text(
                payload.to_string(),
            )]);
        let agent = rig::agent::AgentBuilder::new(model).build();
        let response = agent
            .prompt_typed::<CompactionHandoff>("compact")
            .max_turns(1)
            .await
            .unwrap();
        assert_eq!(response.output.task_and_goal, "goal");
        assert!(
            response
                .output
                .to_markdown()
                .contains("## 3. 文档状态\ndoc")
        );
    }

    #[test]
    fn estimate_is_never_zero() {
        assert_eq!(estimate_context_tokens(""), 1);
        assert!(estimate_context_tokens("summary text") > 0);
    }
}
