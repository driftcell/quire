//! Provider/model records and the shared HTTP client.

use std::{
    sync::{Arc, LazyLock},
    time::Duration,
};

use chrono::{DateTime, Utc};
use moka::future::Cache;
use schemars::JsonSchema;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    error::{RuntimeError, RuntimeResult},
    store::RuntimeStore,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProtocol {
    OpenaiCompatible,
    Anthropic,
}

impl ProviderProtocol {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenaiCompatible => "openai_compatible",
            Self::Anthropic => "anthropic",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "openai_compatible" => Some(Self::OpenaiCompatible),
            "anthropic" => Some(Self::Anthropic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ProviderRecord {
    pub provider_id: String,
    pub name: String,
    pub protocol: ProviderProtocol,
    pub base_url: String,
    /// Never the raw secret: `env:VAR` is retained, encrypted refs are masked.
    pub api_key_masked: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderInput {
    pub provider_id: String,
    pub name: String,
    pub protocol: ProviderProtocol,
    pub base_url: String,
    pub api_key_ref: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ModelRecord {
    pub id: String,
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub context_limit: i64,
    pub max_output_tokens: Option<i64>,
    pub supports_reasoning: bool,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub cost_input: Option<f64>,
    pub cost_output: Option<f64>,
    pub cost_cache_read: Option<f64>,
    pub enabled: bool,
    pub updated_at: DateTime<Utc>,
}

/// Input payload for `models_upsert`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelInput {
    /// Stable model route ID referenced by `kb_upsert_agent.model` (conventionally `"{provider_id}/{model_id}"`, e.g. `"deepseek/deepseek-chat"`).
    pub id: String,
    /// Registered `provider_id` from `models_list_providers` that serves this model route.
    pub provider_id: String,
    /// Upstream provider's native model identifier sent on API calls (e.g. `"deepseek-chat"` or `"claude-sonnet-4-5"`).
    pub model_id: String,
    /// Human-readable model display name shown in the UI.
    pub display_name: String,
    /// Total context window limit in tokens (`> 0`, e.g. `128000` or `200000`).
    pub context_limit: i64,
    /// Optional per-turn output token cap (`> 0`).
    #[serde(default)]
    pub max_output_tokens: Option<i64>,
    /// Whether the model emits reasoning/thinking blocks (`false` by default).
    #[serde(default)]
    pub supports_reasoning: bool,
    /// Whether the model supports tool/function calling (`true` by default; required for Quire agents).
    #[serde(default = "default_enabled")]
    pub supports_tools: bool,
    /// Whether the model accepts multimodal image content blocks from `preview_report` and `preview_echarts` (`false` by default).
    #[serde(default)]
    pub supports_vision: bool,
    /// Optional uncached input price in RMB (CNY) per 1,000,000 tokens (`>= 0.0`).
    #[serde(default)]
    pub cost_input: Option<f64>,
    /// Optional output completion price in RMB (CNY) per 1,000,000 tokens (`>= 0.0`).
    #[serde(default)]
    pub cost_output: Option<f64>,
    /// Optional prompt-cache-hit input price in RMB (CNY) per 1,000,000 tokens (`>= 0.0`).
    #[serde(default)]
    pub cost_cache_read: Option<f64>,
    /// Whether this model route is enabled for agent execution (`true` by default).
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone)]
pub struct ResolvedModel {
    pub route_id: String,
    pub provider_id: String,
    pub protocol: ProviderProtocol,
    pub base_url: String,
    pub model_id: String,
    pub display_name: String,
    pub context_limit: i64,
    pub max_output_tokens: Option<i64>,
    pub supports_reasoning: bool,
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub cost_input: Option<f64>,
    pub cost_output: Option<f64>,
    pub cost_cache_read: Option<f64>,
    pub api_key: SecretString,
}

/// Compute the RMB (CNY) cost of a single completion call from its token counts
/// using `rig::completion::{CacheCost, CacheRates}` for cache-split accounting.
pub fn calculate_usage_cost_rmb(
    cost_input: Option<f64>,
    cost_output: Option<f64>,
    cost_cache_read: Option<f64>,
    input_tokens: u64,
    output_tokens: u64,
    cached_tokens: u64,
) -> Option<f64> {
    if cost_input.is_none() && cost_output.is_none() && cost_cache_read.is_none() {
        return None;
    }
    let usage = rig::completion::Usage::new()
        .input_tokens(input_tokens)
        .output_tokens(output_tokens)
        .cached_input_tokens(cached_tokens.min(input_tokens));
    let cache_cost = rig::completion::CacheCost::from_usage(&usage);
    let input_rate = cost_input.unwrap_or(0.0);
    let cache_rate = cost_cache_read.or(cost_input).unwrap_or(0.0);
    let output_rate = cost_output.unwrap_or(0.0);
    let rates = rig::completion::CacheRates {
        input: input_rate,
        cached_read: cache_rate,
        cache_write: 0.0,
        storage_per_hour: 0.0,
    };
    let input_cost = cache_cost.usd(&rates);
    let output_cost = ((output_tokens as f64) * output_rate) / 1_000_000.0;
    Some(input_cost + output_cost)
}

impl ResolvedModel {
    /// Compute the RMB (CNY) cost of a single completion call from its token counts.
    /// `cost_input`, `cost_output`, and `cost_cache_read` are priced in RMB per 1,000,000 tokens.
    pub fn calculate_cost_rmb(
        &self,
        input_tokens: u64,
        output_tokens: u64,
        cached_tokens: u64,
    ) -> Option<f64> {
        calculate_usage_cost_rmb(
            self.cost_input,
            self.cost_output,
            self.cost_cache_read,
            input_tokens,
            output_tokens,
            cached_tokens,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelsDevImportRequest {
    /// Local provider row the imported models are bound to.
    pub provider_id: String,
    /// Match this upstream models.dev provider id (for example `deepseek`).
    #[serde(default)]
    pub provider_filter: Option<String>,
    /// Optional subset of upstream model IDs.
    #[serde(default)]
    pub model_ids: Option<Vec<String>>,
    /// Offline payload. When omitted, `models_dev_url` is fetched.
    #[serde(default)]
    pub payload: Option<Value>,
    /// Optional one-off catalog URL override.
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ModelsDevImportReport {
    pub provider_id: String,
    pub imported: usize,
    pub models: Vec<ModelRecord>,
}

#[derive(Clone)]
pub struct ModelRegistry {
    pub(crate) store: RuntimeStore,
    pub(crate) cache: Cache<String, Arc<ResolvedModel>>,
    pub(crate) master_key: Option<SecretString>,
    pub(crate) models_dev_url: String,
    pub(crate) http: reqwest::Client,
}

/// Public routing facade used by the agent runtime.
pub type ModelRouter = ModelRegistry;

static MODEL_HTTP_CLIENT: LazyLock<Result<rig::http_client::ReqwestClient, String>> =
    LazyLock::new(|| {
        let client = rig::rig_reqwest::reqwest::Client::builder()
            .connect_timeout(crate::http_client::CONNECT_TIMEOUT)
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|error| format!("model HTTP client init failed: {error}"))?;
        Ok(rig::http_client::ReqwestClient::from(client))
    });

pub(crate) fn model_http_client() -> RuntimeResult<rig::http_client::ReqwestClient> {
    match &*MODEL_HTTP_CLIENT {
        Ok(client) => Ok(client.clone()),
        Err(error) => Err(RuntimeError::Internal(error.clone())),
    }
}
