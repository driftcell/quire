use super::*;

#[derive(Debug, Deserialize)]
pub(crate) struct CreateDocumentRequest {
    doc_id: String,
    name: String,
    content: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateDocumentRequest {
    name: String,
    content: String,
}

pub(crate) async fn list_kb_docs(
    State(service): State<RuntimeService>,
) -> RuntimeResult<Json<Vec<KbDocumentRecord>>> {
    service.kb().list_documents().await.map(Json)
}

pub(crate) async fn create_kb_doc(
    State(service): State<RuntimeService>,
    Json(request): Json<CreateDocumentRequest>,
) -> RuntimeResult<Json<KbDocumentRecord>> {
    service
        .kb()
        .upsert_document(&request.doc_id, &request.name, &request.content)
        .await
        .map(Json)
}

pub(crate) async fn update_kb_doc(
    State(service): State<RuntimeService>,
    Path(doc_id): Path<String>,
    Json(request): Json<UpdateDocumentRequest>,
) -> RuntimeResult<Json<KbDocumentRecord>> {
    service
        .kb()
        .upsert_document(&doc_id, &request.name, &request.content)
        .await
        .map(Json)
}

pub(crate) async fn delete_kb_doc(
    State(service): State<RuntimeService>,
    Path(doc_id): Path<String>,
) -> RuntimeResult<StatusCode> {
    service.kb().delete_document(&doc_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub(crate) struct AgentRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: String,
    model: String,
    system_prompt: String,
    #[serde(default)]
    allowed_tools: Vec<String>,
    #[serde(default)]
    is_default: bool,
    #[serde(default = "default_enabled")]
    enabled: bool,
}

fn default_enabled() -> bool {
    true
}

pub(crate) async fn list_agents(
    State(service): State<RuntimeService>,
) -> RuntimeResult<Json<Vec<AgentDefinitionRecord>>> {
    service.kb().list_agents().await.map(Json)
}

pub(crate) async fn create_agent(
    State(service): State<RuntimeService>,
    Json(request): Json<AgentRequest>,
) -> RuntimeResult<Json<AgentDefinitionRecord>> {
    let name = request
        .name
        .ok_or_else(|| RuntimeError::InvalidRequest("agent name is required".into()))?;
    service
        .kb()
        .upsert_agent(UpsertKbAgentCommand {
            name: &name,
            description: &request.description,
            model: &request.model,
            system_prompt: &request.system_prompt,
            allowed_tools: &request.allowed_tools,
            is_default: request.is_default,
            enabled: request.enabled,
        })
        .await
        .map(Json)
}

pub(crate) async fn update_agent(
    State(service): State<RuntimeService>,
    Path(name): Path<String>,
    Json(request): Json<AgentRequest>,
) -> RuntimeResult<Json<AgentDefinitionRecord>> {
    service
        .kb()
        .upsert_agent(UpsertKbAgentCommand {
            name: &name,
            description: &request.description,
            model: &request.model,
            system_prompt: &request.system_prompt,
            allowed_tools: &request.allowed_tools,
            is_default: request.is_default,
            enabled: request.enabled,
        })
        .await
        .map(Json)
}

pub(crate) async fn delete_agent(
    State(service): State<RuntimeService>,
    Path(name): Path<String>,
) -> RuntimeResult<StatusCode> {
    service.kb().delete_agent(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub(crate) struct ResourceInput {
    path: String,
    content: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SkillRequest {
    #[serde(default)]
    name: Option<String>,
    description: String,
    body: String,
    #[serde(default = "default_enabled")]
    enabled: bool,
    #[serde(default)]
    resources: Vec<ResourceInput>,
}

pub(crate) async fn list_skills(
    State(service): State<RuntimeService>,
) -> RuntimeResult<Json<Vec<SkillDefinitionRecord>>> {
    service.kb().list_skills().await.map(Json)
}

pub(crate) async fn create_skill(
    State(service): State<RuntimeService>,
    Json(request): Json<SkillRequest>,
) -> RuntimeResult<Json<SkillDefinitionRecord>> {
    let name = request
        .name
        .clone()
        .ok_or_else(|| RuntimeError::InvalidRequest("skill name is required".into()))?;
    upsert_skill(&service, &name, request).await
}

pub(crate) async fn update_skill(
    State(service): State<RuntimeService>,
    Path(name): Path<String>,
    Json(request): Json<SkillRequest>,
) -> RuntimeResult<Json<SkillDefinitionRecord>> {
    upsert_skill(&service, &name, request).await
}

pub(crate) async fn upsert_skill(
    service: &RuntimeService,
    name: &str,
    request: SkillRequest,
) -> RuntimeResult<Json<SkillDefinitionRecord>> {
    let resources: Vec<(String, String)> = request
        .resources
        .into_iter()
        .map(|resource| (resource.path, resource.content))
        .collect();
    service
        .kb()
        .upsert_skill(
            name,
            &request.description,
            &request.body,
            request.enabled,
            &resources,
        )
        .await
        .map(Json)
}

pub(crate) async fn delete_skill(
    State(service): State<RuntimeService>,
    Path(name): Path<String>,
) -> RuntimeResult<StatusCode> {
    service.kb().delete_skill(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Knowledge-base branches, promotion and rollback
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateBranchRequest {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default, alias = "base", alias = "source", alias = "from_branch")]
    from: Option<String>,
}

pub(crate) async fn list_kb_branches(
    State(service): State<RuntimeService>,
) -> RuntimeResult<Json<Vec<KbBranchRecord>>> {
    service.kb().list_branches().await.map(Json)
}

pub(crate) async fn create_kb_branch(
    State(service): State<RuntimeService>,
    Json(request): Json<CreateBranchRequest>,
) -> RuntimeResult<Json<KbBranchRecord>> {
    service
        .kb()
        .create_branch(&request.name, &request.description, request.from.as_deref())
        .await
        .map(Json)
}

pub(crate) async fn diff_kb_branch(
    State(service): State<RuntimeService>,
    Path(name): Path<String>,
) -> RuntimeResult<Json<KbBranchDiff>> {
    service.kb().diff_branch(&name).await.map(Json)
}

pub(crate) async fn promote_kb_branch(
    State(service): State<RuntimeService>,
    Path(name): Path<String>,
) -> RuntimeResult<Json<KbPromoteReport>> {
    service.kb().promote_branch(&name).await.map(Json)
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RollbackRequest {
    #[serde(default)]
    version: Option<u64>,
}

pub(crate) async fn rollback_kb(
    State(service): State<RuntimeService>,
    request: Option<Json<RollbackRequest>>,
) -> RuntimeResult<Json<KbRollbackReport>> {
    let version = request.as_ref().and_then(|request| request.0.version);
    service.kb().rollback_active(version).await.map(Json)
}
