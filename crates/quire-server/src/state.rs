//! Shared server state and request metadata helpers.

use std::sync::Arc;

use quire_store::PgReportStore;

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<PgReportStore>,
    pub config: Arc<Config>,
}

impl AppState {
    pub fn new(store: PgReportStore, config: Config) -> Self {
        Self {
            store: Arc::new(store),
            config: Arc::new(config),
        }
    }
}
