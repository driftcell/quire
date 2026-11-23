//! Knowledge-base, agent-definition and skill management with Git-style branches, immutable
//! snapshots and moka caching.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

use chrono::{DateTime, Utc};
use moka::future::Cache;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgConnection, Row};
use tracing::warn;

use crate::{
    error::{RuntimeError, RuntimeResult},
    store::RuntimeStore,
};

mod agents;
mod branch;
mod documents;
mod records;
mod seed;
mod skills;
mod store;

pub use records::*;
pub(crate) use store::*;

#[derive(Clone)]
pub struct KbService {
    store: RuntimeStore,
    prompt_cache: Cache<String, Arc<String>>,
}

impl KbService {
    pub fn new(store: RuntimeStore) -> Self {
        Self {
            store,
            prompt_cache: Cache::new(64),
        }
    }

    pub fn store(&self) -> &RuntimeStore {
        &self.store
    }

    pub async fn current_version(&self) -> RuntimeResult<u64> {
        self.store.kb_version().await
    }
}
