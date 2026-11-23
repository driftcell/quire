use quire_protocol::short_id;
use sqlx::Row;

use super::*;
use crate::auth::{UserRecord, UserRole};

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
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE role = 'admin')")
                .fetch_one(&self.pool)
                .await?;
        if admin_exists {
            return Ok(false);
        }
        let user_id = short_id("usr");
        let inserted = sqlx::query(
            r#"INSERT INTO users (user_id, username, email, display_name, role, password_hash, enabled)
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
            r#"INSERT INTO users (user_id, username, email, display_name, role, password_hash, enabled)
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
               FROM users
               WHERE LOWER(username) = LOWER($1) OR LOWER(email) = LOWER($1)
               LIMIT 1"#,
        )
        .bind(identifier.trim())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(row_to_user).transpose()
    }

    pub async fn get_user_by_id(&self, user_id: &str) -> RuntimeResult<Option<UserRecord>> {
        let row = sqlx::query(
            r#"SELECT user_id, username, email, display_name, role, password_hash, enabled, created_at, updated_at
               FROM users
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
            r#"UPDATE users
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
            r#"UPDATE users
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
               FROM users
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
            r#"UPDATE users
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
