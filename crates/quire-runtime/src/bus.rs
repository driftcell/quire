//! Redis Pub/Sub transport for live Session-SSE, interrupts and cache invalidation.

use std::{future::Future, pin::Pin, time::Duration};

use async_trait::async_trait;
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
#[async_trait]
pub trait InvalidationSubscriber: Send + 'static {
    async fn next_notification(&mut self) -> Option<()>;
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

#[async_trait]
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

#[async_trait]
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

#[async_trait]
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
