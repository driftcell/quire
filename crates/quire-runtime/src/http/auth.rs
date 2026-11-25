use crate::auth::{AuthUser, UserRole, verify_token};

use super::*;

fn extract_bearer_token(request: &Request) -> Option<&str> {
    request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            request
                .headers()
                .get("x-quire-token")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
}

fn unauthorized_response(message: &str) -> Response {
    let mut response = (
        StatusCode::UNAUTHORIZED,
        Json(ApiErrorEnvelope::<()>::new(
            "unauthorized",
            message,
            "Sign in via /api/auth/login or provide a valid API key (`Authorization: Bearer qk_...`).",
            None,
        )),
    )
        .into_response();
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        axum::http::HeaderValue::from_static("Bearer"),
    );
    response
}

fn forbidden_response(message: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(ApiErrorEnvelope::<()>::new(
            "forbidden",
            message,
            "This endpoint requires an administrator account.",
            None,
        )),
    )
        .into_response()
}

/// Authenticate SaaS user/admin requests via HS256 JWT or `qk_...` API key.
pub(crate) async fn runtime_auth(
    State(service): State<RuntimeService>,
    mut request: Request,
    next: Next,
) -> Response {
    if request.method() == axum::http::Method::OPTIONS {
        return next.run(request).await;
    }
    let Some(token) = extract_bearer_token(&request) else {
        return unauthorized_response("missing bearer token");
    };
    if token.starts_with("qk_") {
        match service.store().verify_api_key(token).await {
            Ok(Some(auth_user)) => {
                request.extensions_mut().insert(auth_user);
                return next.run(request).await;
            }
            _ => return unauthorized_response("invalid or revoked API key"),
        }
    }
    let Ok(claims) = verify_token(service.config(), token) else {
        return unauthorized_response("invalid or expired JWT");
    };
    let Some(role) = UserRole::parse(&claims.role) else {
        return unauthorized_response("JWT role is not permitted for API access");
    };
    request.extensions_mut().insert(AuthUser {
        user_id: claims.sub,
        username: claims.username,
        role,
    });
    next.run(request).await
}

/// Enforce administrator role after `runtime_auth`.
pub(crate) async fn admin_auth(request: Request, next: Next) -> Response {
    if request.method() == axum::http::Method::OPTIONS {
        return next.run(request).await;
    }
    let Some(user) = request.extensions().get::<AuthUser>() else {
        return unauthorized_response("missing authenticated user context");
    };
    if user.role != UserRole::Admin {
        return forbidden_response("administrator privileges required");
    }
    next.run(request).await
}

/// Read-only auth for the stateless chart preview route. Accepts any valid user JWT
/// as well as short-lived internal `preview` JWTs minted by `preview/mod.rs`.
pub(crate) async fn chart_preview_auth(
    State(service): State<RuntimeService>,
    request: Request,
    next: Next,
) -> Response {
    if request.method() == axum::http::Method::OPTIONS {
        return next.run(request).await;
    }
    let Some(token) = extract_bearer_token(&request) else {
        return unauthorized_response("missing chart preview JWT");
    };
    if verify_token(service.config(), token).is_err() {
        return unauthorized_response("invalid or expired chart preview JWT");
    }
    next.run(request).await
}

#[derive(Serialize)]
pub(crate) struct RuntimeHealthOk {
    status: &'static str,
    service: &'static str,
}

#[derive(Serialize)]
pub(crate) struct RuntimeHealthDegraded {
    status: &'static str,
    error: String,
}

pub(crate) async fn health(State(service): State<RuntimeService>) -> Response {
    match service.store().health().await {
        Ok(()) => (
            StatusCode::OK,
            Json(RuntimeHealthOk {
                status: "ok",
                service: "quire-runtime",
            }),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(RuntimeHealthDegraded {
                status: "degraded",
                error: error.to_string(),
            }),
        )
            .into_response(),
    }
}
