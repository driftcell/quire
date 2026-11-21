//! Run cancellation registry and cross-instance interrupt listener.

use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::bus::RedisBus;

#[derive(Clone, Default)]
pub struct RunRegistry {
    tokens: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl RunRegistry {
    pub async fn register(&self, run_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.tokens
            .lock()
            .await
            .insert(run_id.to_string(), token.clone());
        token
    }

    pub async fn cancel(&self, run_id: &str) -> bool {
        let tokens = self.tokens.lock().await;
        if let Some(token) = tokens.get(run_id) {
            token.cancel();
            true
        } else {
            false
        }
    }

    pub async fn is_cancelled(&self, run_id: &str) -> bool {
        let tokens = self.tokens.lock().await;
        tokens
            .get(run_id)
            .is_some_and(CancellationToken::is_cancelled)
    }

    pub async fn remove(&self, run_id: &str) {
        self.tokens.lock().await.remove(run_id);
    }

    pub async fn len(&self) -> usize {
        self.tokens.lock().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.tokens.lock().await.is_empty()
    }
}

/// Cancel local runs on cross-instance interrupt commands.
///
/// The listener must never die silently: a failed subscription or a dropped Pub/Sub connection
/// is retried with a short backoff, and malformed frames are skipped by the subscriber itself.
pub fn spawn_interrupt_listener(
    bus: RedisBus,
    registry: RunRegistry,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match bus.subscribe_interrupts().await {
                Ok(mut subscriber) => {
                    while let Some(command) = subscriber.next_command().await {
                        let _ = registry.cancel(&command.run_id).await;
                    }
                    warn!("interrupt subscription ended; resubscribing");
                }
                Err(error) => {
                    warn!(%error, "failed to subscribe to runtime interrupts; retrying");
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
}
