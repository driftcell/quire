//! Redis Streams publisher and readers.

use chrono::{DateTime, Utc};
use futures::StreamExt;
use quire_protocol::{DocId, Revision, SessionId};
use redis::{
    AsyncCommands, Client,
    aio::{ConnectionManager, MultiplexedConnection, PubSub},
    streams::{StreamRangeReply, StreamReadOptions, StreamReadReply},
};
use serde_json::Value;

use crate::{
    error::StoreError,
    types::{RevisionEvent, SessionEvent, SessionStreamEntry, StreamRevision},
};

const REVISION_STREAM_MAXLEN: usize = 1000;
const SESSION_STREAM_MAXLEN: usize = 200;

#[derive(Clone)]
pub struct RedisStreamBus {
    client: Client,
    manager: ConnectionManager,
}

impl RedisStreamBus {
    pub async fn connect(redis_url: &str) -> Result<Self, StoreError> {
        let client = Client::open(redis_url)?;
        let manager = ConnectionManager::new(client.clone()).await?;
        Ok(Self { client, manager })
    }

    pub async fn health(&self) -> Result<(), StoreError> {
        let mut connection = self.manager.clone();
        let _: String = redis::cmd("PING").query_async(&mut connection).await?;
        Ok(())
    }

    pub fn revision_stream_key(doc_id: &str) -> String {
        format!("stream:doc:{doc_id}")
    }

    pub fn session_stream_key(session_id: &str) -> String {
        format!("stream:session:{session_id}")
    }

    pub fn ephemeral_channel(doc_id: &str) -> String {
        format!("ephemeral:doc:{doc_id}")
    }

    pub async fn publish_revision(&self, event: &RevisionEvent) -> Result<(), StoreError> {
        let key = Self::revision_stream_key(event.doc_id.as_str());
        let payload = serde_json::to_string(&event.ops)?;
        let cleared_preview_ids = serde_json::to_string(&event.cleared_preview_ids)?;
        let created_at = event.created_at.to_rfc3339();
        let mut connection = self.manager.clone();
        redis::cmd("XADD")
            .arg(&key)
            .arg("MAXLEN")
            .arg("~")
            .arg(REVISION_STREAM_MAXLEN)
            .arg("*")
            .arg("doc_id")
            .arg(event.doc_id.as_str())
            .arg("rev")
            .arg(event.revision.get())
            .arg("actor")
            .arg(&event.actor)
            .arg("summary")
            .arg(&event.summary)
            .arg("payload")
            .arg(payload)
            .arg("cleared_preview_ids")
            .arg(cleared_preview_ids)
            .arg("created_at")
            .arg(created_at)
            .query_async::<String>(&mut connection)
            .await?;
        Ok(())
    }

    pub async fn publish_session_event(&self, event: &SessionEvent) -> Result<(), StoreError> {
        let key = Self::session_stream_key(event.session_id.as_str());
        let mut connection = self.manager.clone();
        redis::cmd("XADD")
            .arg(&key)
            .arg("MAXLEN")
            .arg("~")
            .arg(SESSION_STREAM_MAXLEN)
            .arg("*")
            .arg("event")
            .arg(&event.event)
            .arg("doc_id")
            .arg(event.doc_id.as_str())
            .arg("title")
            .arg(&event.title)
            .arg("rev")
            .arg(event.revision.get())
            .arg("created_at")
            .arg(event.created_at.to_rfc3339())
            .query_async::<String>(&mut connection)
            .await?;
        Ok(())
    }

    pub async fn publish_ephemeral(
        &self,
        doc_id: &str,
        payload: &impl serde::Serialize,
    ) -> Result<(), StoreError> {
        let mut connection = self.manager.clone();
        let channel = Self::ephemeral_channel(doc_id);
        let body = serde_json::to_string(payload)?;
        let _: i64 = connection.publish(channel, body).await?;
        Ok(())
    }

    /// Read the latest session discovery entries (oldest first, bounded by the stream MAXLEN).
    pub async fn read_session_events(
        &self,
        session_id: &str,
    ) -> Result<Vec<SessionStreamEntry>, StoreError> {
        let key = Self::session_stream_key(session_id);
        let mut connection = self.manager.clone();
        let reply: StreamRangeReply = connection.xrange_count(&key, "-", "+", 100).await?;
        let mut events = Vec::new();
        for entry in reply.ids {
            if let Some(mut event) = parse_session_entry(entry) {
                event.event.session_id = SessionId::parse(session_id)?;
                events.push(event);
            }
        }
        events.sort_by_key(|left| left.event.revision);
        Ok(events)
    }

    pub async fn read_revisions(&self, doc_id: &str) -> Result<Vec<StreamRevision>, StoreError> {
        let key = Self::revision_stream_key(doc_id);
        let mut connection = self.manager.clone();
        let reply: StreamRangeReply = connection.xrange_count(&key, "-", "+", 1000).await?;
        let mut revisions = reply
            .ids
            .into_iter()
            .filter_map(parse_revision_entry)
            .collect::<Vec<_>>();
        revisions.sort_by_key(|revision| revision.event.revision);
        Ok(revisions)
    }

    /// Latest Redis Stream entry ID, or `0-0` when the stream is empty.
    pub async fn latest_revision_stream_id(&self, doc_id: &str) -> Result<String, StoreError> {
        let key = Self::revision_stream_key(doc_id);
        let mut connection = self.manager.clone();
        let reply: StreamRangeReply = connection.xrevrange_count(&key, "+", "-", 1).await?;
        Ok(reply
            .ids
            .first()
            .map(|entry| entry.id.clone())
            .unwrap_or_else(|| "0-0".to_string()))
    }

    pub async fn latest_session_stream_id(&self, session_id: &str) -> Result<String, StoreError> {
        let key = Self::session_stream_key(session_id);
        let mut connection = self.manager.clone();
        let reply: StreamRangeReply = connection.xrevrange_count(&key, "+", "-", 1).await?;
        Ok(reply
            .ids
            .first()
            .map(|entry| entry.id.clone())
            .unwrap_or_else(|| "0-0".to_string()))
    }

    pub async fn blocking_reader(&self) -> Result<StreamReader, StoreError> {
        // XREAD BLOCK holds the response for up to the block timeout, so the response timeout
        // must be disabled on this dedicated connection.
        let config = redis::AsyncConnectionConfig::new().set_response_timeout(None);
        let connection = self
            .client
            .get_multiplexed_async_connection_with_config(&config)
            .await?;
        Ok(StreamReader { connection })
    }

    pub async fn subscribe_ephemeral(
        &self,
        doc_id: &str,
    ) -> Result<EphemeralSubscriber, StoreError> {
        let mut pubsub = self.client.get_async_pubsub().await?;
        pubsub.subscribe(Self::ephemeral_channel(doc_id)).await?;
        Ok(EphemeralSubscriber { pubsub })
    }
}

/// Dedicated connection for blocking XREAD calls (one per SSE connection).
pub struct StreamReader {
    connection: MultiplexedConnection,
}

impl StreamReader {
    pub async fn read_revisions(
        &mut self,
        doc_id: &str,
        last_stream_id: &str,
        block_ms: usize,
    ) -> Result<Vec<StreamRevision>, StoreError> {
        let key = RedisStreamBus::revision_stream_key(doc_id);
        let options = StreamReadOptions::default().block(block_ms).count(100);
        let reply: Option<StreamReadReply> = self
            .connection
            .xread_options(&[&key], &[last_stream_id], &options)
            .await?;
        Ok(reply
            .into_iter()
            .flat_map(|reply| reply.keys)
            .flat_map(|key| key.ids)
            .filter_map(parse_revision_entry)
            .collect())
    }

    pub async fn read_session_events(
        &mut self,
        session_id: &str,
        last_stream_id: &str,
        block_ms: usize,
    ) -> Result<Vec<SessionStreamEntry>, StoreError> {
        let key = RedisStreamBus::session_stream_key(session_id);
        let options = StreamReadOptions::default().block(block_ms).count(100);
        let reply: Option<StreamReadReply> = self
            .connection
            .xread_options(&[&key], &[last_stream_id], &options)
            .await?;
        let mut events: Vec<SessionStreamEntry> = reply
            .into_iter()
            .flat_map(|reply| reply.keys)
            .flat_map(|key| key.ids)
            .filter_map(parse_session_entry)
            .collect();
        let typed_session = SessionId::parse(session_id)?;
        for event in &mut events {
            event.event.session_id = typed_session.clone();
        }
        Ok(events)
    }
}

pub struct EphemeralSubscriber {
    pubsub: PubSub,
}

impl EphemeralSubscriber {
    pub async fn next_payload(&mut self) -> Option<String> {
        let message = self.pubsub.on_message().next().await?;
        message.get_payload::<String>().ok()
    }
}

fn parse_revision_entry(entry: redis::streams::StreamId) -> Option<StreamRevision> {
    let revision = Revision::new(entry.get("rev")?).ok()?;
    let actor: String = entry.get("actor").unwrap_or_default();
    let summary: String = entry.get("summary").unwrap_or_default();
    let payload: String = entry.get("payload").unwrap_or_else(|| "[]".to_string());
    let ops = serde_json::from_str::<Value>(&payload).unwrap_or(Value::Array(Vec::new()));
    let cleared_preview_ids = entry
        .get::<String>("cleared_preview_ids")
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
        .unwrap_or_default();
    let created_at = entry
        .get::<String>("created_at")
        .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);
    let doc_id = DocId::parse(entry.get::<String>("doc_id").unwrap_or_default()).ok()?;
    let event = RevisionEvent {
        doc_id,
        revision,
        actor,
        summary,
        ops,
        cleared_preview_ids,
        created_at,
    };
    Some(StreamRevision {
        stream_id: entry.id,
        event,
    })
}

fn parse_session_entry(entry: redis::streams::StreamId) -> Option<SessionStreamEntry> {
    let event_name: String = entry.get("event")?;
    let doc_id = DocId::parse(entry.get::<String>("doc_id")?).ok()?;
    let title: String = entry.get("title").unwrap_or_default();
    let revision = Revision::new(entry.get("rev").unwrap_or(0)).ok()?;
    let created_at = entry
        .get::<String>("created_at")
        .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);
    // session_id is encoded by the stream key; the transport layer fills it from the URL.
    Some(SessionStreamEntry {
        stream_id: entry.id,
        event: SessionEvent {
            event: event_name,
            session_id: SessionId::parse("unknown").ok()?,
            doc_id,
            title,
            revision,
            created_at,
        },
    })
}
