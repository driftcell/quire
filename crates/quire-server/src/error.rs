//! HTTP error mapping with structured, self-healing recovery hints.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use quire_protocol::ProtocolError;
use quire_store::{RevisionConflict, StoreError};
use serde::Serialize;
use serde_json::{Value, json};
use tracing::error as log_error;

#[derive(Debug)]
pub struct ApiError(pub StoreError);

#[derive(Debug, Serialize)]
struct ErrorEnvelope {
    error: ErrorInfo,
}

#[derive(Debug, Serialize)]
struct ErrorInfo {
    code: String,
    message: String,
    hint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Value>,
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        Self(error)
    }
}

impl From<ProtocolError> for ApiError {
    fn from(error: ProtocolError) -> Self {
        Self(StoreError::Protocol(error))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = status_for(&self.0);
        let details = match &self.0 {
            StoreError::Conflict(conflict) => Some(conflict_details(conflict)),
            StoreError::Patch(error) => Some(json!({
                "patch_error": error.to_string(),
            })),
            StoreError::Protocol(error) => Some(json!({"protocol_error": error})),
            _ => None,
        };
        if is_internal(&self.0) {
            log_error!(error = ?self.0, "request failed with an internal storage error");
        }
        let body = ErrorEnvelope {
            error: ErrorInfo {
                code: self.0.code().to_string(),
                message: public_message(&self.0),
                hint: self.0.hint(),
                details,
            },
        };
        (status, Json(body)).into_response()
    }
}

/// Raw database/Redis/serialization errors stay in logs; clients receive a stable message.
fn is_internal(error: &StoreError) -> bool {
    matches!(
        error,
        StoreError::Database(_)
            | StoreError::Redis(_)
            | StoreError::Json(_)
            | StoreError::Snapshot(_)
            | StoreError::Internal(_)
    )
}

fn public_message(error: &StoreError) -> String {
    if is_internal(error) {
        "internal storage error; retry the request or contact the operator".to_string()
    } else {
        error.to_string()
    }
}

fn status_for(error: &StoreError) -> StatusCode {
    match error {
        StoreError::NotFound { .. } => StatusCode::NOT_FOUND,
        StoreError::AlreadyExists { .. }
        | StoreError::RevisionAhead { .. }
        | StoreError::Conflict(_)
        | StoreError::SourceConflict { .. } => StatusCode::CONFLICT,
        StoreError::InvalidDocId { .. }
        | StoreError::InvalidSourceId { .. }
        | StoreError::InvalidRollbackTarget { .. }
        | StoreError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
        StoreError::Patch(_) | StoreError::Protocol(_) => StatusCode::UNPROCESSABLE_ENTITY,
        StoreError::CorruptRevision { .. } | StoreError::RankExhausted(_) => StatusCode::CONFLICT,
        StoreError::Database(_)
        | StoreError::Redis(_)
        | StoreError::Json(_)
        | StoreError::Snapshot(_)
        | StoreError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn conflict_details(conflict: &RevisionConflict) -> Value {
    json!({
        "doc_id": conflict.doc_id,
        "expected_revision": conflict.expected_revision,
        "current_revision": conflict.current_revision,
        "conflicting_block_ids": conflict.conflicting_block_ids,
        "conflicting_source_ids": conflict.conflicting_source_ids,
        "conflicting_revisions": conflict.conflicting_revisions,
        "latest_blocks": conflict.latest_blocks,
    })
}

/// Convert a store/protocol failure to an MCP error with actionable JSON data.
pub fn mcp_error(error: StoreError) -> rmcp::ErrorData {
    let code = error.code();
    if is_internal(&error) {
        log_error!(error = ?error, "MCP call failed with an internal storage error");
    }
    let message = public_message(&error);
    let hint = error.hint();
    let details = match &error {
        StoreError::Conflict(conflict) => Some(conflict_details(conflict)),
        StoreError::Patch(patch) => Some(json!({"patch_error": patch.to_string()})),
        StoreError::Protocol(protocol) => Some(json!({"protocol_error": protocol})),
        _ => None,
    };
    let data = json!({
        "code": code,
        "message": &message,
        "hint": hint,
        "details": details,
    });
    match error {
        StoreError::Database(_)
        | StoreError::Redis(_)
        | StoreError::Json(_)
        | StoreError::Snapshot(_)
        | StoreError::Internal(_) => rmcp::ErrorData::internal_error(message, Some(data)),
        _ => rmcp::ErrorData::invalid_params(message, Some(data)),
    }
}

pub fn invalid_params(
    message: impl Into<String>,
    hint: impl Into<String>,
    details: Option<Value>,
) -> rmcp::ErrorData {
    let message = message.into();
    rmcp::ErrorData::invalid_params(
        message.clone(),
        Some(json!({
            "code": "INVALID_PARAMS",
            "message": message,
            "hint": hint.into(),
            "details": details,
        })),
    )
}

pub fn internal_error(message: impl Into<String>) -> rmcp::ErrorData {
    let message = message.into();
    rmcp::ErrorData::internal_error(
        message.clone(),
        Some(json!({
            "code": "INTERNAL_ERROR",
            "message": message,
            "hint": "Retry the request; if the problem persists inspect the server logs.",
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_params_uses_unified_envelope() {
        let error = invalid_params("bad field", "fix it", None);
        let data = error.data.expect("structured data");
        assert_eq!(data["code"], "INVALID_PARAMS");
        assert_eq!(data["message"], "bad field");
        assert_eq!(data["hint"], "fix it");
    }

    #[test]
    fn mcp_error_uses_unified_envelope() {
        let error = mcp_error(StoreError::InvalidRequest("bad request".into()));
        let data = error.data.expect("structured data");
        assert_eq!(data["code"], "INVALID_REQUEST");
        assert_eq!(data["message"], "invalid request: bad request");
        assert!(data["hint"].as_str().is_some_and(|hint| !hint.is_empty()));
    }
}
