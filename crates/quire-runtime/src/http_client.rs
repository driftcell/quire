//! Shared HTTP client construction with bounded connect and request timeouts.

use std::time::Duration;

use reqwest::Client;

pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Build a reqwest client with a 15-second connect timeout and a caller-defined
/// total request timeout. The fallback only covers an impossible builder
/// failure so a single malformed TLS environment cannot crash the runtime.
pub(crate) fn timed_client(timeout: Duration) -> Client {
    Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(timeout)
        .build()
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "failed to build timed HTTP client; using reqwest defaults");
            Client::new()
        })
}
