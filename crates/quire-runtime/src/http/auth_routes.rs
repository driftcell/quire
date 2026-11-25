use axum::Extension;

use super::*;
use crate::auth::{ApiKeyPublic, AuthUser, CreatedApiKey, UserPublic, UserRole};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RegisterRequest {
    pub username: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    pub password: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LoginRequest {
    #[serde(alias = "username", alias = "email")]
    pub identifier: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct AuthResponse {
    pub token: String,
    pub user: UserPublic,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UpdateProfileRequest {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub email: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AdminUpdateUserRequest {
    #[serde(default)]
    pub role: Option<UserRole>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CreateApiKeyRequest {
    #[serde(default)]
    pub name: String,
}

pub(crate) async fn register(
    State(service): State<RuntimeService>,
    Json(body): Json<RegisterRequest>,
) -> RuntimeResult<(StatusCode, Json<AuthResponse>)> {
    let result = service
        .register_user(
            &body.username,
            body.email.as_deref(),
            body.display_name.as_deref(),
            &body.password,
        )
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(AuthResponse {
            token: result.token,
            user: result.user,
        }),
    ))
}

pub(crate) async fn login(
    State(service): State<RuntimeService>,
    Json(body): Json<LoginRequest>,
) -> RuntimeResult<Json<AuthResponse>> {
    let result = service.login_user(&body.identifier, &body.password).await?;
    Ok(Json(AuthResponse {
        token: result.token,
        user: result.user,
    }))
}

pub(crate) async fn me(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
) -> RuntimeResult<Json<UserPublic>> {
    let user = service.get_user_profile(&auth_user.user_id).await?;
    Ok(Json(user))
}

pub(crate) async fn change_password(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Json(body): Json<ChangePasswordRequest>,
) -> RuntimeResult<StatusCode> {
    service
        .change_user_password(
            &auth_user.user_id,
            &body.current_password,
            &body.new_password,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn update_profile(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Json(body): Json<UpdateProfileRequest>,
) -> RuntimeResult<Json<UserPublic>> {
    let user = service
        .update_user_profile(
            &auth_user.user_id,
            body.display_name.as_deref(),
            body.email.as_ref().map(|v| v.as_deref()),
        )
        .await?;
    Ok(Json(user))
}

pub(crate) async fn list_api_keys(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
) -> RuntimeResult<Json<Vec<ApiKeyPublic>>> {
    let keys = service.list_api_keys(&auth_user.user_id).await?;
    Ok(Json(keys))
}

pub(crate) async fn create_api_key(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Json(body): Json<CreateApiKeyRequest>,
) -> RuntimeResult<(StatusCode, Json<CreatedApiKey>)> {
    let created = service
        .create_api_key(&auth_user.user_id, &body.name)
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

pub(crate) async fn delete_api_key(
    State(service): State<RuntimeService>,
    Extension(auth_user): Extension<AuthUser>,
    Path(key_id): Path<String>,
) -> RuntimeResult<StatusCode> {
    service.delete_api_key(&auth_user.user_id, &key_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn admin_list_users(
    State(service): State<RuntimeService>,
) -> RuntimeResult<Json<Vec<UserPublic>>> {
    let users = service.list_users().await?;
    Ok(Json(users))
}

pub(crate) async fn admin_update_user(
    State(service): State<RuntimeService>,
    Path(user_id): Path<String>,
    Json(body): Json<AdminUpdateUserRequest>,
) -> RuntimeResult<Json<UserPublic>> {
    let user = service
        .admin_update_user(&user_id, body.role, body.enabled)
        .await?;
    Ok(Json(user))
}
