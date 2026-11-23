use super::*;

pub(crate) async fn runtime_auth(
    State(service): State<RuntimeService>,
    request: Request,
    next: Next,
) -> Response {
    if request.method() == axum::http::Method::OPTIONS {
        return next.run(request).await;
    }
    let expected = service.config().api_token.expose_secret();
    if expected.is_empty() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiErrorEnvelope::<()>::new(
                "auth_not_configured",
                "runtime API token is not configured",
                "Set RUNTIME_API_TOKEN before exposing quire-runtime.",
                None,
            )),
        )
            .into_response();
    }
    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let matches = presented.is_some_and(|value| {
        value.len() == expected.len() && bool::from(value.as_bytes().ct_eq(expected.as_bytes()))
    });
    if !matches {
        let mut response = (
            StatusCode::UNAUTHORIZED,
            Json(ApiErrorEnvelope::<()>::new(
                "unauthorized",
                "missing or invalid runtime bearer token",
                "Send `Authorization: Bearer $RUNTIME_API_TOKEN`.",
                None,
            )),
        )
            .into_response();
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            axum::http::HeaderValue::from_static("Bearer"),
        );
        return response;
    }
    next.run(request).await
}

/// Read-only auth for the stateless chart preview route. Accepts the same `ro_token`/`rw_token`
/// pair as `GET /api/docs/{doc_id}` (plus the runtime API token for local development).
pub(crate) async fn chart_preview_auth(
    State(service): State<RuntimeService>,
    request: Request,
    next: Next,
) -> Response {
    if request.method() == axum::http::Method::OPTIONS {
        return next.run(request).await;
    }
    let config = service.config();
    let candidates = [
        &config.quire_ro_token,
        &config.quire_rw_token,
        &config.api_token,
    ];
    if candidates
        .iter()
        .all(|token| token.expose_secret().trim().is_empty())
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiErrorEnvelope::<()>::new(
                "auth_not_configured",
                "chart preview auth is not configured",
                "Set QUIRE_RO_TOKEN, QUIRE_RW_TOKEN or RUNTIME_API_TOKEN.",
                None,
            )),
        )
            .into_response();
    }
    let presented = request
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
        });
    let matches = presented.is_some_and(|presented| {
        candidates.iter().any(|token| {
            let expected = token.expose_secret().trim();
            !expected.is_empty()
                && expected.len() == presented.len()
                && bool::from(presented.as_bytes().ct_eq(expected.as_bytes()))
        })
    });
    if !matches {
        let mut response = (
            StatusCode::UNAUTHORIZED,
            Json(ApiErrorEnvelope::<()>::new(
                "unauthorized",
                "missing or invalid chart preview token",
                "Send `Authorization: Bearer $QUIRE_RO_TOKEN`.",
                None,
            )),
        )
            .into_response();
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            axum::http::HeaderValue::from_static("Bearer"),
        );
        return response;
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
