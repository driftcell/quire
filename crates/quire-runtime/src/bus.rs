//! Redis Pub/Sub transport for live Session-SSE, interrupts and cache invalidation.

use std::{future::Future, pin::Pin, time::Duration};

use futures::StreamExt;
use quire_session_protocol::SessionEvent;
use redis::{AsyncCommands, Client, aio::PubSub};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::task::JoinHandle;
use tracing::warn;

use crate::{error::RuntimeResult, preview::ChartPreviewRecord};

const INTERRUPT_CHANNEL: &str = "runtime:interrupt";
const KB_INVALIDATE_CHANNEL: &str = "runtime:kb:invalidate";
const MODEL_INVALIDATE_CHANNEL: &str = "runtime:models:invalidate";
const MCP_INVALIDATE_CHANNEL: &str = "runtime:mcp:invalidate";
const CHART_PREVIEW_PREFIX: &str = "quire:chart_preview:";
const CHART_PREVIEW_TTL_SECONDS: u64 = 1800;
const HOT_EVENTS_TTL_SECONDS: u64 = 86_400;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InterruptCommand {
    pub session_id: String,
    pub run_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KbInvalidate {
    pub version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelInvalidate {
    pub version: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpInvalidate {
    pub version: u64,
}

#[derive(Clone)]
pub struct RedisBus {
    client: Client,
}

impl RedisBus {
    pub fn connect(redis_url: &str) -> RuntimeResult<Self> {
        Ok(Self {
            client: Client::open(redis_url)?,
        })
    }

    pub fn event_channel(session_id: &str) -> String {
        format!("runtime:session:{session_id}:events")
    }

    pub async fn publish_event(&self, event: &SessionEvent, session_id: &str) -> RuntimeResult<()> {
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        let payload = serde_json::to_string(event)?;
        let _: i64 = connection
            .publish(Self::event_channel(session_id), payload)
            .await?;
        Ok(())
    }

    pub async fn subscribe_events(&self, session_id: &str) -> RuntimeResult<EventSubscriber> {
        let mut pubsub = self.client.get_async_pubsub().await?;
        pubsub.subscribe(Self::event_channel(session_id)).await?;
        Ok(EventSubscriber { pubsub })
    }

    fn hot_seq_key(session_id: &str) -> String {
        format!("runtime:session:{session_id}:hot_seq")
    }

    fn hot_events_key(session_id: &str) -> String {
        format!("runtime:session:{session_id}:hot_events")
    }

    /// Atomically allocate a session sequence, retain the hot projection and publish it.
    pub async fn append_session_event(
        &self,
        session_id: &str,
        run_id: &str,
        agent_id: &str,
        parent_run_id: Option<String>,
        kind: quire_session_protocol::SessionEventKind,
    ) -> RuntimeResult<SessionEvent> {
        let event = SessionEvent::new(0, run_id, agent_id, parent_run_id, kind);
        let payload = serde_json::to_string(&event)?;
        let script = redis::Script::new(
            r#"
            local seq = redis.call('INCR', KEYS[1])
            local payload = string.gsub(ARGV[1], '"seq":0', '"seq":' .. seq, 1)
            redis.call('RPUSH', KEYS[2], payload)
            redis.call('EXPIRE', KEYS[2], ARGV[2])
            redis.call('PUBLISH', KEYS[3], payload)
            return payload
            "#,
        );
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        let payload: String = script
            .key(Self::hot_seq_key(session_id))
            .key(Self::hot_events_key(session_id))
            .key(Self::event_channel(session_id))
            .arg(payload)
            .arg(HOT_EVENTS_TTL_SECONDS)
            .invoke_async(&mut connection)
            .await?;
        Ok(serde_json::from_str(&payload)?)
    }

    pub async fn current_event_seq(&self, session_id: &str) -> RuntimeResult<u64> {
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        Ok(connection
            .get::<_, Option<u64>>(Self::hot_seq_key(session_id))
            .await?
            .unwrap_or(0))
    }

    pub async fn current_event_seqs(&self, session_ids: &[String]) -> RuntimeResult<Vec<u64>> {
        if session_ids.is_empty() {
            return Ok(Vec::new());
        }
        let keys: Vec<String> = session_ids.iter().map(|id| Self::hot_seq_key(id)).collect();
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        let values: Vec<Option<u64>> = connection.mget(keys).await?;
        Ok(values.into_iter().map(|seq| seq.unwrap_or(0)).collect())
    }

    pub async fn hot_event_payloads(&self, session_id: &str) -> RuntimeResult<Vec<String>> {
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        Ok(connection
            .lrange(Self::hot_events_key(session_id), 0, -1)
            .await?)
    }

    pub async fn hot_events(&self, session_id: &str) -> RuntimeResult<Vec<SessionEvent>> {
        let frames = self.hot_event_payloads(session_id).await?;
        Ok(frames
            .iter()
            .filter_map(|payload| match serde_json::from_str(payload) {
                Ok(event) => Some(event),
                Err(error) => {
                    warn!(%error, session_id, "discarding malformed hot session event");
                    None
                }
            })
            .collect())
    }

    pub async fn clear_hot_events(&self, session_id: &str) -> RuntimeResult<()> {
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        let _: i64 = connection.del(Self::hot_events_key(session_id)).await?;
        Ok(())
    }

    pub async fn trim_hot_events(&self, session_id: &str, min_seq: u64) -> RuntimeResult<u64> {
        let script = redis::Script::new(
            r#"
            local frames = redis.call('LRANGE', KEYS[1], 0, -1)
            local keep = {}
            for _, payload in ipairs(frames) do
              local event = cjson.decode(payload)
              if tonumber(event['seq']) >= tonumber(ARGV[1]) then
                table.insert(keep, payload)
              end
            end
            if #keep == #frames then
              if #keep > 0 then
                redis.call('EXPIRE', KEYS[1], ARGV[2])
              end
              return 0
            end
            redis.call('DEL', KEYS[1])
            if #keep > 0 then
              for i = 1, #keep, 1000 do
                local j = math.min(i + 999, #keep)
                redis.call('RPUSH', KEYS[1], unpack(keep, i, j))
              end
              redis.call('EXPIRE', KEYS[1], ARGV[2])
            end
            return #frames - #keep
            "#,
        );
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        Ok(script
            .key(Self::hot_events_key(session_id))
            .arg(min_seq)
            .arg(HOT_EVENTS_TTL_SECONDS)
            .invoke_async(&mut connection)
            .await?)
    }

    pub async fn delete_session_hot_state(&self, session_id: &str) -> RuntimeResult<()> {
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        let _: i64 = connection.del(Self::hot_events_key(session_id)).await?;
        let _: i64 = connection.del(Self::hot_seq_key(session_id)).await?;
        Ok(())
    }

    pub async fn publish_interrupt(&self, command: &InterruptCommand) -> RuntimeResult<()> {
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        let payload = serde_json::to_string(command)?;
        let _: i64 = connection.publish(INTERRUPT_CHANNEL, payload).await?;
        Ok(())
    }

    pub async fn subscribe_interrupts(&self) -> RuntimeResult<InterruptSubscriber> {
        let mut pubsub = self.client.get_async_pubsub().await?;
        pubsub.subscribe(INTERRUPT_CHANNEL).await?;
        Ok(InterruptSubscriber { pubsub })
    }

    pub async fn publish_kb_invalidation(&self, version: u64) -> RuntimeResult<()> {
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        let payload = serde_json::to_string(&KbInvalidate { version })?;
        let _: i64 = connection.publish(KB_INVALIDATE_CHANNEL, payload).await?;
        Ok(())
    }

    pub async fn subscribe_kb_invalidation(&self) -> RuntimeResult<KbInvalidateSubscriber> {
        let mut pubsub = self.client.get_async_pubsub().await?;
        pubsub.subscribe(KB_INVALIDATE_CHANNEL).await?;
        Ok(KbInvalidateSubscriber { pubsub })
    }

    pub async fn publish_model_invalidation(&self, version: u64) -> RuntimeResult<()> {
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        let payload = serde_json::to_string(&ModelInvalidate { version })?;
        let _: i64 = connection
            .publish(MODEL_INVALIDATE_CHANNEL, payload)
            .await?;
        Ok(())
    }

    pub async fn subscribe_model_invalidation(&self) -> RuntimeResult<ModelInvalidateSubscriber> {
        let mut pubsub = self.client.get_async_pubsub().await?;
        pubsub.subscribe(MODEL_INVALIDATE_CHANNEL).await?;
        Ok(ModelInvalidateSubscriber { pubsub })
    }

    pub async fn publish_mcp_invalidation(&self, version: u64) -> RuntimeResult<()> {
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        let payload = serde_json::to_string(&McpInvalidate { version })?;
        let _: i64 = connection.publish(MCP_INVALIDATE_CHANNEL, payload).await?;
        Ok(())
    }

    pub async fn subscribe_mcp_invalidation(&self) -> RuntimeResult<McpInvalidateSubscriber> {
        let mut pubsub = self.client.get_async_pubsub().await?;
        pubsub.subscribe(MCP_INVALIDATE_CHANNEL).await?;
        Ok(McpInvalidateSubscriber { pubsub })
    }

    pub fn kb_invalidation_subscription(&self) -> SubscriptionFuture<KbInvalidateSubscriber> {
        let bus = self.clone();
        Box::pin(async move { bus.subscribe_kb_invalidation().await })
    }

    pub fn model_invalidation_subscription(&self) -> SubscriptionFuture<ModelInvalidateSubscriber> {
        let bus = self.clone();
        Box::pin(async move { bus.subscribe_model_invalidation().await })
    }

    pub fn mcp_invalidation_subscription(&self) -> SubscriptionFuture<McpInvalidateSubscriber> {
        let bus = self.clone();
        Box::pin(async move { bus.subscribe_mcp_invalidation().await })
    }

    pub fn chart_preview_key(preview_id: &str) -> String {
        format!("{CHART_PREVIEW_PREFIX}{}", preview_id.trim())
    }

    /// Store one single-chart preview for 30 minutes.
    pub async fn store_chart_preview(&self, preview: &ChartPreviewRecord) -> RuntimeResult<()> {
        if preview.preview_id.trim().is_empty() {
            return Err(crate::error::RuntimeError::InvalidRequest(
                "chart preview_id must not be empty".into(),
            ));
        }
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        let payload = serde_json::to_string(preview)?;
        let _: () = connection
            .set_ex(
                Self::chart_preview_key(&preview.preview_id),
                payload,
                CHART_PREVIEW_TTL_SECONDS,
            )
            .await?;
        Ok(())
    }

    /// Load a single-chart preview. Returns `None` when it never existed or the 30-minute TTL
    /// has expired.
    pub async fn load_chart_preview(
        &self,
        preview_id: &str,
    ) -> RuntimeResult<Option<ChartPreviewRecord>> {
        let preview_id = preview_id.trim();
        if preview_id.is_empty() {
            return Ok(None);
        }
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        let payload: Option<String> = connection.get(Self::chart_preview_key(preview_id)).await?;
        payload
            .map(|payload| serde_json::from_str(&payload).map_err(Into::into))
            .transpose()
    }

    /// Remove a chart preview when it is no longer needed.
    pub async fn delete_chart_preview(&self, preview_id: &str) -> RuntimeResult<()> {
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        let _: i64 = connection.del(Self::chart_preview_key(preview_id)).await?;
        Ok(())
    }

    pub async fn health(&self) -> RuntimeResult<()> {
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        let _: String = redis::cmd("PING").query_async(&mut connection).await?;
        Ok(())
    }
}

pub type SubscriptionFuture<S> = Pin<Box<dyn Future<Output = RuntimeResult<S>> + Send + 'static>>;

pub type InvalidationCallback =
    Box<dyn FnMut() -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> + Send + 'static>;

/// A unit-valued cache-invalidation subscription.
pub trait InvalidationSubscriber: Send + 'static {
    fn next_notification(&mut self) -> impl Future<Output = Option<()>> + Send;
}

/// Keep a cache-invalidation subscription alive across Redis failures.
///
/// A malformed frame is handled inside the subscriber and never reaches this loop. A ended or
/// failed subscription is retried with backoff; after a reconnect the callback runs once even
/// without a frame, because invalidations published during the gap cannot be replayed.
pub fn spawn_resilient_invalidation_listener<S, F>(
    bus: RedisBus,
    mut subscribe: F,
    mut on_invalidate: InvalidationCallback,
) -> JoinHandle<()>
where
    S: InvalidationSubscriber,
    F: for<'a> FnMut(&'a RedisBus) -> SubscriptionFuture<S> + Send + 'static,
{
    tokio::spawn(async move {
        let mut was_connected = false;
        loop {
            match subscribe(&bus).await {
                Ok(mut subscriber) => {
                    if was_connected {
                        on_invalidate().await;
                    }
                    was_connected = true;
                    while subscriber.next_notification().await.is_some() {
                        on_invalidate().await;
                    }
                    warn!("cache invalidation subscription ended; resubscribing");
                }
                Err(error) => {
                    warn!(%error, "failed to subscribe to cache invalidation; retrying");
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
}

pub struct EventSubscriber {
    pubsub: PubSub,
}

impl EventSubscriber {
    pub async fn next_event(&mut self) -> Option<SessionEvent> {
        next_json(&mut self.pubsub).await
    }
}

pub struct InterruptSubscriber {
    pubsub: PubSub,
}

impl InterruptSubscriber {
    pub async fn next_command(&mut self) -> Option<InterruptCommand> {
        next_json(&mut self.pubsub).await
    }
}

pub struct KbInvalidateSubscriber {
    pubsub: PubSub,
}

impl KbInvalidateSubscriber {
    pub async fn next_invalidation(&mut self) -> Option<KbInvalidate> {
        next_json(&mut self.pubsub).await
    }
}

impl InvalidationSubscriber for KbInvalidateSubscriber {
    async fn next_notification(&mut self) -> Option<()> {
        self.next_invalidation().await.map(|_| ())
    }
}

pub struct ModelInvalidateSubscriber {
    pubsub: PubSub,
}

impl ModelInvalidateSubscriber {
    pub async fn next_invalidation(&mut self) -> Option<ModelInvalidate> {
        next_json(&mut self.pubsub).await
    }
}

impl InvalidationSubscriber for ModelInvalidateSubscriber {
    async fn next_notification(&mut self) -> Option<()> {
        self.next_invalidation().await.map(|_| ())
    }
}

pub struct McpInvalidateSubscriber {
    pubsub: PubSub,
}

impl McpInvalidateSubscriber {
    pub async fn next_invalidation(&mut self) -> Option<McpInvalidate> {
        next_json(&mut self.pubsub).await
    }
}

impl InvalidationSubscriber for McpInvalidateSubscriber {
    async fn next_notification(&mut self) -> Option<()> {
        self.next_invalidation().await.map(|_| ())
    }
}

/// Decode one Pub/Sub payload, rejecting malformed frames without terminating the subscription.
fn decode_frame<T: DeserializeOwned>(payload: &str) -> Option<T> {
    match serde_json::from_str(payload) {
        Ok(value) => Some(value),
        Err(error) => {
            warn!(%error, "discarding malformed Redis Pub/Sub frame");
            None
        }
    }
}

/// Yield the next decodable message. Malformed frames are skipped so one bad payload cannot
/// close an SSE stream or permanently stop a control listener; `None` means the subscription
/// itself ended and the caller should resubscribe.
async fn next_json<T: DeserializeOwned>(pubsub: &mut PubSub) -> Option<T> {
    loop {
        let message = pubsub.on_message().next().await?;
        let payload: String = match message.get_payload() {
            Ok(payload) => payload,
            Err(error) => {
                warn!(%error, "discarding Redis Pub/Sub frame with a non-string payload");
                continue;
            }
        };
        if let Some(value) = decode_frame(&payload) {
            return Some(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_frames_are_rejected_without_ending_the_stream() {
        let invalid: Option<InterruptCommand> = decode_frame("not json");
        assert!(invalid.is_none());

        let decoded: Option<InterruptCommand> =
            decode_frame("{\"session_id\":\"sess_1\",\"run_id\":\"run_1\"}");
        assert_eq!(
            decoded,
            Some(InterruptCommand {
                session_id: "sess_1".into(),
                run_id: "run_1".into(),
            })
        );

        let invalid_event: Option<SessionEvent> = decode_frame("{}");
        assert!(invalid_event.is_none());
    }

    #[test]
    fn chart_preview_keys_use_the_documented_namespace() {
        assert_eq!(
            RedisBus::chart_preview_key("chart_abc"),
            "quire:chart_preview:chart_abc"
        );
        assert_eq!(CHART_PREVIEW_TTL_SECONDS, 1800);
    }
}

const OSS_PRESIGN_PREFIX: &str = "quire:oss_presign:";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OssPresignTicket {
    pub session_id: String,
    pub attachment_id: String,
    pub object_key: String,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: u64,
}

impl RedisBus {
    fn oss_presign_key(session_id: &str, object_key: &str) -> String {
        format!("{OSS_PRESIGN_PREFIX}{session_id}:{object_key}")
    }

    pub async fn store_oss_presign_ticket(
        &self,
        ticket: &OssPresignTicket,
        ttl_secs: u64,
    ) -> RuntimeResult<()> {
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        let payload = serde_json::to_string(ticket)?;
        let _: () = connection
            .set_ex(
                Self::oss_presign_key(&ticket.session_id, &ticket.object_key),
                payload,
                ttl_secs.max(60),
            )
            .await?;
        Ok(())
    }

    pub async fn take_oss_presign_ticket(
        &self,
        session_id: &str,
        object_key: &str,
    ) -> RuntimeResult<Option<OssPresignTicket>> {
        let mut connection = self.client.get_multiplexed_async_connection().await?;
        let key = Self::oss_presign_key(session_id, object_key);
        let raw: Option<String> = connection.get(&key).await?;
        if raw.is_some() {
            let _: i64 = connection.del(&key).await?;
        }
        raw.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
}
