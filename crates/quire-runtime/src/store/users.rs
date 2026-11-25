use quire_protocol::short_id;
use sqlx::Row;

use super::*;
use crate::auth::{ApiKeyPublic, AuthUser, UserRecord, UserRole, hash_api_key};

impl RuntimeStore {
    /// Seed the initial administrator from environment variables ONLY if no admin exists yet.
    /// Once initialized, the admin account and password live in PostgreSQL and are never overwritten.
    pub async fn ensure_initial_admin(
        &self,
        username: &str,
        email: Option<&str>,
        display_name: &str,
        password_hash: &str,
    ) -> RuntimeResult<bool> {
        let admin_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM identity.users WHERE role = 'admin' AND user_id <> 'usr_test_admin')")
                .fetch_one(&self.pool)
                .await?;
        if admin_exists {
            return Ok(false);
        }
        let user_id = short_id("usr");
        let inserted = sqlx::query(
            r#"INSERT INTO identity.users (user_id, username, email, display_name, role, password_hash, enabled)
               VALUES ($1, $2, $3, $4, 'admin', $5, TRUE)
               ON CONFLICT DO NOTHING"#,
        )
        .bind(&user_id)
        .bind(username)
        .bind(email)
        .bind(display_name)
        .bind(password_hash)
        .execute(&self.pool)
        .await?;
        Ok(inserted.rows_affected() > 0)
    }

    pub async fn create_user(
        &self,
        username: &str,
        email: Option<&str>,
        display_name: &str,
        role: UserRole,
        password_hash: &str,
    ) -> RuntimeResult<UserRecord> {
        let user_id = short_id("usr");
        let row = sqlx::query(
            r#"INSERT INTO identity.users (user_id, username, email, display_name, role, password_hash, enabled)
               VALUES ($1, $2, $3, $4, $5, $6, TRUE)
               RETURNING user_id, username, email, display_name, role, password_hash, enabled, created_at, updated_at"#,
        )
        .bind(&user_id)
        .bind(username)
        .bind(email)
        .bind(display_name)
        .bind(role.as_str())
        .bind(password_hash)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| match &error {
            sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some("23505") => {
                RuntimeError::Conflict("username or email already registered".into())
            }
            _ => RuntimeError::Database(error),
        })?;
        row_to_user(&row)
    }

    pub async fn find_user_for_login(&self, identifier: &str) -> RuntimeResult<Option<UserRecord>> {
        let row = sqlx::query(
            r#"SELECT user_id, username, email, display_name, role, password_hash, enabled, created_at, updated_at
               FROM identity.users
               WHERE LOWER(username) = LOWER($1) OR LOWER(email) = LOWER($1)
               LIMIT 1"#,
        )
        .bind(identifier.trim())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(row_to_user).transpose()
    }

    pub async fn verify_active_user(&self, user_id: &str) -> RuntimeResult<Option<AuthUser>> {
        let row = sqlx::query(
            r#"SELECT user_id, username, role, enabled
               FROM identity.users
               WHERE user_id = $1 AND enabled = TRUE"#,
        )
        .bind(user_id.trim())
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let role_raw: String = row.try_get("role")?;
        let role = UserRole::parse(&role_raw).unwrap_or(UserRole::User);
        Ok(Some(AuthUser {
            user_id: row.try_get("user_id")?,
            username: row.try_get("username")?,
            role,
        }))
    }

    pub async fn get_user_by_id(&self, user_id: &str) -> RuntimeResult<Option<UserRecord>> {
        let row = sqlx::query(
            r#"SELECT user_id, username, email, display_name, role, password_hash, enabled, created_at, updated_at
               FROM identity.users
               WHERE user_id = $1"#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(row_to_user).transpose()
    }

    pub async fn update_user_password(
        &self,
        user_id: &str,
        new_password_hash: &str,
    ) -> RuntimeResult<()> {
        let result = sqlx::query(
            r#"UPDATE identity.users
               SET password_hash = $2, updated_at = NOW()
               WHERE user_id = $1"#,
        )
        .bind(user_id)
        .bind(new_password_hash)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(RuntimeError::InvalidRequest("user not found".into()));
        }
        Ok(())
    }

    pub async fn update_user_profile(
        &self,
        user_id: &str,
        display_name: Option<&str>,
        email: Option<Option<&str>>,
    ) -> RuntimeResult<UserRecord> {
        let current = self
            .get_user_by_id(user_id)
            .await?
            .ok_or_else(|| RuntimeError::InvalidRequest("user not found".into()))?;
        let next_name = display_name.unwrap_or(&current.display_name);
        let next_email = match email {
            Some(v) => v,
            None => current.email.as_deref(),
        };
        let row = sqlx::query(
            r#"UPDATE identity.users
               SET display_name = $2, email = $3, updated_at = NOW()
               WHERE user_id = $1
               RETURNING user_id, username, email, display_name, role, password_hash, enabled, created_at, updated_at"#,
        )
        .bind(user_id)
        .bind(next_name)
        .bind(next_email)
        .fetch_one(&self.pool)
        .await
        .map_err(|error| match &error {
            sqlx::Error::Database(db_err) if db_err.code().as_deref() == Some("23505") => {
                RuntimeError::Conflict("email is already in use".into())
            }
            _ => RuntimeError::Database(error),
        })?;
        row_to_user(&row)
    }

    pub async fn list_users(&self) -> RuntimeResult<Vec<UserRecord>> {
        let rows = sqlx::query(
            r#"SELECT user_id, username, email, display_name, role, password_hash, enabled, created_at, updated_at
               FROM identity.users
               ORDER BY created_at ASC"#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_user).collect()
    }

    pub async fn admin_update_user(
        &self,
        user_id: &str,
        role: Option<UserRole>,
        enabled: Option<bool>,
    ) -> RuntimeResult<UserRecord> {
        let current = self
            .get_user_by_id(user_id)
            .await?
            .ok_or_else(|| RuntimeError::InvalidRequest("user not found".into()))?;
        let next_role = role.unwrap_or(current.role);
        let next_enabled = enabled.unwrap_or(current.enabled);
        let row = sqlx::query(
            r#"UPDATE identity.users
               SET role = $2, enabled = $3, updated_at = NOW()
               WHERE user_id = $1
               RETURNING user_id, username, email, display_name, role, password_hash, enabled, created_at, updated_at"#,
        )
        .bind(user_id)
        .bind(next_role.as_str())
        .bind(next_enabled)
        .fetch_one(&self.pool)
        .await?;
        row_to_user(&row)
    }

    pub async fn create_api_key(
        &self,
        user_id: &str,
        name: &str,
        key_prefix: &str,
        key_hash: &str,
    ) -> RuntimeResult<ApiKeyPublic> {
        let key_id = short_id("key");
        let row = sqlx::query(
            r#"INSERT INTO identity.api_keys (key_id, user_id, name, key_prefix, key_hash)
               VALUES ($1, $2, $3, $4, $5)
               RETURNING key_id, user_id, name, key_prefix, last_used_at, created_at"#,
        )
        .bind(&key_id)
        .bind(user_id)
        .bind(name)
        .bind(key_prefix)
        .bind(key_hash)
        .fetch_one(&self.pool)
        .await?;
        row_to_api_key(&row)
    }

    pub async fn list_api_keys(&self, user_id: &str) -> RuntimeResult<Vec<ApiKeyPublic>> {
        let rows = sqlx::query(
            r#"SELECT key_id, user_id, name, key_prefix, last_used_at, created_at
               FROM identity.api_keys
               WHERE user_id = $1
               ORDER BY created_at DESC, key_id ASC"#,
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_api_key).collect()
    }

    pub async fn delete_api_key(&self, user_id: &str, key_id: &str) -> RuntimeResult<bool> {
        let result = sqlx::query(
            r#"DELETE FROM identity.api_keys
               WHERE key_id = $1 AND user_id = $2"#,
        )
        .bind(key_id)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn verify_api_key(&self, raw_token: &str) -> RuntimeResult<Option<AuthUser>> {
        let trimmed = raw_token.trim();
        if !trimmed.starts_with("qk_") {
            return Ok(None);
        }
        let key_hash = hash_api_key(trimmed);
        let row = sqlx::query(
            r#"UPDATE identity.api_keys AS k
               SET last_used_at = NOW()
               FROM identity.users AS u
               WHERE k.key_hash = $1
                 AND u.user_id = k.user_id
                 AND u.enabled = TRUE
               RETURNING u.user_id, u.username, u.role"#,
        )
        .bind(&key_hash)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let role_raw: String = row.try_get("role")?;
        let Some(role) = UserRole::parse(&role_raw) else {
            return Ok(None);
        };
        Ok(Some(AuthUser {
            user_id: row.try_get("user_id")?,
            username: row.try_get("username")?,
            role,
        }))
    }
}

fn row_to_user(row: &PgRow) -> RuntimeResult<UserRecord> {
    let role_raw: String = row.try_get("role")?;
    let role = UserRole::parse(&role_raw)
        .ok_or_else(|| RuntimeError::Internal(format!("invalid user role `{role_raw}`")))?;
    Ok(UserRecord {
        user_id: row.try_get("user_id")?,
        username: row.try_get("username")?,
        email: row.try_get("email")?,
        display_name: row.try_get("display_name")?,
        role,
        password_hash: row.try_get("password_hash")?,
        enabled: row.try_get("enabled")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn row_to_api_key(row: &PgRow) -> RuntimeResult<ApiKeyPublic> {
    Ok(ApiKeyPublic {
        key_id: row.try_get("key_id")?,
        user_id: row.try_get("user_id")?,
        name: row.try_get("name")?,
        key_prefix: row.try_get("key_prefix")?,
        last_used_at: row.try_get("last_used_at")?,
        created_at: row.try_get("created_at")?,
    })
}
