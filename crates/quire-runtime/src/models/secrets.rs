use base64::Engine as _;
use ring::{
    aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey},
    digest::{SHA256, digest},
    rand::{SecureRandom, SystemRandom},
};
use secrecy::{ExposeSecret, SecretString};
use sqlx::Row;

use crate::error::{RuntimeError, RuntimeResult};

use super::{ModelRecord, ProviderProtocol, ProviderRecord};

pub(crate) fn row_to_provider(row: &sqlx::postgres::PgRow) -> RuntimeResult<ProviderRecord> {
    let protocol_raw: String = row.try_get("protocol")?;
    Ok(ProviderRecord {
        provider_id: row.try_get("provider_id")?,
        name: row.try_get("name")?,
        protocol: ProviderProtocol::parse(&protocol_raw).ok_or_else(|| {
            RuntimeError::Internal(format!("invalid provider protocol `{protocol_raw}`"))
        })?,
        base_url: row.try_get("base_url")?,
        api_key_masked: mask_api_key_ref(&row.try_get::<String, _>("api_key_ref")?),
        enabled: row.try_get("enabled")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub(crate) fn row_to_model(row: &sqlx::postgres::PgRow) -> RuntimeResult<ModelRecord> {
    Ok(ModelRecord {
        id: row.try_get("id")?,
        provider_id: row.try_get("provider_id")?,
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
        enabled: row.try_get("enabled")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn mask_api_key_ref(value: &str) -> String {
    if value.starts_with("env:") {
        value.to_string()
    } else if value.starts_with("enc:") {
        "enc:***".to_string()
    } else {
        "***".to_string()
    }
}

/// Mask a generic `env:` / `enc:` / raw secret reference for public records.
pub fn mask_secret_ref(value: &str) -> String {
    mask_api_key_ref(value)
}

/// Normalize a generic secret reference, encrypting raw values with the runtime master key.
pub fn normalize_secret_ref(
    value: &str,
    master_key: Option<&SecretString>,
) -> RuntimeResult<String> {
    normalize_api_key_ref(value, master_key)
}

pub(crate) fn normalize_api_key_ref(
    value: &str,
    master_key: Option<&SecretString>,
) -> RuntimeResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(RuntimeError::InvalidRequest(
            "api_key_ref must not be empty".into(),
        ));
    }
    if trimmed.starts_with("env:") || trimmed.starts_with("enc:") {
        return Ok(trimmed.to_string());
    }
    let Some(master) = master_key else {
        return Err(RuntimeError::InvalidRequest(
            "raw api keys require RUNTIME_MASTER_KEY, or use api_key_ref=env:VAR".into(),
        ));
    };
    encrypt_secret(trimmed, master)
}

pub fn resolve_api_key_ref(
    value: &str,
    master_key: Option<&SecretString>,
) -> RuntimeResult<SecretString> {
    if let Some(name) = value.strip_prefix("env:") {
        let secret = std::env::var(name).map_err(|_| {
            RuntimeError::InvalidRequest(format!("environment variable `{name}` is not set"))
        })?;
        return Ok(SecretString::from(secret));
    }
    if value.starts_with("enc:") {
        let master = master_key.ok_or_else(|| {
            RuntimeError::InvalidRequest(
                "RUNTIME_MASTER_KEY is required to decrypt provider credentials".into(),
            )
        })?;
        return Ok(SecretString::from(decrypt_secret(value, master)?));
    }
    Err(RuntimeError::InvalidRequest(
        "api_key_ref must be env:VAR or enc:...".into(),
    ))
}

pub fn encrypt_secret(plaintext: &str, master_key: &SecretString) -> RuntimeResult<String> {
    let key = master_aead_key(master_key)?;
    let mut nonce_bytes = [0u8; 12];
    SystemRandom::new()
        .fill(&mut nonce_bytes)
        .map_err(|_| RuntimeError::Internal("failed to generate encryption nonce".into()))?;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    let mut buffer = plaintext.as_bytes().to_vec();
    key.seal_in_place_append_tag(nonce, Aad::empty(), &mut buffer)
        .map_err(|_| RuntimeError::Internal("failed to encrypt credential".into()))?;
    let mut combined = nonce_bytes.to_vec();
    combined.extend_from_slice(&buffer);
    Ok(format!(
        "enc:{}",
        base64::engine::general_purpose::STANDARD.encode(combined)
    ))
}

pub fn decrypt_secret(ciphertext: &str, master_key: &SecretString) -> RuntimeResult<String> {
    let encoded = ciphertext.strip_prefix("enc:").ok_or_else(|| {
        RuntimeError::InvalidRequest("encrypted credential must start with enc:".into())
    })?;
    let combined = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| RuntimeError::InvalidRequest(format!("invalid encrypted ref: {error}")))?;
    let nonce_bytes: [u8; 12] = combined
        .get(..12)
        .and_then(|slice| <[u8; 12]>::try_from(slice).ok())
        .ok_or_else(|| RuntimeError::InvalidRequest("encrypted ref is truncated".into()))?;
    let mut buffer = combined
        .get(12..)
        .ok_or_else(|| RuntimeError::InvalidRequest("encrypted ref is truncated".into()))?
        .to_vec();
    let key = master_aead_key(master_key)?;
    let plaintext = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce_bytes),
            Aad::empty(),
            &mut buffer,
        )
        .map_err(|_| RuntimeError::InvalidRequest("credential decryption failed".into()))?;
    String::from_utf8(plaintext.to_vec())
        .map_err(|_| RuntimeError::InvalidRequest("decrypted credential is not UTF-8".into()))
}

fn master_aead_key(master_key: &SecretString) -> RuntimeResult<LessSafeKey> {
    let key = digest(&SHA256, master_key.expose_secret().as_bytes());
    let unbound = UnboundKey::new(&AES_256_GCM, key.as_ref())
        .map_err(|_| RuntimeError::Internal("failed to derive credential encryption key".into()))?;
    Ok(LessSafeKey::new(unbound))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_refs_hide_and_roundtrip_secret_values() {
        let master = SecretString::from("master-key-for-tests");
        let encrypted = encrypt_secret("sk-super-secret", &master).unwrap();
        assert!(encrypted.starts_with("enc:"));
        assert!(!encrypted.contains("sk-super-secret"));
        assert_eq!(
            decrypt_secret(&encrypted, &master).unwrap(),
            "sk-super-secret"
        );
        assert_eq!(mask_api_key_ref(&encrypted), "enc:***");
        assert_eq!(
            mask_api_key_ref("env:DEEPSEEK_API_KEY"),
            "env:DEEPSEEK_API_KEY"
        );
    }

    #[test]
    fn env_refs_resolve_without_exposing_the_value() {
        let resolved = resolve_api_key_ref("env:PATH", None).unwrap();
        assert!(!resolved.expose_secret().is_empty());
        assert!(resolve_api_key_ref("env:RUNTIME_MISSING_VAR_FOR_TEST", None).is_err());
    }
}
