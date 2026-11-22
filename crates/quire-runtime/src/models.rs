//! Unified provider/model registry, secret references and models.dev import.

use std::sync::{Arc, LazyLock};
use std::time::Duration;

use base64::Engine as _;
use chrono::{DateTime, Utc};
use moka::future::Cache;
use rig_core::catalog::Catalog;
use rig_core::completion::CompletionRequest;
use rig_core::driver::DynModel;
use rig_core::operation::Completion;
use rig_core::providers::registry::ProviderId;
use ring::{
    aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey},
    digest::{SHA256, digest},
    rand::{SecureRandom, SystemRandom},
};
use schemars::JsonSchema;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ModelInput {
    pub id: String,
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub context_limit: i64,
    #[serde(default)]
    pub max_output_tokens: Option<i64>,
    #[serde(default)]
    pub supports_reasoning: bool,
    #[serde(default = "default_enabled")]
    pub supports_tools: bool,
    #[serde(default)]
    pub supports_vision: bool,
    #[serde(default)]
    pub cost_input: Option<f64>,
    #[serde(default)]
    pub cost_output: Option<f64>,
    #[serde(default)]
    pub cost_cache_read: Option<f64>,
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
    pub api_key: SecretString,
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
    store: RuntimeStore,
    cache: Cache<String, Arc<ResolvedModel>>,
    master_key: Option<SecretString>,
    models_dev_url: String,
    http: reqwest::Client,
}

/// Public routing facade used by the agent runtime.
pub type ModelRouter = ModelRegistry;

static MODEL_HTTP_CLIENT: LazyLock<Result<rig_reqwest::ReqwestClient, String>> =
    LazyLock::new(|| {
        let client = rig_reqwest::reqwest::Client::builder()
            .connect_timeout(crate::http_client::CONNECT_TIMEOUT)
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|error| format!("model HTTP client init failed: {error}"))?;
        Ok(rig_reqwest::ReqwestClient::from(client))
    });

fn model_http_client() -> RuntimeResult<rig_reqwest::ReqwestClient> {
    match &*MODEL_HTTP_CLIENT {
        Ok(client) => Ok(client.clone()),
        Err(error) => Err(RuntimeError::Internal(error.clone())),
    }
}

impl ModelRegistry {
    pub fn new(
        store: RuntimeStore,
        master_key: Option<SecretString>,
        models_dev_url: impl Into<String>,
    ) -> Self {
        Self {
            store,
            cache: Cache::new(128),
            master_key,
            models_dev_url: models_dev_url.into(),
            http: crate::http_client::timed_client(Duration::from_secs(60)),
        }
    }

    pub fn store(&self) -> &RuntimeStore {
        &self.store
    }

    pub fn spawn_invalidation_listener(&self) -> tokio::task::JoinHandle<()> {
        let cache = self.cache.clone();
        crate::bus::spawn_resilient_invalidation_listener(
            self.store.bus().clone(),
            crate::bus::RedisBus::model_invalidation_subscription,
            Box::new(move || {
                let cache = cache.clone();
                Box::pin(async move {
                    cache.invalidate_all();
                    cache.run_pending_tasks().await;
                })
            }),
        )
    }

    pub async fn list_providers(&self) -> RuntimeResult<Vec<ProviderRecord>> {
        let rows = sqlx::query(
            r#"SELECT provider_id, name, protocol, base_url, api_key_ref, enabled,
                      created_at, updated_at
               FROM llm_providers ORDER BY provider_id ASC"#,
        )
        .fetch_all(self.store.pool())
        .await?;
        rows.iter().map(row_to_provider).collect()
    }

    pub async fn upsert_provider(&self, input: ProviderInput) -> RuntimeResult<ProviderRecord> {
        if input.provider_id.trim().is_empty() || input.base_url.trim().is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "provider_id and base_url are required".into(),
            ));
        }
        let api_key_ref = normalize_api_key_ref(&input.api_key_ref, self.master_key.as_ref())?;
        sqlx::query(
            r#"INSERT INTO llm_providers
               (provider_id, name, protocol, base_url, api_key_ref, enabled)
               VALUES ($1, $2, $3, $4, $5, $6)
               ON CONFLICT (provider_id) DO UPDATE
               SET name = EXCLUDED.name, protocol = EXCLUDED.protocol,
                   base_url = EXCLUDED.base_url, api_key_ref = EXCLUDED.api_key_ref,
                   enabled = EXCLUDED.enabled, updated_at = NOW()"#,
        )
        .bind(&input.provider_id)
        .bind(&input.name)
        .bind(input.protocol.as_str())
        .bind(&input.base_url)
        .bind(api_key_ref)
        .bind(input.enabled)
        .execute(self.store.pool())
        .await?;
        self.after_mutation().await?;
        self.get_provider(&input.provider_id)
            .await?
            .ok_or_else(|| RuntimeError::Internal("provider vanished".into()))
    }

    pub async fn get_provider(&self, provider_id: &str) -> RuntimeResult<Option<ProviderRecord>> {
        let row = sqlx::query(
            r#"SELECT provider_id, name, protocol, base_url, api_key_ref, enabled,
                      created_at, updated_at
               FROM llm_providers WHERE provider_id = $1"#,
        )
        .bind(provider_id)
        .fetch_optional(self.store.pool())
        .await?;
        row.as_ref().map(row_to_provider).transpose()
    }

    pub async fn delete_provider(&self, provider_id: &str) -> RuntimeResult<()> {
        sqlx::query("DELETE FROM llm_providers WHERE provider_id = $1")
            .bind(provider_id)
            .execute(self.store.pool())
            .await?;
        self.after_mutation().await?;
        Ok(())
    }

    pub async fn list_models(&self) -> RuntimeResult<Vec<ModelRecord>> {
        let rows = sqlx::query(
            r#"SELECT id, provider_id, model_id, display_name, context_limit,
                      max_output_tokens, supports_reasoning, supports_tools, supports_vision,
                      cost_input, cost_output, cost_cache_read, enabled, updated_at
               FROM llm_models ORDER BY id ASC"#,
        )
        .fetch_all(self.store.pool())
        .await?;
        rows.iter().map(row_to_model).collect()
    }

    pub async fn upsert_model(&self, input: ModelInput) -> RuntimeResult<ModelRecord> {
        if input.id.trim().is_empty()
            || input.model_id.trim().is_empty()
            || input.context_limit <= 0
        {
            return Err(RuntimeError::InvalidRequest(
                "model id, model_id and a positive context_limit are required".into(),
            ));
        }
        sqlx::query(
            r#"INSERT INTO llm_models
               (id, provider_id, model_id, display_name, context_limit, max_output_tokens,
                supports_reasoning, supports_tools, supports_vision,
                cost_input, cost_output, cost_cache_read, enabled)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
               ON CONFLICT (id) DO UPDATE
               SET provider_id = EXCLUDED.provider_id, model_id = EXCLUDED.model_id,
                   display_name = EXCLUDED.display_name, context_limit = EXCLUDED.context_limit,
                   max_output_tokens = EXCLUDED.max_output_tokens,
                   supports_reasoning = EXCLUDED.supports_reasoning,
                   supports_tools = EXCLUDED.supports_tools,
                   supports_vision = EXCLUDED.supports_vision,
                   cost_input = EXCLUDED.cost_input, cost_output = EXCLUDED.cost_output,
                   cost_cache_read = EXCLUDED.cost_cache_read, enabled = EXCLUDED.enabled,
                   updated_at = NOW()"#,
        )
        .bind(&input.id)
        .bind(&input.provider_id)
        .bind(&input.model_id)
        .bind(&input.display_name)
        .bind(input.context_limit)
        .bind(input.max_output_tokens)
        .bind(input.supports_reasoning)
        .bind(input.supports_tools)
        .bind(input.supports_vision)
        .bind(input.cost_input)
        .bind(input.cost_output)
        .bind(input.cost_cache_read)
        .bind(input.enabled)
        .execute(self.store.pool())
        .await?;
        self.after_mutation().await?;
        self.get_model(&input.id)
            .await?
            .ok_or_else(|| RuntimeError::Internal("model vanished".into()))
    }

    pub async fn get_model(&self, id: &str) -> RuntimeResult<Option<ModelRecord>> {
        let row = sqlx::query(
            r#"SELECT id, provider_id, model_id, display_name, context_limit,
                      max_output_tokens, supports_reasoning, supports_tools, supports_vision,
                      cost_input, cost_output, cost_cache_read, enabled, updated_at
               FROM llm_models WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(self.store.pool())
        .await?;
        row.as_ref().map(row_to_model).transpose()
    }

    pub async fn delete_model(&self, id: &str) -> RuntimeResult<()> {
        sqlx::query("DELETE FROM llm_models WHERE id = $1")
            .bind(id)
            .execute(self.store.pool())
            .await?;
        self.after_mutation().await?;
        Ok(())
    }

    pub async fn resolve(&self, route_id: &str) -> RuntimeResult<ResolvedModel> {
        if let Some(cached) = self.cache.get(route_id).await {
            return Ok((*cached).clone());
        }
        let row = sqlx::query(
            r#"SELECT m.id, m.provider_id, m.model_id, m.display_name, m.context_limit,
                      m.max_output_tokens, m.supports_reasoning, m.supports_tools,
                      m.supports_vision, p.protocol, p.base_url, p.api_key_ref
               FROM llm_models m
               JOIN llm_providers p ON p.provider_id = m.provider_id
               WHERE m.id = $1 AND m.enabled AND p.enabled"#,
        )
        .bind(route_id)
        .fetch_optional(self.store.pool())
        .await?
        .ok_or_else(|| {
            RuntimeError::InvalidRequest(format!(
                "model route `{route_id}` is not registered or enabled"
            ))
        })?;
        let protocol_raw: String = row.try_get("protocol")?;
        let protocol = ProviderProtocol::parse(&protocol_raw).ok_or_else(|| {
            RuntimeError::Internal(format!("invalid provider protocol `{protocol_raw}`"))
        })?;
        let api_key_ref: String = row.try_get("api_key_ref")?;
        let resolved = ResolvedModel {
            route_id: row.try_get("id")?,
            provider_id: row.try_get("provider_id")?,
            protocol,
            base_url: row.try_get("base_url")?,
            model_id: row.try_get("model_id")?,
            display_name: row.try_get("display_name")?,
            context_limit: row.try_get("context_limit")?,
            max_output_tokens: row.try_get("max_output_tokens")?,
            supports_reasoning: row.try_get("supports_reasoning")?,
            supports_tools: row.try_get("supports_tools")?,
            supports_vision: row.try_get("supports_vision")?,
            api_key: resolve_api_key_ref(&api_key_ref, self.master_key.as_ref())?,
        };
        let resolved = Arc::new(resolved);
        self.cache
            .insert(route_id.to_string(), resolved.clone())
            .await;
        Ok((*resolved).clone())
    }

    /// Build a rig-core completion model for the registered protocol.
    pub async fn build_rig_model(&self, route_id: &str) -> RuntimeResult<DynModel<Completion>> {
        let model = self.resolve(route_id).await?;
        let api_key = model.api_key.expose_secret().to_string();
        let http = model_http_client()?;
        match model.protocol {
            ProviderProtocol::OpenaiCompatible => {
                let config = rig_core::providers::openai::OpenAIConfig::new(api_key)
                    .with_base_url(model.base_url);
                let model = config.connect(http).completion(model.model_id);
                Ok(model
                    .with_idle_timeout(Some(Duration::from_secs(120)))
                    .erase())
            }
            ProviderProtocol::Anthropic => {
                let config = rig_core::providers::anthropic::AnthropicConfig::new(api_key)
                    .with_base_url(model.base_url);
                let model = config.connect(http).completion(model.model_id);
                Ok(model
                    .with_idle_timeout(Some(Duration::from_secs(120)))
                    .erase())
            }
        }
    }

    /// Local, no-network capability check for a route's first completion request.
    pub async fn preflight_warnings(&self, route_id: &str) -> RuntimeResult<Vec<String>> {
        let resolved = self.resolve(route_id).await?;
        let mut warnings = Vec::new();
        if !resolved.supports_tools {
            warnings.push(
                "model does not advertise tool calls, but Quire agents rely on tools".to_string(),
            );
        }
        let model = self.build_rig_model(route_id).await?;
        if let Err(error) = model.check(&CompletionRequest::new("preflight")) {
            warnings.push(format!("request preflight: {error}"));
        }
        Ok(warnings)
    }

    pub async fn import_models_dev(
        &self,
        request: ModelsDevImportRequest,
    ) -> RuntimeResult<ModelsDevImportReport> {
        self.get_provider(&request.provider_id)
            .await?
            .ok_or_else(|| {
                RuntimeError::InvalidRequest(format!(
                    "provider `{}` must exist before importing models",
                    request.provider_id
                ))
            })?;
        let payload = match (request.payload, request.url) {
            (Some(payload), _) => payload,
            (None, Some(url)) => self.fetch_catalog(&url).await?,
            (None, None) => self.fetch_catalog(&self.models_dev_url).await?,
        };
        if let Some(filter) = request.provider_filter.as_deref() {
            let _ = ProviderId::declare(filter);
        }
        let payload = serde_json::to_string(&payload)?;
        let (catalog, skipped) = Catalog::from_models_dev(&payload).map_err(|error| {
            RuntimeError::InvalidRequest(format!("invalid models.dev data: {error}"))
        })?;
        for entry in &skipped {
            tracing::warn!(path = %entry.path, reason = ?entry.reason, "skipped models.dev entry");
        }
        let provider_filter = request.provider_filter.as_deref();
        let wanted: Option<Vec<String>> = request.model_ids;
        let mut imported = Vec::new();
        for spec in catalog.iter() {
            let vendor = spec.provider.vendor();
            if let Some(filter) = provider_filter
                && filter != vendor
                && filter != spec.provider.to_string()
            {
                continue;
            }
            if let Some(wanted) = &wanted
                && !wanted.iter().any(|id| id == &spec.id)
            {
                continue;
            }
            let route_id = format!("{}/{}", request.provider_id, spec.id);
            let context_limit = spec
                .context_window
                .map(i64::from)
                .filter(|value| *value > 0)
                .unwrap_or(128_000);
            let input = ModelInput {
                id: route_id,
                provider_id: request.provider_id.clone(),
                model_id: spec.id.clone(),
                display_name: if spec.display_name.trim().is_empty() {
                    spec.id.clone()
                } else {
                    spec.display_name.clone()
                },
                context_limit,
                max_output_tokens: spec.max_output_tokens.map(i64::from),
                supports_reasoning: spec.reasoning.supported(),
                supports_tools: spec.tools,
                supports_vision: spec.input.image || spec.input.pdf,
                cost_input: spec.pricing.map(|pricing| pricing.input),
                cost_output: spec.pricing.map(|pricing| pricing.output),
                cost_cache_read: spec.pricing.and_then(|pricing| pricing.cache_read),
                enabled: true,
            };
            imported.push(self.upsert_model(input).await?);
        }
        Ok(ModelsDevImportReport {
            provider_id: request.provider_id,
            imported: imported.len(),
            models: imported,
        })
    }

    async fn fetch_catalog(&self, url: &str) -> RuntimeResult<Value> {
        let response = self.http.get(url).send().await?;
        if !response.status().is_success() {
            return Err(RuntimeError::InvalidRequest(format!(
                "models.dev catalog fetch failed with {}",
                response.status()
            )));
        }
        Ok(response.json().await?)
    }

    async fn after_mutation(&self) -> RuntimeResult<()> {
        self.cache.invalidate_all();
        self.cache.run_pending_tasks().await;
        let version = u64::try_from(Utc::now().timestamp_millis()).unwrap_or(0);
        if let Err(error) = self.store.bus().publish_model_invalidation(version).await {
            tracing::warn!(%error, "failed to publish model cache invalidation");
        }
        Ok(())
    }
}

fn row_to_provider(row: &sqlx::postgres::PgRow) -> RuntimeResult<ProviderRecord> {
    let protocol_raw: String = row.try_get("protocol")?;
    Ok(ProviderRecord {
        provider_id: row.try_get("provider_id")?,
        name: row.try_get("name")?,
        protocol: ProviderProtocol::parse(&protocol_raw).ok_or_else(|| {
            RuntimeError::Internal(format!("invalid provider protocol `{protocol_raw}`"))
        })?,
        base_url: row.try_get("base_url")?,
        api_key_masked: mask_api_key_ref(&row.try_get::<String, _>("api_key_ref")?),
        enabled: row.try_get("enabled")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn row_to_model(row: &sqlx::postgres::PgRow) -> RuntimeResult<ModelRecord> {
    Ok(ModelRecord {
        id: row.try_get("id")?,
        provider_id: row.try_get("provider_id")?,
        model_id: row.try_get("model_id")?,
        display_name: row.try_get("display_name")?,
        context_limit: row.try_get("context_limit")?,
        max_output_tokens: row.try_get("max_output_tokens")?,
        supports_reasoning: row.try_get("supports_reasoning")?,
        supports_tools: row.try_get("supports_tools")?,
        supports_vision: row.try_get("supports_vision")?,
        cost_input: row.try_get("cost_input")?,
        cost_output: row.try_get("cost_output")?,
        cost_cache_read: row.try_get("cost_cache_read")?,
        enabled: row.try_get("enabled")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn mask_api_key_ref(value: &str) -> String {
    if value.starts_with("env:") {
        value.to_string()
    } else if value.starts_with("enc:") {
        "enc:***".to_string()
    } else {
        "***".to_string()
    }
}

/// Mask a generic `env:` / `enc:` / raw secret reference for public records.
pub fn mask_secret_ref(value: &str) -> String {
    mask_api_key_ref(value)
}

/// Normalize a generic secret reference, encrypting raw values with the runtime master key.
pub fn normalize_secret_ref(
    value: &str,
    master_key: Option<&SecretString>,
) -> RuntimeResult<String> {
    normalize_api_key_ref(value, master_key)
}

fn normalize_api_key_ref(value: &str, master_key: Option<&SecretString>) -> RuntimeResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(RuntimeError::InvalidRequest(
            "api_key_ref must not be empty".into(),
        ));
    }
    if trimmed.starts_with("env:") || trimmed.starts_with("enc:") {
        return Ok(trimmed.to_string());
    }
    let Some(master) = master_key else {
        return Err(RuntimeError::InvalidRequest(
            "raw api keys require RUNTIME_MASTER_KEY, or use api_key_ref=env:VAR".into(),
        ));
    };
    encrypt_secret(trimmed, master)
}

pub fn resolve_api_key_ref(
    value: &str,
    master_key: Option<&SecretString>,
) -> RuntimeResult<SecretString> {
    if let Some(name) = value.strip_prefix("env:") {
        let secret = std::env::var(name).map_err(|_| {
            RuntimeError::InvalidRequest(format!("environment variable `{name}` is not set"))
        })?;
        return Ok(SecretString::from(secret));
    }
    if value.starts_with("enc:") {
        let master = master_key.ok_or_else(|| {
            RuntimeError::InvalidRequest(
                "RUNTIME_MASTER_KEY is required to decrypt provider credentials".into(),
            )
        })?;
        return Ok(SecretString::from(decrypt_secret(value, master)?));
    }
    Err(RuntimeError::InvalidRequest(
        "api_key_ref must be env:VAR or enc:...".into(),
    ))
}

pub fn encrypt_secret(plaintext: &str, master_key: &SecretString) -> RuntimeResult<String> {
    let key = master_aead_key(master_key)?;
    let mut nonce_bytes = [0u8; 12];
    SystemRandom::new()
        .fill(&mut nonce_bytes)
        .map_err(|_| RuntimeError::Internal("failed to generate encryption nonce".into()))?;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    let mut buffer = plaintext.as_bytes().to_vec();
    key.seal_in_place_append_tag(nonce, Aad::empty(), &mut buffer)
        .map_err(|_| RuntimeError::Internal("failed to encrypt credential".into()))?;
    let mut combined = nonce_bytes.to_vec();
    combined.extend_from_slice(&buffer);
    Ok(format!(
        "enc:{}",
        base64::engine::general_purpose::STANDARD.encode(combined)
    ))
}

pub fn decrypt_secret(ciphertext: &str, master_key: &SecretString) -> RuntimeResult<String> {
    let encoded = ciphertext.strip_prefix("enc:").ok_or_else(|| {
        RuntimeError::InvalidRequest("encrypted credential must start with enc:".into())
    })?;
    let combined = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| RuntimeError::InvalidRequest(format!("invalid encrypted ref: {error}")))?;
    let nonce_bytes: [u8; 12] = combined
        .get(..12)
        .and_then(|slice| <[u8; 12]>::try_from(slice).ok())
        .ok_or_else(|| RuntimeError::InvalidRequest("encrypted ref is truncated".into()))?;
    let mut buffer = combined
        .get(12..)
        .ok_or_else(|| RuntimeError::InvalidRequest("encrypted ref is truncated".into()))?
        .to_vec();
    let key = master_aead_key(master_key)?;
    let plaintext = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce_bytes),
            Aad::empty(),
            &mut buffer,
        )
        .map_err(|_| RuntimeError::InvalidRequest("credential decryption failed".into()))?;
    String::from_utf8(plaintext.to_vec())
        .map_err(|_| RuntimeError::InvalidRequest("decrypted credential is not UTF-8".into()))
}

fn master_aead_key(master_key: &SecretString) -> RuntimeResult<LessSafeKey> {
    let key = digest(&SHA256, master_key.expose_secret().as_bytes());
    let unbound = UnboundKey::new(&AES_256_GCM, key.as_ref())
        .map_err(|_| RuntimeError::Internal("failed to derive credential encryption key".into()))?;
    Ok(LessSafeKey::new(unbound))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_refs_hide_and_roundtrip_secret_values() {
        let master = SecretString::from("master-key-for-tests");
        let encrypted = encrypt_secret("sk-super-secret", &master).unwrap();
        assert!(encrypted.starts_with("enc:"));
        assert!(!encrypted.contains("sk-super-secret"));
        assert_eq!(
            decrypt_secret(&encrypted, &master).unwrap(),
            "sk-super-secret"
        );
        assert_eq!(mask_api_key_ref(&encrypted), "enc:***");
        assert_eq!(
            mask_api_key_ref("env:DEEPSEEK_API_KEY"),
            "env:DEEPSEEK_API_KEY"
        );
    }

    #[test]
    fn env_refs_resolve_without_exposing_the_value() {
        let resolved = resolve_api_key_ref("env:PATH", None).unwrap();
        assert!(!resolved.expose_secret().is_empty());
        assert!(resolve_api_key_ref("env:RUNTIME_MISSING_VAR_FOR_TEST", None).is_err());
    }
}
