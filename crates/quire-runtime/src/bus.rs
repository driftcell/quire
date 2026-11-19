//! Redis Pub/Sub transport for live Session-SSE, interrupts and cache invalidation.

use futures::StreamExt;
use quire_session_protocol::SessionEvent;
use redis::{AsyncCommands, Client, aio::PubSub};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

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

pub struct EventSubscriber {
    pubsub: PubSub,
}

impl EventSubscriber {
    pub async fn next_event(&mut self) -> Option<SessionEvent> {
        let message = self.pubsub.on_message().next().await?;
        let payload: String = message.get_payload().ok()?;
        serde_json::from_str(&payload).ok()
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

pub struct ModelInvalidateSubscriber {
    pubsub: PubSub,
}

impl ModelInvalidateSubscriber {
    pub async fn next_invalidation(&mut self) -> Option<ModelInvalidate> {
        next_json(&mut self.pubsub).await
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

async fn next_json<T: DeserializeOwned>(pubsub: &mut PubSub) -> Option<T> {
    let message = pubsub.on_message().next().await?;
    let payload: String = message.get_payload().ok()?;
    serde_json::from_str(&payload).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_preview_keys_use_the_documented_namespace() {
        assert_eq!(
            RedisBus::chart_preview_key("chart_abc"),
            "quire:chart_preview:chart_abc"
        );
        assert_eq!(CHART_PREVIEW_TTL_SECONDS, 1800);
    }
}
