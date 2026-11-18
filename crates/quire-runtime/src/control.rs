//! Run cancellation registry and cross-instance interrupt listener.

use std::{collections::HashMap, sync::Arc};

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

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

pub fn spawn_interrupt_listener(
    bus: RedisBus,
    registry: RunRegistry,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let Ok(mut subscriber) = bus.subscribe_interrupts().await else {
            return;
        };
        while let Some(command) = subscriber.next_command().await {
            let _ = registry.cancel(&command.run_id).await;
        }
    })
}
