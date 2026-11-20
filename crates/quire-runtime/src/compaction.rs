//! Prompt-cache-friendly context compaction.
//!
//! Compaction reuses the frozen pinned prompt and the history prefix as-is,
//! appends one fixed instruction, and only swaps the pinned prompt after the
//! summary succeeds.

use chrono::{DateTime, Utc};
use quire_session_protocol::{MessageRole, SessionMessage};

use crate::{comment_refs::render_stored_user_message, prompt::ContextMessage};

pub const COMPACTION_TIMEOUT_SECS: u64 = 120;

pub const COMPACTION_INSTRUCTION: &str = include_str!("compaction_prompt.md");

pub const COMPACTION_MODE_MESSAGE: &str = "<compaction_mode>Tools are disabled for this request. This is not a task continuation. Do not emit tool calls, XML tags or DSML. Return only the requested Markdown handoff summary.</compaction_mode>";

pub fn validate_compaction_summary(summary: &str) -> bool {
    if summary.contains("<｜｜DSML｜｜") || summary.contains("<|tool") {
        return false;
    }
    let heading_count = summary
        .lines()
        .filter(|line| line.trim_start().starts_with("## "))
        .count();
    let meaningful_chars = summary.chars().filter(|ch| !ch.is_whitespace()).count();
    heading_count >= 3 && meaningful_chars >= 80
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompactionState {
    pub summary: Option<String>,
    pub up_to_message_id: Option<String>,
    pub compacted_at: Option<DateTime<Utc>>,
}

fn is_context_role(role: MessageRole) -> bool {
    matches!(
        role,
        MessageRole::User | MessageRole::Assistant | MessageRole::Tool
    )
}

/// Assemble the model-visible history after an optional compaction anchor.
///
/// Persisted tool output is flat text without provider tool-call IDs. Any tool
/// role is therefore reclassified as user text, which guarantees that an
/// interrupted tool turn can never leave an unpaired tool_call/tool_result pair
/// in a provider request.
pub fn assemble_context_history(
    messages: &[SessionMessage],
    state: &CompactionState,
    doc_id: Option<&str>,
    synced_kb_version: u64,
) -> Vec<ContextMessage> {
    let start = state
        .up_to_message_id
        .as_deref()
        .and_then(|anchor| {
            messages
                .iter()
                .position(|message| message.message_id == anchor)
                .map(|index| index.saturating_add(1))
        })
        .unwrap_or(0);

    let mut history = Vec::new();
    if let Some(summary) = state.summary.as_deref()
        && !summary.trim().is_empty()
    {
        history.push(ContextMessage {
            role: MessageRole::System,
            content: format!(
                "<compaction_handoff>\n{}\n</compaction_handoff>",
                summary.trim()
            ),
        });
    }

    for message in messages.iter().skip(start) {
        if !is_context_role(message.role)
            || (message.content.trim().is_empty()
                && message.comment_refs.is_empty()
                && message.attachment_ids.is_empty())
        {
            continue;
        }
        history.push(ContextMessage {
            role: if message.role == MessageRole::Tool {
                MessageRole::User
            } else {
                message.role
            },
            content: render_stored_user_message(message),
        });
    }

    history.push(ContextMessage {
        role: MessageRole::System,
        content: format!(
            "<report_state>\ndoc_id: {}\nsynced_kb_version: {synced_kb_version}\n</report_state>",
            doc_id.unwrap_or("none")
        ),
    });
    history
}

pub fn estimate_context_tokens(text: &str) -> u64 {
    let chars = text.chars().count();
    u64::try_from(chars.div_ceil(3)).unwrap_or(u64::MAX).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: &str, role: MessageRole, content: &str) -> SessionMessage {
        SessionMessage {
            message_id: id.into(),
            session_id: "sess_test".into(),
            role,
            content: content.into(),
            comment_refs: Vec::new(),
            attachment_ids: Vec::new(),
            run_id: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn history_starts_with_summary_then_messages_after_anchor() {
        let messages = vec![
            message("old", MessageRole::User, "old request"),
            message("anchor", MessageRole::Assistant, "old answer"),
            message("marker", MessageRole::Compaction, "summary marker"),
            message("tool", MessageRole::Tool, "interrupted output"),
            message("new", MessageRole::User, "new request"),
        ];
        let state = CompactionState {
            summary: Some("handoff".into()),
            up_to_message_id: Some("anchor".into()),
            compacted_at: None,
        };

        let history = assemble_context_history(&messages, &state, Some("doc_1"), 42);

        assert_eq!(history.len(), 4);
        assert_eq!(history[0].role, MessageRole::System);
        assert!(history[0].content.contains("handoff"));
        assert_eq!(history[1].role, MessageRole::User);
        assert_eq!(history[1].content, "interrupted output");
        assert_eq!(history[2].content, "new request");
        let state_message = history.last().expect("report state");
        assert_eq!(state_message.role, MessageRole::System);
        assert!(state_message.content.contains("doc_1"));
        assert!(state_message.content.contains("42"));
    }

    #[test]
    fn comment_only_messages_remain_in_model_history() {
        let mut comment_only = message("comment", MessageRole::User, "");
        comment_only
            .comment_refs
            .push(quire_session_protocol::SessionCommentRef {
                thread_id: "ct_1".into(),
                block_id: "block_1".into(),
                end_block_id: None,
                anchor_blocks: Vec::new(),
                status: "open".into(),
                quote: None,
                end_quote: None,
                anchor_snapshot: None,
                comment: "Revise this paragraph".into(),
            });

        let history = assemble_context_history(
            &[comment_only],
            &CompactionState::default(),
            Some("doc_1"),
            42,
        );
        assert_eq!(history.len(), 2);
        let projected: serde_json::Value =
            serde_json::from_str(&history[0].content).unwrap_or_default();
        assert_eq!(projected["instruction"], "");
        assert_eq!(projected["comment_refs"][0]["thread_id"], "ct_1");
    }

    #[test]
    fn attachment_only_messages_remain_in_model_history() {
        let mut attachment_only = message("attachment", MessageRole::User, "");
        attachment_only.attachment_ids.push("att_1".into());

        let history = assemble_context_history(
            &[attachment_only],
            &CompactionState::default(),
            Some("doc_1"),
            42,
        );
        assert_eq!(history.len(), 2);
        let projected: serde_json::Value =
            serde_json::from_str(&history[0].content).unwrap_or_default();
        assert_eq!(projected["instruction"], "");
        assert_eq!(projected["attachment_ids"][0], "att_1");
    }

    #[test]
    fn estimate_is_never_zero() {
        assert_eq!(estimate_context_tokens(""), 1);
        assert!(estimate_context_tokens("summary text") > 0);
    }

    #[test]
    fn only_structured_handoff_summaries_are_accepted() {
        let valid = "# 会话交接摘要\n\n## 1. 任务与目标\n继续完成对标研报。\n\n## 2. 已确认口径\n2020-2021 年，五校，A 校为基线。\n\n## 3. 文档状态\n未绑定报告。\n\n## 4. 下一步\n等待用户确认。";
        assert!(validate_compaction_summary(valid));
        assert!(!validate_compaction_summary(
            "<｜｜DSML｜｜ calls><｜｜DSML｜｜ invoke>"
        ));
        assert!(!validate_compaction_summary(
            "## Objective & Goal\nonly one"
        ));
    }
}
