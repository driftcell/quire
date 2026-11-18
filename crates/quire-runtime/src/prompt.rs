//! Prompt-cache-preserving context assembly.

use quire_session_protocol::MessageRole;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeContext {
    /// The frozen prefix. Resume never rewrites it.
    pub pinned_system_prompt: String,
    pub messages: Vec<ContextMessage>,
}

/// Build model context for a resumed turn.
///
/// When the knowledge-base version advanced after the prompt was frozen, a transient
/// `<knowledge_update>` reminder is appended after history and before the new user message. The
/// pinned system prompt and all prior messages remain byte-for-byte stable for provider caches.
pub fn build_resume_context(
    pinned_system_prompt: &str,
    history: &[ContextMessage],
    user_message: &str,
    kb_version: u64,
    synced_kb_version: u64,
) -> ResumeContext {
    let mut messages = history.to_vec();
    if kb_version > synced_kb_version {
        messages.push(ContextMessage {
            role: MessageRole::System,
            content: format!(
                "<knowledge_update>Knowledge base advanced from version {synced_kb_version} to {kb_version}. Call load_skill or read_skill_resource before relying on skill-specific guidance.</knowledge_update>"
            ),
        });
    }
    messages.push(ContextMessage {
        role: MessageRole::User,
        content: user_message.to_string(),
    });
    ResumeContext {
        pinned_system_prompt: pinned_system_prompt.to_string(),
        messages,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_keeps_pinned_prompt_and_injects_knowledge_update() {
        let history = vec![ContextMessage {
            role: MessageRole::User,
            content: "Earlier request".into(),
        }];
        let pinned = "FROZEN PREFIX";
        let context = build_resume_context(pinned, &history, "New request", 9, 4);
        assert_eq!(context.pinned_system_prompt, pinned);
        assert_eq!(context.messages.len(), 3);
        assert_eq!(
            context.messages.get(1).map(|m| m.role),
            Some(MessageRole::System)
        );
        assert!(
            context
                .messages
                .get(1)
                .is_some_and(|message| message.content.contains("version 4 to 9"))
        );
        assert_eq!(
            context
                .messages
                .last()
                .map(|message| message.content.as_str()),
            Some("New request")
        );
        assert_eq!(
            context
                .messages
                .first()
                .map(|message| message.content.as_str()),
            Some("Earlier request")
        );
    }

    #[test]
    fn unchanged_kb_does_not_add_a_reminder() {
        let context = build_resume_context("prefix", &[], "hello", 7, 7);
        assert_eq!(context.messages.len(), 1);
        assert_eq!(
            context.messages.first().map(|m| m.role),
            Some(MessageRole::User)
        );
    }
}
