//! SaaS user authentication: PBKDF2-SHA256 password hashing and HS256 JWT tokens.

use std::num::NonZeroU32;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use quire_server::jwt::{JwtClaims, sign_jwt, verify_jwt};
use ring::{
    pbkdf2,
    rand::{SecureRandom, SystemRandom},
};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
    error::{RuntimeError, RuntimeResult},
};

const PBKDF2_ITERATIONS: u32 = 100_000;
const SALT_LEN: usize = 16;
const HASH_LEN: usize = 32;
const PREVIEW_JWT_TTL_SECS: i64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    Admin,
    User,
}

impl UserRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::User => "user",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "admin" => Some(Self::Admin),
            "user" => Some(Self::User),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UserRecord {
    pub user_id: String,
    pub username: String,
    pub email: Option<String>,
    pub display_name: String,
    pub role: UserRole,
    pub password_hash: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPublic {
    pub user_id: String,
    pub username: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    pub display_name: String,
    pub role: UserRole,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<UserRecord> for UserPublic {
    fn from(record: UserRecord) -> Self {
        Self {
            user_id: record.user_id,
            username: record.username,
            email: record.email,
            display_name: record.display_name,
            role: record.role,
            enabled: record.enabled,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

/// Authenticated user context extracted from a verified SaaS JWT.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: String,
    pub username: String,
    pub role: UserRole,
}

pub fn hash_password(password: &str) -> RuntimeResult<String> {
    let rng = SystemRandom::new();
    let mut salt = [0u8; SALT_LEN];
    rng.fill(&mut salt)
        .map_err(|_| RuntimeError::Internal("failed to generate password salt".into()))?;

    let iterations = NonZeroU32::new(PBKDF2_ITERATIONS)
        .ok_or_else(|| RuntimeError::Internal("invalid PBKDF2 iteration count".into()))?;
    let mut derived = [0u8; HASH_LEN];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        iterations,
        &salt,
        password.as_bytes(),
        &mut derived,
    );

    let salt_b64 = URL_SAFE_NO_PAD.encode(salt);
    let hash_b64 = URL_SAFE_NO_PAD.encode(derived);
    Ok(format!(
        "pbkdf2_sha256${PBKDF2_ITERATIONS}${salt_b64}${hash_b64}"
    ))
}

pub fn verify_password(password: &str, stored_hash: &str) -> bool {
    let mut parts = stored_hash.split('$');
    let Some("pbkdf2_sha256") = parts.next() else {
        return false;
    };
    let Some(iter_raw) = parts.next() else {
        return false;
    };
    let Some(salt_b64) = parts.next() else {
        return false;
    };
    let Some(hash_b64) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        return false;
    }

    let Ok(iter_num) = iter_raw.parse::<u32>() else {
        return false;
    };
    let Some(iterations) = NonZeroU32::new(iter_num) else {
        return false;
    };
    let Ok(salt) = URL_SAFE_NO_PAD.decode(salt_b64) else {
        return false;
    };
    let Ok(expected_hash) = URL_SAFE_NO_PAD.decode(hash_b64) else {
        return false;
    };

    pbkdf2::verify(
        pbkdf2::PBKDF2_HMAC_SHA256,
        iterations,
        &salt,
        password.as_bytes(),
        &expected_hash,
    )
    .is_ok()
}

pub fn issue_user_jwt(config: &Config, user: &UserPublic) -> RuntimeResult<String> {
    let now = Utc::now().timestamp();
    let ttl = i64::try_from(config.jwt_ttl_secs).unwrap_or(604_800);
    let claims = JwtClaims {
        sub: user.user_id.clone(),
        username: user.username.clone(),
        role: user.role.as_str().to_string(),
        iat: now,
        exp: now + ttl,
    };
    sign_jwt(config.jwt_secret.expose_secret().as_bytes(), &claims).map_err(RuntimeError::Internal)
}

pub fn issue_preview_jwt(config: &Config) -> RuntimeResult<String> {
    let now = Utc::now().timestamp();
    let claims = JwtClaims {
        sub: "preview".to_string(),
        username: "preview".to_string(),
        role: "preview".to_string(),
        iat: now,
        exp: now + PREVIEW_JWT_TTL_SECS,
    };
    sign_jwt(config.jwt_secret.expose_secret().as_bytes(), &claims).map_err(RuntimeError::Internal)
}

pub fn verify_token(config: &Config, token: &str) -> Result<JwtClaims, String> {
    verify_jwt(config.jwt_secret.expose_secret().as_bytes(), token)
}

pub fn validate_username(raw: &str) -> RuntimeResult<String> {
    let trimmed = raw.trim();
    if trimmed.len() < 3 || trimmed.len() > 32 {
        return Err(RuntimeError::InvalidRequest(
            "username must be 3-32 characters".into(),
        ));
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        return Err(RuntimeError::InvalidRequest(
            "username may only contain ASCII letters, digits, '_', '-', or '.'".into(),
        ));
    }
    Ok(trimmed.to_string())
}

pub fn validate_password(password: &str) -> RuntimeResult<()> {
    if password.len() < 6 || password.len() > 128 {
        return Err(RuntimeError::InvalidRequest(
            "password must be 6-128 characters".into(),
        ));
    }
    Ok(())
}

pub fn normalize_optional_email(raw: Option<&str>) -> RuntimeResult<Option<String>> {
    let Some(trimmed) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if trimmed.len() > 128 || !trimmed.contains('@') || trimmed.contains(' ') {
        return Err(RuntimeError::InvalidRequest("invalid email address".into()));
    }
    Ok(Some(trimmed.to_ascii_lowercase()))
}
