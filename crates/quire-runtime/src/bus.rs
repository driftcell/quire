//! Redis Pub/Sub transport for live Session-SSE, interrupts and cache invalidation.

use futures::StreamExt;
use quire_session_protocol::SessionEvent;
use redis::{AsyncCommands, Client, aio::PubSub};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::error::RuntimeResult;

const INTERRUPT_CHANNEL: &str = "runtime:interrupt";
const KB_INVALIDATE_CHANNEL: &str = "runtime:kb:invalidate";
const MODEL_INVALIDATE_CHANNEL: &str = "runtime:models:invalidate";

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

async fn next_json<T: DeserializeOwned>(pubsub: &mut PubSub) -> Option<T> {
    let message = pubsub.on_message().next().await?;
    let payload: String = message.get_payload().ok()?;
    serde_json::from_str(&payload).ok()
}
