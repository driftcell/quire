use super::*;

pub(crate) async fn list_model_providers(
    State(service): State<RuntimeService>,
) -> RuntimeResult<Json<Vec<ProviderRecord>>> {
    service.models().list_providers().await.map(Json)
}

pub(crate) async fn create_model_provider(
    State(service): State<RuntimeService>,
    Json(input): Json<ProviderInput>,
) -> RuntimeResult<Json<ProviderRecord>> {
    service.models().upsert_provider(input).await.map(Json)
}

pub(crate) async fn update_model_provider(
    State(service): State<RuntimeService>,
    Path(provider_id): Path<String>,
    Json(mut input): Json<ProviderInput>,
) -> RuntimeResult<Json<ProviderRecord>> {
    input.provider_id = provider_id;
    service.models().upsert_provider(input).await.map(Json)
}

pub(crate) async fn delete_model_provider(
    State(service): State<RuntimeService>,
    Path(provider_id): Path<String>,
) -> RuntimeResult<StatusCode> {
    service.models().delete_provider(&provider_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn list_models(
    State(service): State<RuntimeService>,
) -> RuntimeResult<Json<Vec<ModelRecord>>> {
    service.models().list_models().await.map(Json)
}

pub(crate) async fn create_model(
    State(service): State<RuntimeService>,
    Json(input): Json<ModelInput>,
) -> RuntimeResult<Json<ModelRecord>> {
    service.models().upsert_model(input).await.map(Json)
}

pub(crate) async fn update_model(
    State(service): State<RuntimeService>,
    Path(id): Path<String>,
    Json(mut input): Json<ModelInput>,
) -> RuntimeResult<Json<ModelRecord>> {
    input.id = id;
    service.models().upsert_model(input).await.map(Json)
}

pub(crate) async fn delete_model(
    State(service): State<RuntimeService>,
    Path(id): Path<String>,
) -> RuntimeResult<StatusCode> {
    service.models().delete_model(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn import_models_dev(
    State(service): State<RuntimeService>,
    Json(request): Json<ModelsDevImportRequest>,
) -> RuntimeResult<Json<ModelsDevImportReport>> {
    service.models().import_models_dev(request).await.map(Json)
}

// ---------------------------------------------------------------------------
// Global MCP registry
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct McpToolsQuery {
    #[serde(default)]
    server_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct McpToolToggleRequest {
    enabled: bool,
}

pub(crate) async fn list_mcp_servers(
    State(service): State<RuntimeService>,
) -> RuntimeResult<Json<Vec<McpServerRecord>>> {
    service.mcp().list_servers().await.map(Json)
}

pub(crate) async fn upsert_mcp_server(
    State(service): State<RuntimeService>,
    Json(input): Json<McpServerInput>,
) -> RuntimeResult<Json<McpServerRecord>> {
    service.mcp().upsert_server(input).await.map(Json)
}

pub(crate) async fn patch_mcp_server(
    State(service): State<RuntimeService>,
    Path(server_id): Path<String>,
    Json(patch): Json<McpServerPatch>,
) -> RuntimeResult<Json<McpServerRecord>> {
    service
        .mcp()
        .patch_server(&server_id, patch)
        .await
        .map(Json)
}

pub(crate) async fn delete_mcp_server(
    State(service): State<RuntimeService>,
    Path(server_id): Path<String>,
) -> RuntimeResult<StatusCode> {
    service.mcp().delete_server(&server_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn sync_mcp_server(
    State(service): State<RuntimeService>,
    Path(server_id): Path<String>,
) -> RuntimeResult<Json<McpSyncReport>> {
    service.mcp().sync_server_tools(&server_id).await.map(Json)
}

pub(crate) async fn list_mcp_tools(
    State(service): State<RuntimeService>,
    Query(query): Query<McpToolsQuery>,
) -> RuntimeResult<Json<Vec<McpToolRecord>>> {
    service
        .mcp()
        .list_tools(query.server_id.as_deref())
        .await
        .map(Json)
}

pub(crate) async fn set_mcp_tool_enabled(
    State(service): State<RuntimeService>,
    Path((server_id, tool_name)): Path<(String, String)>,
    Json(request): Json<McpToolToggleRequest>,
) -> RuntimeResult<Json<McpToolRecord>> {
    service
        .mcp()
        .set_tool_enabled(&server_id, &tool_name, request.enabled)
        .await
        .map(Json)
}

// ---------------------------------------------------------------------------
// Knowledge-base CRUD
// ---------------------------------------------------------------------------
