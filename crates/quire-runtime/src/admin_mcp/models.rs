use super::*;

#[tool_router(router = models_router, vis = "pub(crate)")]
impl AdminMcpServer {
    #[tool(
        name = "models_list_providers",
        description = quire_protocol::tool_description!("models_list_providers")
    )]
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

    #[tool(
        name = "models_upsert_provider",
        description = quire_protocol::tool_description!("models_upsert_provider")
    )]
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

    #[tool(
        name = "models_list",
        description = quire_protocol::tool_description!("models_list")
    )]
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

    #[tool(
        name = "models_import_models_dev",
        description = quire_protocol::tool_description!("models_import_models_dev")
    )]
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

    #[tool(
        name = "models_upsert",
        description = quire_protocol::tool_description!("models_upsert")
    )]
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

    #[tool(
        name = "models_delete",
        description = quire_protocol::tool_description!("models_delete")
    )]
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

    #[tool(
        name = "models_delete_provider",
        description = quire_protocol::tool_description!("models_delete_provider")
    )]
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

    // ---------------------------------------------------------------------
    // Global MCP registry
    // ---------------------------------------------------------------------
}
