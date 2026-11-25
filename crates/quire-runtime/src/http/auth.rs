use crate::auth::{AuthUser, UserRole, verify_token};

use super::*;

#[derive(Debug, Clone)]
pub(crate) enum ChartPreviewAccess {
    User(AuthUser),
    PreviewChart(String),
}

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

/// Authenticate SaaS user/admin requests via HS256 JWT or `qk_...` API key,
/// verifying against `identity.users` that the user account is still active (`enabled = TRUE`)
/// and resolving the user's current database role.
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
    if UserRole::parse(&claims.role).is_none() {
        return unauthorized_response("JWT role is not permitted for API access");
    };
    let Ok(Some(live_user)) = service.store().verify_active_user(&claims.sub).await else {
        return unauthorized_response("user account is disabled or no longer exists");
    };
    request.extensions_mut().insert(live_user);
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

/// Read-only auth for `/api/chart-previews/{preview_id}`. Accepts active user JWTs/API keys
/// as well as short-lived chart-scoped `preview` JWTs minted by `preview/mod.rs`.
pub(crate) async fn chart_preview_auth(
    State(service): State<RuntimeService>,
    mut request: Request,
    next: Next,
) -> Response {
    if request.method() == axum::http::Method::OPTIONS {
        return next.run(request).await;
    }
    let Some(token) = extract_bearer_token(&request) else {
        return unauthorized_response("missing chart preview JWT");
    };
    if token.starts_with("qk_") {
        match service.store().verify_api_key(token).await {
            Ok(Some(auth_user)) => {
                request
                    .extensions_mut()
                    .insert(ChartPreviewAccess::User(auth_user));
                return next.run(request).await;
            }
            _ => return unauthorized_response("invalid or revoked API key"),
        }
    }
    let Ok(claims) = verify_token(service.config(), token) else {
        return unauthorized_response("invalid or expired chart preview JWT");
    };
    if claims.role == "preview" {
        let Some(chart_id) = claims
            .preview_chart_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return forbidden_response("preview JWT is not scoped to a chart resource");
        };
        request
            .extensions_mut()
            .insert(ChartPreviewAccess::PreviewChart(chart_id.to_string()));
        return next.run(request).await;
    }
    let Ok(Some(live_user)) = service.store().verify_active_user(&claims.sub).await else {
        return unauthorized_response("user account is disabled or no longer exists");
    };
    request
        .extensions_mut()
        .insert(ChartPreviewAccess::User(live_user));
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
