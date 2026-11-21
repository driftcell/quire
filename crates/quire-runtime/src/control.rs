//! Run cancellation registry and cross-instance interrupt listener.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::bus::RedisBus;

#[derive(Clone, Default)]
pub struct RunRegistry {
    tokens: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl RunRegistry {
    fn lock_tokens(&self) -> MutexGuard<'_, HashMap<String, CancellationToken>> {
        match self.tokens.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    pub async fn register(&self, run_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.lock_tokens().insert(run_id.to_string(), token.clone());
        token
    }

    pub async fn cancel(&self, run_id: &str) -> bool {
        let tokens = self.lock_tokens();
        if let Some(token) = tokens.get(run_id) {
            token.cancel();
            true
        } else {
            false
        }
    }

    pub async fn is_cancelled(&self, run_id: &str) -> bool {
        self.lock_tokens()
            .get(run_id)
            .is_some_and(CancellationToken::is_cancelled)
    }

    pub async fn remove(&self, run_id: &str) {
        self.lock_tokens().remove(run_id);
    }

    fn remove_now(&self, run_id: &str) {
        self.lock_tokens().remove(run_id);
    }

    pub async fn len(&self) -> usize {
        self.lock_tokens().len()
    }

    pub async fn is_empty(&self) -> bool {
        self.lock_tokens().is_empty()
    }
}

/// RAII owner of one run's cancellation token and registry entry.
///
/// Dropping the guard always cancels and unregisters the run, including panic
/// and early-return paths. The provider stream's own abort-on-drop guard covers
/// the producer task itself.
pub struct RunTurnGuard {
    run_id: String,
    token: CancellationToken,
    registry: RunRegistry,
}

impl RunTurnGuard {
    pub fn new(run_id: impl Into<String>, token: CancellationToken, registry: RunRegistry) -> Self {
        Self {
            run_id: run_id.into(),
            token,
            registry,
        }
    }

    pub fn token(&self) -> &CancellationToken {
        &self.token
    }
}

impl Drop for RunTurnGuard {
    fn drop(&mut self) {
        self.token.cancel();
        self.registry.remove_now(&self.run_id);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dropping_a_run_turn_guard_cancels_and_unregisters() {
        let registry = RunRegistry::default();
        let token = registry.register("run_guard").await;
        assert!(!registry.is_empty().await);

        let guard = RunTurnGuard::new("run_guard", token.clone(), registry.clone());
        assert!(!guard.token().is_cancelled());
        drop(guard);

        assert!(token.is_cancelled());
        assert!(registry.is_empty().await);
    }
}
