//! Unified provider/model registry, secret references and models.dev import.

mod records;
mod registry;
mod secrets;

pub(crate) use records::model_http_client;
pub use records::*;
pub use secrets::{mask_secret_ref, normalize_secret_ref, resolve_api_key_ref};
pub(crate) use secrets::{normalize_api_key_ref, row_to_model, row_to_provider};
