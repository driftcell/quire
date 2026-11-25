use secrecy::ExposeSecret;

use super::*;
use crate::auth::{
    ApiKeyPublic, CreatedApiKey, UserPublic, UserRole, generate_api_key_secret, hash_password,
    issue_user_jwt, normalize_optional_email, validate_password, validate_username,
    verify_password,
};

#[derive(Debug, Clone)]
pub struct AuthSuccess {
    pub token: String,
    pub user: UserPublic,
}

impl RuntimeService {
    /// Seed the initial administrator from environment variables once if no admin exists in DB.
    pub async fn seed_initial_admin(&self) -> RuntimeResult<bool> {
        let username = validate_username(&self.config.admin_username)?;
        let email = normalize_optional_email(self.config.admin_email.as_deref())?;
        let password = self.config.admin_password.expose_secret();
        validate_password(password)?;
        let password_hash = hash_password(password)?;
        self.store
            .ensure_initial_admin(&username, email.as_deref(), &username, &password_hash)
            .await
    }

    pub async fn register_user(
        &self,
        username: &str,
        email: Option<&str>,
        display_name: Option<&str>,
        password: &str,
    ) -> RuntimeResult<AuthSuccess> {
        let username = validate_username(username)?;
        let email = normalize_optional_email(email)?;
        validate_password(password)?;
        let display_name = display_name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&username)
            .chars()
            .take(64)
            .collect::<String>();
        let password_hash = hash_password(password)?;
        let record = self
            .store
            .create_user(
                &username,
                email.as_deref(),
                &display_name,
                UserRole::User,
                &password_hash,
            )
            .await?;
        let user = UserPublic::from(record);
        let token = issue_user_jwt(&self.config, &user)?;
        Ok(AuthSuccess { token, user })
    }

    pub async fn login_user(&self, identifier: &str, password: &str) -> RuntimeResult<AuthSuccess> {
        let identifier = identifier.trim();
        if identifier.is_empty() || password.is_empty() {
            return Err(RuntimeError::InvalidRequest(
                "username/email and password are required".into(),
            ));
        }
        let Some(record) = self.store.find_user_for_login(identifier).await? else {
            return Err(RuntimeError::Unauthorized(
                "invalid username or password".into(),
            ));
        };
        if !record.enabled {
            return Err(RuntimeError::Forbidden("account is disabled".into()));
        }
        if !verify_password(password, &record.password_hash) {
            return Err(RuntimeError::Unauthorized(
                "invalid username or password".into(),
            ));
        }
        let user = UserPublic::from(record);
        let token = issue_user_jwt(&self.config, &user)?;
        Ok(AuthSuccess { token, user })
    }

    pub async fn get_user_profile(&self, user_id: &str) -> RuntimeResult<UserPublic> {
        let record = self
            .store
            .get_user_by_id(user_id)
            .await?
            .ok_or_else(|| RuntimeError::Unauthorized("user account not found".into()))?;
        if !record.enabled {
            return Err(RuntimeError::Forbidden("account is disabled".into()));
        }
        Ok(UserPublic::from(record))
    }

    pub async fn change_user_password(
        &self,
        user_id: &str,
        current_password: &str,
        new_password: &str,
    ) -> RuntimeResult<()> {
        validate_password(new_password)?;
        let record = self
            .store
            .get_user_by_id(user_id)
            .await?
            .ok_or_else(|| RuntimeError::Unauthorized("user account not found".into()))?;
        if !verify_password(current_password, &record.password_hash) {
            return Err(RuntimeError::Unauthorized(
                "current password is incorrect".into(),
            ));
        }
        let new_hash = hash_password(new_password)?;
        self.store.update_user_password(user_id, &new_hash).await
    }

    pub async fn update_user_profile(
        &self,
        user_id: &str,
        display_name: Option<&str>,
        email: Option<Option<&str>>,
    ) -> RuntimeResult<UserPublic> {
        let cleaned_name = display_name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.chars().take(64).collect::<String>());
        let cleaned_email = match email {
            Some(Some(raw)) => Some(normalize_optional_email(Some(raw))?),
            Some(None) => Some(None),
            None => None,
        };
        let record = self
            .store
            .update_user_profile(
                user_id,
                cleaned_name.as_deref(),
                cleaned_email.as_ref().map(|v| v.as_deref()),
            )
            .await?;
        Ok(UserPublic::from(record))
    }

    pub async fn list_users(&self) -> RuntimeResult<Vec<UserPublic>> {
        let records = self.store.list_users().await?;
        Ok(records.into_iter().map(UserPublic::from).collect())
    }

    pub async fn admin_update_user(
        &self,
        user_id: &str,
        role: Option<UserRole>,
        enabled: Option<bool>,
    ) -> RuntimeResult<UserPublic> {
        let record = self.store.admin_update_user(user_id, role, enabled).await?;
        Ok(UserPublic::from(record))
    }

    pub async fn create_api_key(&self, user_id: &str, name: &str) -> RuntimeResult<CreatedApiKey> {
        let cleaned = name.trim();
        let effective_name = if cleaned.is_empty() {
            "默认外部代理密钥"
        } else {
            cleaned
        };
        if effective_name.chars().count() > 64 {
            return Err(RuntimeError::InvalidRequest(
                "API key name must be 1-64 characters".into(),
            ));
        }
        let (secret_key, key_prefix, key_hash) = generate_api_key_secret()?;
        let key = self
            .store
            .create_api_key(user_id, effective_name, &key_prefix, &key_hash)
            .await?;
        Ok(CreatedApiKey { key, secret_key })
    }

    pub async fn list_api_keys(&self, user_id: &str) -> RuntimeResult<Vec<ApiKeyPublic>> {
        self.store.list_api_keys(user_id).await
    }

    pub async fn delete_api_key(&self, user_id: &str, key_id: &str) -> RuntimeResult<()> {
        let deleted = self.store.delete_api_key(user_id, key_id).await?;
        if !deleted {
            return Err(RuntimeError::InvalidRequest("API key not found".into()));
        }
        Ok(())
    }
}
