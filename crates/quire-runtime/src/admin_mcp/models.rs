use super::*;

#[tool_router(router = models_router, vis = "pub(crate)")]
impl AdminMcpServer {
    /// List all registered LLM providers with their wire `protocol` (`openai_compatible` or `anthropic`), `base_url`, masked API key reference, and `enabled` flag.
    #[tool(name = "models_list_providers")]
    pub async fn models_list_providers(
        &self,
        Parameters(_request): Parameters<EmptyParams>,
    ) -> Result<Json<ModelsProvidersReceipt>, ErrorData> {
        let providers = self
            .service
            .models()
            .list_providers()
            .await
            .map_err(admin_error)?;
        let next_hint =
            "Call models_list to inspect routes or models_upsert_provider to add one.".to_string();
        Ok(Json(ModelsProvidersReceipt {
            providers,
            next_hint,
        }))
    }

    /// Register or update an LLM provider (`provider_id`, `name`, `protocol`, `base_url`, `api_key_ref`, `enabled`).
    ///
    /// Call this before registering or importing model routes for that `provider_id`. Prefer `env:VAR_NAME` for `api_key_ref`; raw keys require `RUNTIME_MASTER_KEY` to be configured for envelope encryption.
    #[tool(name = "models_upsert_provider")]
    pub async fn models_upsert_provider(
        &self,
        Parameters(request): Parameters<ModelsUpsertProviderParams>,
    ) -> Result<Json<ModelProviderReceipt>, ErrorData> {
        let provider = self
            .service
            .models()
            .upsert_provider(ProviderInput {
                provider_id: request.provider_id,
                name: request.name,
                protocol: request.protocol,
                base_url: request.base_url,
                api_key_ref: request.api_key_ref,
                enabled: request.enabled,
            })
            .await
            .map_err(admin_error)?;
        let next_hint =
            "Import routes with models_import_models_dev, then bind them to agents with kb_upsert_agent."
                .to_string();
        Ok(Json(ModelProviderReceipt {
            provider,
            next_hint,
        }))
    }

    /// List all registered model routes across providers, including `id` (the route ID referenced by agents), `provider_id`, upstream `model_id`, token limits, capability flags, and RMB per-1M-token pricing.
    #[tool(name = "models_list")]
    pub async fn models_list(
        &self,
        Parameters(_request): Parameters<EmptyParams>,
    ) -> Result<Json<ModelsListReceipt>, ErrorData> {
        let models = self
            .service
            .models()
            .list_models()
            .await
            .map_err(admin_error)?;
        let next_hint =
            "Use a model route ID when creating an agent with kb_upsert_agent.".to_string();
        Ok(Json(ModelsListReceipt { models, next_hint }))
    }

    /// Batch-import model routes and capability/pricing metadata from a `models.dev` catalog (remote URL or inline `payload`) into an existing local `provider_id`.
    ///
    /// Each imported route receives `id = "{provider_id}/{model_id}"` (or `{model_id}` if already prefixed). Ensure `provider_id` is registered via `models_upsert_provider` first.
    #[tool(name = "models_import_models_dev")]
    pub async fn models_import_models_dev(
        &self,
        Parameters(request): Parameters<ModelsImportParams>,
    ) -> Result<Json<ModelsImportReceipt>, ErrorData> {
        let report = self
            .service
            .models()
            .import_models_dev(ModelsDevImportRequest {
                provider_id: request.provider_id,
                provider_filter: request.provider_filter,
                model_ids: request.model_ids,
                payload: request.payload,
                url: request.url,
            })
            .await
            .map_err(admin_error)?;
        let next_hint = if report.imported == 0 {
            "No models matched the filter. Call models_import_models_dev without provider_filter or model_ids."
                .to_string()
        } else {
            format!(
                "Imported {} routes for provider `{}`. Call models_list to inspect them.",
                report.imported, report.provider_id
            )
        };
        Ok(Json(ModelsImportReceipt {
            provider_id: report.provider_id,
            imported: report.imported,
            models: report.models,
            next_hint,
        }))
    }

    /// Create or update a single model route (`id`, `provider_id`, upstream `model_id`, token limits, capability flags, and RMB per-1M-token costs) and run preflight validation.
    ///
    /// Always inspect `warnings` in the receipt (for example missing environment variable credentials on the bound provider or disabled `supports_tools`).
    #[tool(name = "models_upsert")]
    pub async fn models_upsert(
        &self,
        Parameters(request): Parameters<ModelInput>,
    ) -> Result<Json<ModelReceipt>, ErrorData> {
        let model = self
            .service
            .models()
            .upsert_model(request)
            .await
            .map_err(admin_error)?;
        let warnings = self
            .service
            .models()
            .preflight_warnings(&model.id)
            .await
            .unwrap_or_else(|error| vec![format!("preflight unavailable: {error}")]);
        let next_hint = format!(
            "Model `{}` saved under provider `{}`. Reference it in kb_upsert_agent.",
            model.id, model.provider_id
        );
        Ok(Json(ModelReceipt {
            model,
            warnings,
            next_hint,
        }))
    }

    /// Delete a model route by its route `id` and invalidate the model resolution cache.
    ///
    /// Before deleting, ensure no active or draft knowledge-base agent still binds `model` to this route ID.
    #[tool(name = "models_delete")]
    pub async fn models_delete(
        &self,
        Parameters(request): Parameters<ModelIdParams>,
    ) -> Result<Json<DeleteReceipt>, ErrorData> {
        self.service
            .models()
            .delete_model(&request.id)
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Model `{}` deleted. Call models_list to verify remaining routes.",
            request.id
        );
        Ok(Json(DeleteReceipt {
            deleted: true,
            target: request.id,
            next_hint,
        }))
    }

    /// Delete a registered LLM provider by `provider_id` and cascade-delete all model routes bound to it.
    ///
    /// Verify via `models_list` that no agents depend on routes under this provider before deleting.
    #[tool(name = "models_delete_provider")]
    pub async fn models_delete_provider(
        &self,
        Parameters(request): Parameters<ProviderIdParams>,
    ) -> Result<Json<DeleteReceipt>, ErrorData> {
        self.service
            .models()
            .delete_provider(&request.provider_id)
            .await
            .map_err(admin_error)?;
        let next_hint = format!(
            "Provider `{}` deleted. Call models_list_providers to verify.",
            request.provider_id
        );
        Ok(Json(DeleteReceipt {
            deleted: true,
            target: request.provider_id,
            next_hint,
        }))
    }
}
