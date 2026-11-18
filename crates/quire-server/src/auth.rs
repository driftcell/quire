//! Dual-layer bearer-token auth and request-scoped agent context.

use axum::{
    Json,
    extract::{FromRequestParts, Request, State},
    http::{HeaderMap, StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use quire_protocol::{ProtocolError, SessionId};
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;
use subtle::ConstantTimeEq;

use crate::state::AppState;

/// Per-request identity extracted from trusted HTTP headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestContext {
    pub session_id: Option<SessionId>,
    /// Audit actor for revision history. Defaults to `agent`.
    pub actor: String,
}

impl RequestContext {
    /// Extract `X-Session-Id` and `X-Actor` (legacy `X-Agent-Id`) from request headers.
    pub fn from_parts(parts: &Parts) -> Result<Self, ProtocolError> {
        let session_id = parts
            .headers
            .get("x-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(SessionId::parse)
            .transpose()?;
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
            .unwrap_or("agent")
            .to_string();
        Ok(Self { session_id, actor })
    }
}

/// Extractor that requires the read-write token (`QUIRE_RW_TOKEN`).
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
        authorize(state, &parts.headers, Access::Rw)
            .await
            .map_err(|response| *response)?;
        Ok(Self)
    }
}

/// Extractor that accepts either the read-only token or the read-write token.
#[derive(Debug, Clone, Copy)]
pub struct RoAuth;

impl FromRequestParts<AppState> for RoAuth {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        authorize(state, &parts.headers, Access::Ro)
            .await
            .map_err(|response| *response)?;
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
) -> Result<(), Box<Response>> {
    let configured_rw = state.config.rw_token.as_ref();
    let configured_ro = state.config.ro_token.as_ref();
    if configured_rw.is_none() && configured_ro.is_none() {
        return Err(Box::new(auth_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_not_configured",
            "Set QUIRE_RW_TOKEN and QUIRE_RO_TOKEN before exposing the server.",
        )));
    }
    let Some(presented) = presented_token(headers) else {
        return Err(Box::new(missing_token()));
    };
    let allowed = match access {
        Access::Rw => configured_rw.is_some_and(|token| token_matches(token, presented)),
        Access::Ro => {
            configured_ro.is_some_and(|token| token_matches(token, presented))
                || configured_rw.is_some_and(|token| token_matches(token, presented))
        }
    };
    if allowed {
        Ok(())
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
        "Provide `Authorization: Bearer <token>` with a read-only or read-write token.",
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
        Json(json!({
            "error": {
                "code": code,
                "message": code,
                "hint": hint,
            }
        })),
    )
        .into_response()
}

/// Middleware for MCP and mutating routes.
pub async fn rw_auth(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if request.method() == axum::http::Method::OPTIONS {
        return next.run(request).await;
    }
    match authorize(&state, request.headers(), Access::Rw).await {
        Ok(()) => next.run(request).await,
        Err(response) => *response,
    }
}

/// Middleware for read-only frontend routes (RO or RW token accepted).
pub async fn ro_auth(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if request.method() == axum::http::Method::OPTIONS {
        return next.run(request).await;
    }
    match authorize(&state, request.headers(), Access::Ro).await {
        Ok(()) => next.run(request).await,
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

        let request = axum::http::Request::builder().body(()).expect("request");
        let (parts, ()) = request.into_parts();
        let context = RequestContext::from_parts(&parts).expect("context");
        assert!(context.session_id.is_none());
        assert_eq!(context.actor, "agent");
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
