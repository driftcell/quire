//! Canonical structured conversation history backed by Rig's official PostgreSQL memory store.

use std::collections::HashMap;

use chrono::Utc;
use quire_protocol::short_id;
use quire_session_protocol::{MessageRole, SendMessageRequest, SessionMessage};
use rig_core::{
    conversation::{Aside, Author, Conversation, MessageId},
    message::{AssistantContent, Message},
};
use rig_memory::{SummaryState, apply_summary, load_async, save_async, summary_state};
use rig_postgres::PostgresSessionStore;
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
            || full_history.len() < current.len()
            || !full_history
                .iter()
                .take(current.len())
                .eq(expected_prefix.iter())
        {
            return Err(RuntimeError::Internal(format!(
                "conversation `{log}` changed while the agent run was in flight"
            )));
        }
        let prefix_len = current.len();
        let suffix: Vec<Message> = full_history.into_iter().skip(prefix_len).collect();
        for message in suffix {
            let message_id = conversation.push(message.clone());
            let Some(run_id) = run_id else {
                continue;
            };
            let Message::Assistant(assistant) = &message else {
                continue;
            };
            let text = assistant
                .content
                .iter()
                .filter_map(|content| match content {
                    AssistantContent::Text(text) if !text.text.is_empty() => {
                        Some(text.text.as_str())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            if text.is_empty() {
                continue;
            }
            let receipt = MessageReceipt {
                client_msg_id: format!("run:{run_id}:{}", short_id("msg")),
                visible: true,
                message: SessionMessage {
                    message_id: short_id("msg"),
                    session_id: log.to_string(),
                    role: MessageRole::Assistant,
                    content: text,
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
        self.save(log, &mut conversation).await
    }
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
    use rig_memory::{MemoryStore, load, save};

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
