use std::{sync::Arc, time::Duration};

use chrono::Utc;
use moka::future::Cache;
use rig::catalog::Catalog;
use rig::completion::CompletionRequest;
use rig::driver::DynModel;
use rig::operation::Completion;
use rig::providers::registry::ProviderId;
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use sqlx::Row;

use crate::{
    error::{RuntimeError, RuntimeResult},
    store::RuntimeStore,
};

use super::*;

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
               FROM catalog.llm_providers ORDER BY provider_id ASC"#,
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
            r#"INSERT INTO catalog.llm_providers
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
               FROM catalog.llm_providers WHERE provider_id = $1"#,
        )
        .bind(provider_id)
        .fetch_optional(self.store.pool())
        .await?;
        row.as_ref().map(row_to_provider).transpose()
    }

    pub async fn delete_provider(&self, provider_id: &str) -> RuntimeResult<()> {
        sqlx::query("DELETE FROM catalog.llm_providers WHERE provider_id = $1")
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
               FROM catalog.llm_models ORDER BY id ASC"#,
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
            r#"INSERT INTO catalog.llm_models
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
               FROM catalog.llm_models WHERE id = $1"#,
        )
        .bind(id)
        .fetch_optional(self.store.pool())
        .await?;
        row.as_ref().map(row_to_model).transpose()
    }

    pub async fn delete_model(&self, id: &str) -> RuntimeResult<()> {
        sqlx::query("DELETE FROM catalog.llm_models WHERE id = $1")
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
                      m.supports_vision, m.cost_input, m.cost_output, m.cost_cache_read,
                      p.protocol, p.base_url, p.api_key_ref
               FROM catalog.llm_models m
               JOIN catalog.llm_providers p ON p.provider_id = m.provider_id
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
            cost_input: row.try_get("cost_input")?,
            cost_output: row.try_get("cost_output")?,
            cost_cache_read: row.try_get("cost_cache_read")?,
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
                let config = rig::providers::openai::OpenAIConfig::new(api_key)
                    .with_base_url(model.base_url);
                let model = config.connect(http).completion(model.model_id);
                Ok(model
                    .with_idle_timeout(Some(Duration::from_secs(120)))
                    .erase())
            }
            ProviderProtocol::Anthropic => {
                let config = rig::providers::anthropic::AnthropicConfig::new(api_key)
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
