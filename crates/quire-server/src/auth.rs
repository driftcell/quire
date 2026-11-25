//! Dual-layer bearer-token auth, user API key verification, and request-scoped agent context.

use axum::{
    Json,
    extract::{FromRequestParts, Request, State},
    http::{HeaderMap, StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use quire_protocol::{ApiErrorEnvelope, ProtocolError, SessionId};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use subtle::ConstantTimeEq;

use crate::state::{AppState, VerifiedExternalIdentity};

/// Per-request identity extracted from trusted HTTP headers and verified auth credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestContext {
    pub session_id: Option<SessionId>,
    /// Audit actor for revision history. Defaults to `agent`.
    pub actor: String,
    /// Authenticated user ID when authorized via a user API key or JWT.
    pub user_id: Option<String>,
    /// Whether the authenticated user holds administrator privileges.
    pub is_admin: bool,
    /// When authenticated via a short-lived preview JWT, the single `doc_id` it is permitted to read.
    pub preview_doc_id: Option<String>,
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::system()
    }
}

impl RequestContext {
    /// Extract `X-Session-Id`, `X-Actor` (legacy `X-Agent-Id`), and any verified user identity from request parts.
    pub fn from_parts(parts: &Parts) -> Result<Self, ProtocolError> {
        let session_id = parts
            .headers
            .get("x-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(SessionId::parse)
            .transpose()?;
        let verified_user = parts.extensions.get::<VerifiedExternalIdentity>();
        let actor = parts
            .headers
            .get("x-actor")
            .or_else(|| parts.headers.get("x-agent-id"))
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= 64
                    && value.chars().all(|ch| {
                        ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':' | ' ')
                    })
            })
            .or_else(|| {
                verified_user.map(|u| u.username.as_str()).filter(|name| {
                    !name.is_empty()
                        && name.len() <= 64
                        && name.chars().all(|ch| {
                            ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | ':' | ' ')
                        })
                })
            })
            .unwrap_or("agent")
            .to_string();
        let user_id = verified_user
            .filter(|u| u.preview_doc_id.is_none())
            .map(|u| u.user_id.clone());
        let is_admin = verified_user.is_some_and(|u| u.is_admin);
        let preview_doc_id = verified_user.and_then(|u| u.preview_doc_id.clone());
        Ok(Self {
            session_id,
            actor,
            user_id,
            is_admin,
            preview_doc_id,
        })
    }

    /// Unrestricted internal context for direct Rust callers and unit tests.
    pub fn system() -> Self {
        Self {
            session_id: None,
            actor: "agent".to_string(),
            user_id: None,
            is_admin: true,
            preview_doc_id: None,
        }
    }

    /// Resolve one MCP call's identity from transport headers and trusted per-call metadata.
    pub fn from_mcp(
        parts: &Parts,
        meta: Option<&rmcp::model::RequestMetaObject>,
    ) -> Result<Self, String> {
        let mut context = Self::from_parts(parts).map_err(|error| error.to_string())?;
        let Some(value) = meta.and_then(|meta| meta.0.0.get("quire")) else {
            return Ok(context);
        };
        let quire: QuireRequestMeta = serde_json::from_value(value.clone())
            .map_err(|error| format!("invalid _meta.quire identity: {error}"))?;
        if let Some(session_id) = quire.session_id {
            let parsed = SessionId::parse(&session_id).map_err(|error| error.to_string())?;
            if context
                .session_id
                .as_ref()
                .is_some_and(|header| header != &parsed)
            {
                return Err("X-Session-Id and _meta.quire.session_id disagree".into());
            }
            context.session_id = Some(parsed);
        }
        if let Some(agent_id) = quire.agent_id {
            let agent_id =
                valid_actor(&agent_id).ok_or_else(|| "invalid _meta.quire.agent_id".to_string())?;
            if parts.headers.contains_key("x-actor") || parts.headers.contains_key("x-agent-id") {
                if context.actor != agent_id {
                    return Err("X-Actor and _meta.quire.agent_id disagree".into());
                }
            } else {
                context.actor = agent_id;
            }
        }
        if let Some(run_id) = quire.run_id {
            let valid = !run_id.is_empty()
                && run_id.len() <= 128
                && run_id.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ':')
                });
            if !valid {
                return Err("invalid _meta.quire.run_id".into());
            }
        }
        Ok(context)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QuireRequestMeta {
    session_id: Option<String>,
    run_id: Option<String>,
    agent_id: Option<String>,
}

fn valid_actor(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()
        && value.len() <= 64
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | ':' | ' ')
        }))
    .then(|| value.to_string())
}

/// Extractor that requires the read-write token (`QUIRE_RW_TOKEN`), user JWT, or user API key.
///
/// Use on mutating handlers; middleware remains available for nested services such as `/mcp`
/// that cannot participate in Axum's extractor pipeline.
#[derive(Debug, Clone, Copy)]
pub struct RwAuth;

impl FromRequestParts<AppState> for RwAuth {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let identity = authorize(state, &parts.headers, Access::Rw)
            .await
            .map_err(|response| *response)?;
        if let Some(identity) = identity {
            parts.extensions.insert(identity);
        }
        Ok(Self)
    }
}

/// Extractor that accepts either the read-only token, read-write token, user JWT, or user API key.
#[derive(Debug, Clone, Copy)]
pub struct RoAuth;

impl FromRequestParts<AppState> for RoAuth {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let identity = authorize(state, &parts.headers, Access::Ro)
            .await
            .map_err(|response| *response)?;
        if let Some(identity) = identity {
            parts.extensions.insert(identity);
        }
        Ok(Self)
    }
}

impl<S> FromRequestParts<S> for RequestContext
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Self::from_parts(parts)
            .map_err(|error| auth_error(StatusCode::BAD_REQUEST, error.code(), &error.hint()))
    }
}

enum Access {
    Rw,
    Ro,
}

async fn authorize(
    state: &AppState,
    headers: &HeaderMap,
    access: Access,
) -> Result<Option<VerifiedExternalIdentity>, Box<Response>> {
    let configured_rw = state.config.rw_token.as_ref();
    let configured_ro = state.config.ro_token.as_ref();
    let configured_jwt = state.config.jwt_secret.as_ref();
    if configured_rw.is_none()
        && configured_ro.is_none()
        && configured_jwt.is_none()
        && state.harness_bridge.is_none()
    {
        return Err(Box::new(auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_not_configured",
            "Configure JWT_SECRET or QUIRE_RW_TOKEN before exposing the server.",
        )));
    }
    let Some(presented) = presented_token(headers) else {
        return Err(Box::new(missing_token()));
    };
    if let Some(secret) = configured_jwt
        && let Ok(claims) = crate::jwt::verify_jwt(secret.expose_secret().as_bytes(), presented)
    {
        if claims.role == "preview" {
            if matches!(access, Access::Ro)
                && let Some(preview_doc_id) = claims
                    .preview_doc_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
            {
                return Ok(Some(VerifiedExternalIdentity {
                    user_id: "preview".to_string(),
                    username: "preview".to_string(),
                    is_admin: false,
                    preview_doc_id: Some(preview_doc_id.to_string()),
                }));
            }
            return Err(Box::new(missing_token()));
        }
        if let Some(bridge) = state.harness_bridge.as_ref() {
            if let Some(identity) = bridge.verify_active_user(&claims.sub).await {
                return Ok(Some(identity));
            }
            return Err(Box::new(missing_token()));
        }
        return Ok(Some(VerifiedExternalIdentity {
            user_id: claims.sub,
            username: claims.username,
            is_admin: claims.role == "admin",
            preview_doc_id: None,
        }));
    }
    if let Some(bridge) = state.harness_bridge.as_ref()
        && let Some(identity) = bridge.verify_api_key(presented).await
    {
        return Ok(Some(identity));
    }
    let is_rw = configured_rw.is_some_and(|token| token_matches(token, presented));
    let allowed = match access {
        Access::Rw => is_rw,
        Access::Ro => is_rw || configured_ro.is_some_and(|token| token_matches(token, presented)),
    };
    if allowed {
        Ok(None)
    } else {
        Err(Box::new(missing_token()))
    }
}

fn token_matches(expected: &SecretString, presented: &str) -> bool {
    let expected = expected.expose_secret().as_bytes();
    let presented = presented.as_bytes();
    expected.len() == presented.len() && bool::from(expected.ct_eq(presented))
}

fn presented_token(headers: &HeaderMap) -> Option<&str> {
    if let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    {
        let value = value.trim();
        if !value.is_empty() {
            return Some(value);
        }
    }
    headers
        .get("x-quire-token")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn missing_token() -> Response {
    let mut response = auth_error(
        StatusCode::UNAUTHORIZED,
        "unauthorized",
        "Provide `Authorization: Bearer <api_key_or_token>` with a valid Quire API key or token.",
    );
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        header::HeaderValue::from_static("Bearer"),
    );
    response
}

fn auth_error(status: StatusCode, code: &str, hint: &str) -> Response {
    (
        status,
        Json(ApiErrorEnvelope::<()>::new(code, code, hint, None)),
    )
        .into_response()
}

/// Middleware for MCP and mutating routes.
pub async fn rw_auth(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    if request.method() == axum::http::Method::OPTIONS {
        return next.run(request).await;
    }
    match authorize(&state, request.headers(), Access::Rw).await {
        Ok(identity) => {
            if let Some(identity) = identity {
                request.extensions_mut().insert(identity);
            }
            next.run(request).await
        }
        Err(response) => *response,
    }
}

/// Middleware for read-only frontend routes (RO or RW token accepted).
pub async fn ro_auth(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    if request.method() == axum::http::Method::OPTIONS {
        return next.run(request).await;
    }
    match authorize(&state, request.headers(), Access::Ro).await {
        Ok(identity) => {
            if let Some(identity) = identity {
                request.extensions_mut().insert(identity);
            }
            next.run(request).await
        }
        Err(response) => *response,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn bearer_and_custom_header_are_supported() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret"),
        );
        assert_eq!(presented_token(&headers), Some("secret"));
        let mut headers = HeaderMap::new();
        headers.insert("x-quire-token", HeaderValue::from_static("other"));
        assert_eq!(presented_token(&headers), Some("other"));
        assert_eq!(presented_token(&HeaderMap::new()), None);
    }

    #[test]
    fn token_comparison_is_exact() {
        let secret = SecretString::from("abc");
        assert!(token_matches(&secret, "abc"));
        assert!(!token_matches(&secret, "abcd"));
    }

    #[test]
    fn request_context_extracts_headers_and_defaults_actor() {
        let request = axum::http::Request::builder()
            .header("x-session-id", "sess_01")
            .header("x-actor", "lead-agent")
            .body(())
            .expect("request");
        let (parts, ()) = request.into_parts();
        let context = RequestContext::from_parts(&parts).expect("context");
        assert_eq!(
            context.session_id.as_ref().map(SessionId::as_str),
            Some("sess_01")
        );
        assert_eq!(context.actor, "lead-agent");
        assert!(context.user_id.is_none());

        let request = axum::http::Request::builder().body(()).expect("request");
        let (parts, ()) = request.into_parts();
        let context = RequestContext::from_parts(&parts).expect("context");
        assert!(context.session_id.is_none());
        assert_eq!(context.actor, "agent");
    }

    #[test]
    fn mcp_meta_supplies_shared_connection_identity_and_rejects_mismatch() {
        let request = axum::http::Request::builder().body(()).expect("request");
        let (parts, ()) = request.into_parts();
        let mut meta = rmcp::model::RequestMetaObject::new();
        meta.insert(
            "quire".into(),
            serde_json::json!({
                "session_id": "sess_01",
                "run_id": "run_01",
                "agent_id": "writer"
            }),
        );
        let context = RequestContext::from_mcp(&parts, Some(&meta)).expect("meta context");
        assert_eq!(
            context.session_id.as_ref().map(SessionId::as_str),
            Some("sess_01")
        );
        assert_eq!(context.actor, "writer");

        let request = axum::http::Request::builder()
            .header("x-session-id", "sess_other")
            .body(())
            .expect("request");
        let (parts, ()) = request.into_parts();
        assert!(RequestContext::from_mcp(&parts, Some(&meta)).is_err());
    }

    #[test]
    fn request_context_rejects_malformed_session_ids() {
        let request = axum::http::Request::builder()
            .header("x-session-id", "not valid")
            .body(())
            .expect("request");
        let (parts, ()) = request.into_parts();
        assert!(RequestContext::from_parts(&parts).is_err());
    }
}
