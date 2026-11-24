//! Canonical structured conversation history backed by Rig's official PostgreSQL memory store.

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use quire_protocol::short_id;
use quire_session_protocol::{MessageRole, SendMessageRequest, SessionMessage};
use rig::memory::{SummaryState, apply_summary, load_async, save_async, summary_state};
use rig::postgres::PostgresSessionStore;
use rig::{
    conversation::{Aside, Author, Conversation, MessageId},
    message::{AssistantContent, Message},
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::{
    comment_refs::render_stored_user_message,
    error::{RuntimeError, RuntimeResult},
};

const MESSAGE_RECEIPT: &str = "quire.message_receipt";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MessageReceipt {
    client_msg_id: String,
    visible: bool,
    message: SessionMessage,
}

/// Cloneable Rig memory handle. Clones share the same PostgreSQL pool.
#[derive(Clone, Debug)]
pub struct ConversationMemory {
    store: PostgresSessionStore,
    pool: PgPool,
}

impl ConversationMemory {
    pub async fn open(pool: PgPool) -> RuntimeResult<Self> {
        Ok(Self {
            store: PostgresSessionStore::open(pool.clone()).await?,
            pool,
        })
    }

    pub fn store(&self) -> &PostgresSessionStore {
        &self.store
    }

    pub fn subagent_log(session_id: &str, run_id: &str) -> String {
        format!("{session_id}:sub:{run_id}")
    }

    pub async fn delete_session(&self, session_id: &str) -> RuntimeResult<()> {
        let prefix = format!("{session_id}:sub:");
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "DELETE FROM rig_memory_log_chunks WHERE name = $1 OR left(name, length($2)) = $2",
        )
        .bind(session_id)
        .bind(&prefix)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM rig_memory_logs WHERE name = $1 OR left(name, length($2)) = $2")
            .bind(session_id)
            .bind(prefix)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn load(&self, log: &str) -> RuntimeResult<Conversation> {
        load_async(&self.store, log)
            .await
            .map_err(|error| RuntimeError::Internal(format!("load conversation `{log}`: {error}")))
    }

    async fn save(&self, log: &str, conversation: &mut Conversation) -> RuntimeResult<()> {
        save_async(&self.store, log, conversation)
            .await
            .map_err(|error| RuntimeError::Internal(format!("save conversation `{log}`: {error}")))
    }

    /// Append one user-visible or internal prompt with an idempotent client key.
    /// UI metadata lives in saved rig-memory asides and is never sent to the model.
    pub async fn append_prompt(
        &self,
        log: &str,
        request: &SendMessageRequest,
        visible: bool,
    ) -> RuntimeResult<(SessionMessage, bool)> {
        let mut conversation = self.load(log).await?;
        if let Some(receipt) = receipts(&conversation)
            .into_values()
            .find(|receipt| receipt.client_msg_id == request.client_msg_id)
        {
            return Ok((receipt.message, true));
        }
        let now = Utc::now();
        let message = SessionMessage {
            message_id: short_id("msg"),
            session_id: log.to_string(),
            role: MessageRole::User,
            content: request.content.clone(),
            comment_refs: request.comment_refs.clone(),
            attachment_ids: request.attachment_ids.clone(),
            run_id: None,
            created_at: now,
        };
        let rendered = render_stored_user_message(&message);
        let message_id = conversation.push(Message::user(rendered));
        let receipt = MessageReceipt {
            client_msg_id: request.client_msg_id.clone(),
            visible,
            message: message.clone(),
        };
        conversation.add_aside(
            Aside::new(
                MESSAGE_RECEIPT,
                Author::plugin("quire"),
                serde_json::to_value(receipt)?,
            )
            .after(Some(message_id)),
        );
        self.save(log, &mut conversation).await?;
        Ok((message, false))
    }

    pub async fn append_internal_prompt(
        &self,
        log: &str,
        client_msg_id: String,
        content: String,
    ) -> RuntimeResult<SessionMessage> {
        let request = SendMessageRequest {
            client_msg_id,
            content,
            comment_refs: Vec::new(),
            attachment_ids: Vec::new(),
        };
        let (message, _) = self.append_prompt(log, &request, false).await?;
        Ok(message)
    }

    /// Remove a failed run's suffix while retaining the selected user prompt as the latest entry.
    pub async fn rewind_to_user_prompt(
        &self,
        log: &str,
        session_message_id: &str,
    ) -> RuntimeResult<SessionMessage> {
        let mut conversation = self.load(log).await?;
        let message = rewind_conversation_to_prompt(&mut conversation, session_message_id)?;
        self.save(log, &mut conversation).await?;
        Ok(message)
    }

    pub async fn find_by_client_id(
        &self,
        log: &str,
        client_msg_id: &str,
    ) -> RuntimeResult<Option<SessionMessage>> {
        let conversation = self.load(log).await?;
        Ok(receipts(&conversation)
            .into_values()
            .find(|receipt| receipt.visible && receipt.client_msg_id == client_msg_id)
            .map(|receipt| receipt.message))
    }

    /// Project visible user/assistant messages from canonical messages and saved asides.
    pub async fn list_messages(&self, log: &str) -> RuntimeResult<Vec<SessionMessage>> {
        let conversation = self.load(log).await?;
        let mut receipts = receipts(&conversation);
        let mut messages = Vec::new();
        for entry in conversation.entries() {
            let Some(receipt) = receipts.remove(&entry.id) else {
                continue;
            };
            if receipt.visible {
                messages.push(receipt.message);
            }
        }
        Ok(messages)
    }

    /// Test/stub providers use the same canonical store; real Rig runs already commit receipts.
    pub async fn append_assistant_if_missing(
        &self,
        log: &str,
        run_id: &str,
        content: &str,
    ) -> RuntimeResult<SessionMessage> {
        if content.is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "an empty assistant response must not be persisted".into(),
            ));
        }
        let mut conversation = self.load(log).await?;
        if let Some(message) = receipts(&conversation).into_values().find_map(|receipt| {
            (receipt.visible
                && receipt.message.run_id.as_deref() == Some(run_id)
                && receipt.message.role == MessageRole::Assistant)
                .then_some(receipt.message)
        }) {
            return Ok(message);
        }
        let message = SessionMessage {
            message_id: short_id("msg"),
            session_id: log.to_string(),
            role: MessageRole::Assistant,
            content: content.to_string(),
            comment_refs: Vec::new(),
            attachment_ids: Vec::new(),
            run_id: Some(run_id.to_string()),
            created_at: Utc::now(),
        };
        let message_id = conversation.push(Message::assistant(content));
        let receipt = MessageReceipt {
            client_msg_id: format!("assistant:{run_id}"),
            visible: true,
            message: message.clone(),
        };
        conversation.add_aside(
            Aside::new(
                MESSAGE_RECEIPT,
                Author::plugin("quire"),
                serde_json::to_value(receipt)?,
            )
            .after(Some(message_id)),
        );
        self.save(log, &mut conversation).await?;
        Ok(message)
    }

    pub async fn history_for_compaction(
        &self,
        log: &str,
        exclude_message_id: Option<&str>,
    ) -> RuntimeResult<Vec<Message>> {
        let conversation = self.load(log).await?;
        let end = match exclude_message_id {
            Some(message_id) => receipts(&conversation)
                .into_iter()
                .find_map(|(after, receipt)| {
                    (receipt.message.message_id == message_id).then_some(after)
                })
                .and_then(|id| conversation.index_of(id))
                .ok_or_else(|| {
                    RuntimeError::InvalidRequest(format!(
                        "compaction exclusion message `{message_id}` is not in canonical memory"
                    ))
                })?,
            None => conversation.messages().len(),
        };
        Ok(conversation
            .messages()
            .iter()
            .take(end)
            .map(|message| message.as_ref().clone())
            .collect())
    }

    pub async fn apply_compaction(
        &self,
        log: &str,
        summary: &str,
        exclude_message_id: Option<&str>,
    ) -> RuntimeResult<SessionMessage> {
        if summary.trim().is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "compaction summary must not be empty".into(),
            ));
        }
        let mut conversation = self.load(log).await?;
        let keep_from = match exclude_message_id {
            Some(message_id) => Some(
                receipts(&conversation)
                    .into_iter()
                    .find_map(|(after, receipt)| {
                        (receipt.message.message_id == message_id).then_some(after)
                    })
                    .ok_or_else(|| {
                        RuntimeError::InvalidRequest(format!(
                            "compaction exclusion message `{message_id}` is not in canonical memory"
                        ))
                    })?,
            ),
            None => None,
        };
        let state = SummaryState {
            summary: summary.to_string(),
            ..summary_state(&conversation)
        };
        let spliced = apply_summary(&mut conversation, keep_from, &state).ok_or_else(|| {
            RuntimeError::Internal("failed to apply canonical compaction summary".into())
        })?;
        let summary_id = spliced.changed.first().copied().ok_or_else(|| {
            RuntimeError::Internal("canonical compaction did not create a summary entry".into())
        })?;
        let marker = SessionMessage {
            message_id: short_id("msg"),
            session_id: log.to_string(),
            role: MessageRole::Compaction,
            content: summary.to_string(),
            comment_refs: Vec::new(),
            attachment_ids: Vec::new(),
            run_id: None,
            created_at: Utc::now(),
        };
        let receipt = MessageReceipt {
            client_msg_id: format!("compaction:{}", short_id("msg")),
            visible: true,
            message: marker.clone(),
        };
        conversation.add_aside(
            Aside::new(
                MESSAGE_RECEIPT,
                Author::plugin("quire"),
                serde_json::to_value(receipt)?,
            )
            .after(Some(summary_id)),
        );
        self.save(log, &mut conversation).await?;
        Ok(marker)
    }

    /// Commit one runner segment against the exact history prefix it loaded.
    ///
    /// `PromptResponse::messages` excludes input history while `PromptError::Cancelled`
    /// returns canonical full history. Callers normalize both to `full_history`; this
    /// method appends only the new suffix and lets rig-memory reject concurrent writers.
    pub async fn commit(
        &self,
        log: &str,
        expected_prefix: &[Message],
        full_history: Vec<Message>,
        run_id: Option<&str>,
    ) -> RuntimeResult<()> {
        let mut conversation = self.load(log).await?;
        let current = conversation.messages();
        if current.len() != expected_prefix.len()
            || !current
                .iter()
                .map(|message| message.as_ref())
                .eq(expected_prefix.iter())
        {
            let first_mismatch = current
                .iter()
                .zip(expected_prefix.iter())
                .position(|(actual, expected)| actual.as_ref() != expected);
            return Err(RuntimeError::Internal(format!(
                "conversation `{log}` changed while the agent run was in flight: expected_prefix_len={}, current_len={}, first_mismatch={first_mismatch:?}, full_history_len={}",
                expected_prefix.len(),
                current.len(),
                full_history.len()
            )));
        }
        // A completion-hook cancellation may precede Rig's initial prompt commit. The prompt was
        // already persisted by append_prompt, so this is a valid no-op rather than a torn suffix.
        if full_history.len().saturating_add(1) == current.len()
            && full_history
                .iter()
                .eq(expected_prefix.iter().take(full_history.len()))
        {
            return Ok(());
        }
        if full_history.len() < current.len()
            || !full_history
                .iter()
                .take(current.len())
                .eq(expected_prefix.iter())
        {
            let first_mismatch = full_history
                .iter()
                .zip(expected_prefix.iter())
                .position(|(actual, expected)| actual != expected);
            return Err(RuntimeError::Internal(format!(
                "conversation `{log}` changed while the agent run was in flight: full history does not preserve the loaded prefix: expected_prefix_len={}, current_len={}, full_history_len={}, first_mismatch={first_mismatch:?}",
                expected_prefix.len(),
                current.len(),
                full_history.len()
            )));
        }
        let suffix: Vec<Message> = full_history.into_iter().skip(current.len()).collect();
        if suffix.is_empty() {
            return Ok(());
        }
        append_run_suffix(&mut conversation, suffix, log, run_id)?;
        self.save(log, &mut conversation).await
    }
}

fn assistant_text(message: &Message) -> String {
    let Message::Assistant(assistant) = message else {
        return String::new();
    };
    assistant
        .content
        .iter()
        .filter_map(|content| match content {
            AssistantContent::Text(text) if !text.text.is_empty() => Some(text.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn append_run_suffix(
    conversation: &mut Conversation,
    suffix: Vec<Message>,
    log: &str,
    run_id: Option<&str>,
) -> RuntimeResult<()> {
    let visible_text = suffix
        .iter()
        .map(assistant_text)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let receipt_anchor = (!visible_text.is_empty() && run_id.is_some())
        .then(|| {
            suffix
                .iter()
                .rposition(|message| !assistant_text(message).is_empty())
        })
        .flatten();
    for (index, message) in suffix.into_iter().enumerate() {
        let message_id = conversation.push(message);
        if receipt_anchor != Some(index) {
            continue;
        }
        let Some(run_id) = run_id else {
            continue;
        };
        let receipt = MessageReceipt {
            client_msg_id: format!("run:{run_id}:{}", short_id("msg")),
            visible: true,
            message: SessionMessage {
                message_id: short_id("msg"),
                session_id: log.to_string(),
                role: MessageRole::Assistant,
                content: visible_text.clone(),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
                run_id: Some(run_id.to_string()),
                created_at: Utc::now(),
            },
        };
        conversation.add_aside(
            Aside::new(
                MESSAGE_RECEIPT,
                Author::plugin("quire"),
                serde_json::to_value(receipt)?,
            )
            .after(Some(message_id)),
        );
    }
    Ok(())
}

fn rewind_conversation_to_prompt(
    conversation: &mut Conversation,
    session_message_id: &str,
) -> RuntimeResult<SessionMessage> {
    let (prompt_id, receipt) = receipts(conversation)
        .into_iter()
        .find(|(_, receipt)| {
            receipt.message.message_id == session_message_id
                && receipt.message.role == MessageRole::User
        })
        .ok_or_else(|| {
            RuntimeError::InvalidRequest(format!(
                "retry prompt `{session_message_id}` is not in canonical memory"
            ))
        })?;
    let prompt_index = conversation.index_of(prompt_id).ok_or_else(|| {
        RuntimeError::InvalidRequest(format!(
            "retry prompt `{session_message_id}` is no longer in canonical memory"
        ))
    })?;
    let removed: HashSet<MessageId> = conversation
        .entries()
        .skip(prompt_index.saturating_add(1))
        .map(|entry| entry.id)
        .collect();
    if !removed.is_empty() {
        let aside_ids = conversation
            .asides()
            .filter_map(|(id, aside)| {
                aside
                    .after
                    .filter(|after| removed.contains(after))
                    .map(|_| id)
            })
            .collect::<Vec<_>>();
        for id in aside_ids {
            conversation.remove_aside(id);
        }
        conversation.splice(
            prompt_index.saturating_add(1)..,
            std::iter::empty::<rig::conversation::Entry>(),
            Author::plugin("quire.retry"),
        );
    }
    Ok(receipt.message)
}

fn receipts(conversation: &Conversation) -> HashMap<MessageId, MessageReceipt> {
    conversation
        .asides()
        .filter_map(|(_, aside)| {
            if aside.kind != MESSAGE_RECEIPT {
                return None;
            }
            let after = aside.after?;
            let receipt = serde_json::from_value::<MessageReceipt>(aside.value.clone()).ok()?;
            Some((after, receipt))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::memory::{MemoryStore, load, save};

    #[test]
    fn one_run_projects_one_aggregate_nonempty_assistant_receipt() {
        let mut conversation = Conversation::from_messages([Message::user("prompt")]);
        append_run_suffix(
            &mut conversation,
            vec![
                Message::assistant("first"),
                Message::user("tool result"),
                Message::assistant("second"),
            ],
            "session",
            Some("run_1"),
        )
        .unwrap();
        let projected = receipts(&conversation);
        assert_eq!(projected.len(), 1);
        let receipt = projected.into_values().next().unwrap();
        assert_eq!(receipt.message.content, "first\n\nsecond");
        assert_eq!(receipt.message.run_id.as_deref(), Some("run_1"));

        let mut empty = Conversation::from_messages([Message::user("prompt")]);
        append_run_suffix(
            &mut empty,
            vec![Message::assistant("")],
            "session",
            Some("run_empty"),
        )
        .unwrap();
        assert!(receipts(&empty).is_empty());
    }

    #[test]
    fn retry_rewind_removes_the_failed_suffix_and_its_receipts() {
        let mut conversation = Conversation::new();
        let prompt_id = conversation.push(Message::user("prompt"));
        let prompt = SessionMessage {
            message_id: "msg_prompt".into(),
            session_id: "session".into(),
            role: MessageRole::User,
            content: "prompt".into(),
            comment_refs: Vec::new(),
            attachment_ids: Vec::new(),
            run_id: None,
            created_at: Utc::now(),
        };
        conversation.add_aside(
            Aside::new(
                MESSAGE_RECEIPT,
                Author::plugin("quire"),
                serde_json::to_value(MessageReceipt {
                    client_msg_id: "client_prompt".into(),
                    visible: true,
                    message: prompt,
                })
                .unwrap(),
            )
            .after(Some(prompt_id)),
        );
        let assistant_id = conversation.push(Message::assistant("failed answer"));
        conversation.add_aside(
            Aside::new(
                MESSAGE_RECEIPT,
                Author::plugin("quire"),
                serde_json::to_value(MessageReceipt {
                    client_msg_id: "run_failed".into(),
                    visible: true,
                    message: SessionMessage {
                        message_id: "msg_failed".into(),
                        session_id: "session".into(),
                        role: MessageRole::Assistant,
                        content: "failed answer".into(),
                        comment_refs: Vec::new(),
                        attachment_ids: Vec::new(),
                        run_id: Some("run_failed".into()),
                        created_at: Utc::now(),
                    },
                })
                .unwrap(),
            )
            .after(Some(assistant_id)),
        );
        conversation.push(Message::user("unfinished tool result"));

        let prompt = rewind_conversation_to_prompt(&mut conversation, "msg_prompt").unwrap();
        assert_eq!(prompt.message_id, "msg_prompt");
        assert_eq!(conversation.messages(), &[Message::user("prompt").into()]);
        assert_eq!(receipts(&conversation).len(), 1);
    }

    #[test]
    fn receipt_asides_survive_repeated_appends() {
        let store = MemoryStore::default();
        for index in 0..2 {
            let mut conversation = load(&store, "session").unwrap();
            let message = SessionMessage {
                message_id: format!("msg_{index}"),
                session_id: "session".into(),
                role: MessageRole::User,
                content: format!("content {index}"),
                comment_refs: Vec::new(),
                attachment_ids: Vec::new(),
                run_id: None,
                created_at: Utc::now(),
            };
            let message_id = conversation.push(Message::user(format!("content {index}")));
            let receipt = MessageReceipt {
                client_msg_id: format!("client_{index}"),
                visible: true,
                message,
            };
            conversation.add_aside(
                Aside::new(
                    MESSAGE_RECEIPT,
                    Author::plugin("quire"),
                    serde_json::to_value(receipt).unwrap(),
                )
                .after(Some(message_id)),
            );
            save(&store, "session", &mut conversation).unwrap();
            let loaded = load(&store, "session").unwrap();
            assert_eq!(loaded.messages().len(), index + 1);
        }
    }
}
