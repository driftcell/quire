//! HTTP error mapping with structured, self-healing recovery hints.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use quire_protocol::{ApiErrorEnvelope, ApiErrorInfo, BlockId, ProtocolError};
use quire_store::{RevisionConflict, StoreError};
use serde::Serialize;
use serde_json::Value;
use tracing::error as log_error;

#[derive(Debug)]
pub struct ApiError(pub StoreError);

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum StoreErrorDetails<'a> {
    Conflict(&'a RevisionConflict),
    Patch { patch_error: String },
    Protocol { protocol_error: &'a ProtocolError },
}

/// Structured details accepted by the MCP `invalid_params` boundary.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum McpErrorDetails {
    MissingBlockIds {
        missing: Vec<String>,
        valid_block_ids: Vec<String>,
    },
    MissingSourceIds {
        missing: Vec<String>,
        valid_source_ids: Vec<String>,
    },
    ValidVariants {
        valid_variants: &'static [&'static str],
    },
    DuplicateBlockIds {
        duplicate_block_ids: Vec<BlockId>,
    },
    ValidMaxDepths {
        valid_max_depths: [u8; 3],
    },
    ValidExamples {
        valid_examples: &'static [&'static str],
    },
    CurrentRevision {
        current_revision: i64,
    },
    StoredTargetProseChars {
        stored_target_prose_chars: u32,
    },
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
        let details = store_error_details(&self.0);
        if is_internal(&self.0) {
            log_error!(error = ?self.0, "request failed with an internal storage error");
        }
        let body = ApiErrorEnvelope::new(
            self.0.code(),
            public_message(&self.0),
            self.0.hint(),
            details,
        );
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
        StoreError::AlreadyExists { .. }
        | StoreError::RevisionAhead { .. }
        | StoreError::Conflict(_)
        | StoreError::SourceConflict { .. } => StatusCode::CONFLICT,
        StoreError::InvalidDocId { .. }
        | StoreError::InvalidSourceId { .. }
        | StoreError::InvalidRollbackTarget { .. }
        | StoreError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
        StoreError::NotFound { .. }
        | StoreError::CommentThreadNotFound { .. }
        | StoreError::CommentTargetNotFound { .. } => StatusCode::NOT_FOUND,
        StoreError::Patch(_) | StoreError::Protocol(_) => StatusCode::UNPROCESSABLE_ENTITY,
        StoreError::CorruptRevision { .. } | StoreError::RankExhausted(_) => StatusCode::CONFLICT,
        StoreError::Database(_)
        | StoreError::Redis(_)
        | StoreError::Json(_)
        | StoreError::Snapshot(_)
        | StoreError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn store_error_details(error: &StoreError) -> Option<StoreErrorDetails<'_>> {
    match error {
        StoreError::Conflict(conflict) => Some(StoreErrorDetails::Conflict(conflict)),
        StoreError::Patch(error) => Some(StoreErrorDetails::Patch {
            patch_error: error.to_string(),
        }),
        StoreError::Protocol(error) => Some(StoreErrorDetails::Protocol {
            protocol_error: error,
        }),
        _ => None,
    }
}

fn serialized_data<T: Serialize>(data: T) -> Option<Value> {
    match serde_json::to_value(data) {
        Ok(value) => Some(value),
        Err(error) => {
            log_error!(%error, "failed to serialize structured API error data");
            None
        }
    }
}

/// Convert a store/protocol failure to an MCP error with actionable JSON data.
pub fn mcp_error(error: StoreError) -> rmcp::ErrorData {
    let code = error.code();
    if is_internal(&error) {
        log_error!(error = ?error, "MCP call failed with an internal storage error");
    }
    let message = public_message(&error);
    let hint = error.hint();
    let details = store_error_details(&error);
    let data = serialized_data(ApiErrorInfo::new(code, &message, hint, details));
    match error {
        StoreError::Database(_)
        | StoreError::Redis(_)
        | StoreError::Json(_)
        | StoreError::Snapshot(_)
        | StoreError::Internal(_) => rmcp::ErrorData::internal_error(message, data),
        _ => rmcp::ErrorData::invalid_params(message, data),
    }
}

pub fn invalid_params(
    message: impl Into<String>,
    hint: impl Into<String>,
    details: Option<McpErrorDetails>,
) -> rmcp::ErrorData {
    let message = message.into();
    let data = serialized_data(ApiErrorInfo::new("INVALID_PARAMS", &message, hint, details));
    rmcp::ErrorData::invalid_params(message, data)
}

pub fn internal_error(message: impl Into<String>) -> rmcp::ErrorData {
    let message = message.into();
    let data = serialized_data(ApiErrorInfo::<()>::new(
        "INTERNAL_ERROR",
        &message,
        "Retry the request; if the problem persists inspect the server logs.",
        None,
    ));
    rmcp::ErrorData::internal_error(message, data)
}

#[derive(Serialize)]
pub struct ProtocolErrorData<'a> {
    pub code: &'a str,
    pub message: String,
    pub hint: String,
    pub protocol_error: &'a ProtocolError,
}

pub fn protocol_error(error: ProtocolError) -> rmcp::ErrorData {
    let code = error.code();
    let hint = error.hint();
    let message = error.to_string();
    let data = serialized_data(ProtocolErrorData {
        code,
        message: message.clone(),
        hint,
        protocol_error: &error,
    });
    rmcp::ErrorData::invalid_params(message, data)
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
