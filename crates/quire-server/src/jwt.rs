//! Minimal HS256 JWT signing and verification backed by `ring::hmac`.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use ring::hmac;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JwtClaims {
    pub sub: String,
    pub username: String,
    pub role: String,
    pub iat: i64,
    pub exp: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_doc_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_chart_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JwtHeader {
    alg: String,
    typ: String,
}

/// Sign an HS256 JWT with the provided secret bytes.
pub fn sign_jwt(secret: &[u8], claims: &JwtClaims) -> Result<String, String> {
    let header = JwtHeader {
        alg: "HS256".to_string(),
        typ: "JWT".to_string(),
    };
    let header_json =
        serde_json::to_vec(&header).map_err(|error| format!("serialize jwt header: {error}"))?;
    let claims_json =
        serde_json::to_vec(claims).map_err(|error| format!("serialize jwt claims: {error}"))?;

    let header_b64 = URL_SAFE_NO_PAD.encode(header_json);
    let claims_b64 = URL_SAFE_NO_PAD.encode(claims_json);
    let signing_input = format!("{header_b64}.{claims_b64}");

    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    let signature = hmac::sign(&key, signing_input.as_bytes());
    let sig_b64 = URL_SAFE_NO_PAD.encode(signature.as_ref());

    Ok(format!("{signing_input}.{sig_b64}"))
}

/// Verify an HS256 JWT signature and expiration timestamp.
pub fn verify_jwt(secret: &[u8], token: &str) -> Result<JwtClaims, String> {
    if secret.is_empty() {
        return Err("empty JWT secret".into());
    }
    let mut parts = token.split('.');
    let Some(header_b64) = parts.next() else {
        return Err("malformed JWT".into());
    };
    let Some(claims_b64) = parts.next() else {
        return Err("malformed JWT".into());
    };
    let Some(sig_b64) = parts.next() else {
        return Err("malformed JWT".into());
    };
    if parts.next().is_some() {
        return Err("malformed JWT".into());
    }

    let header_bytes = URL_SAFE_NO_PAD
        .decode(header_b64)
        .map_err(|_| "invalid JWT header encoding".to_string())?;
    let header: JwtHeader =
        serde_json::from_slice(&header_bytes).map_err(|_| "invalid JWT header JSON".to_string())?;
    if header.alg != "HS256" {
        return Err("unsupported JWT algorithm".into());
    }

    let sig_bytes = URL_SAFE_NO_PAD
        .decode(sig_b64)
        .map_err(|_| "invalid JWT signature encoding".to_string())?;
    let signing_input = format!("{header_b64}.{claims_b64}");
    let key = hmac::Key::new(hmac::HMAC_SHA256, secret);
    hmac::verify(&key, signing_input.as_bytes(), &sig_bytes)
        .map_err(|_| "invalid JWT signature".to_string())?;

    let claims_bytes = URL_SAFE_NO_PAD
        .decode(claims_b64)
        .map_err(|_| "invalid JWT claims encoding".to_string())?;
    let claims: JwtClaims =
        serde_json::from_slice(&claims_bytes).map_err(|_| "invalid JWT claims JSON".to_string())?;

    let now = Utc::now().timestamp();
    if claims.exp <= now {
        return Err("JWT expired".into());
    }
    Ok(claims)
}
