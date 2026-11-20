//! Runtime API errors.

use axum::{Json, http::StatusCode, response::IntoResponse};
use serde_json::json;

use crate::kb::KbConflictReport;

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("redis error: {0}")]
    Redis(#[from] redis::RedisError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("session `{0}` was not found")]
    SessionNotFound(String),
    #[error("run `{0}` was not found")]
    RunNotFound(String),
    #[error("tool call `{0}` was not found")]
    ToolCallNotFound(String),
    #[error("attachment `{0}` was not found for this session")]
    AttachmentNotFound(String),
    #[error("chart preview `{0}` was not found or expired")]
    ChartPreviewNotFound(String),
    #[error("pending queue item `{0}` was not found")]
    PendingQueueItemNotFound(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("compaction conflict: {0}")]
    CompactionConflict(String),
    #[error("knowledge-base branch promotion conflict")]
    KbConflict(Box<KbConflictReport>),
    #[error("internal runtime error: {0}")]
    Internal(String),
}

impl RuntimeError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::SessionNotFound(_) => "SESSION_NOT_FOUND",
            Self::RunNotFound(_) => "RUN_NOT_FOUND",
            Self::ToolCallNotFound(_) => "TOOL_CALL_NOT_FOUND",
            Self::AttachmentNotFound(_) => "ATTACHMENT_NOT_FOUND",
            Self::ChartPreviewNotFound(_) => "CHART_PREVIEW_NOT_FOUND",
            Self::PendingQueueItemNotFound(_) => "QUEUE_ITEM_NOT_FOUND",
            Self::InvalidRequest(_) => "INVALID_REQUEST",
            Self::CompactionConflict(_) => "COMPACTION_CONFLICT",
            Self::KbConflict(_) => "KB_BRANCH_CONFLICT",
            Self::Database(_)
            | Self::Redis(_)
            | Self::Json(_)
            | Self::Http(_)
            | Self::Io(_)
            | Self::Internal(_) => "INTERNAL_ERROR",
        }
    }

    pub fn hint(&self) -> &'static str {
        match self {
            Self::SessionNotFound(_) => "Call GET /api/sessions to discover valid session IDs.",
            Self::RunNotFound(_) => "Refresh the session detail and retry with the active run ID.",
            Self::ToolCallNotFound(_) => {
                "Open the session detail and retry with a tool_call_id from this session."
            }
            Self::AttachmentNotFound(_) => {
                "Use an attachment_id returned by the current session's upload endpoint."
            }
            Self::ChartPreviewNotFound(_) => {
                "Call preview_echarts again to create a fresh preview; chart previews expire after 30 minutes."
            }
            Self::PendingQueueItemNotFound(_) => {
                "Refresh the session and retry with a queue item that is still pending."
            }
            Self::InvalidRequest(_) => {
                "Check the request body against the session protocol schema."
            }
            Self::CompactionConflict(_) => {
                "Wait for the active run to finish, then retry compaction."
            }
            Self::KbConflict(_) => {
                "Resolve the overlapping items in the draft branch with kb_upsert_* or delete them, then call kb_promote_branch again."
            }
            _ => "Retry the request; if the problem persists inspect the runtime logs.",
        }
    }
}

impl IntoResponse for RuntimeError {
    fn into_response(self) -> axum::response::Response {
        let status = match &self {
            Self::SessionNotFound(_)
            | Self::RunNotFound(_)
            | Self::ToolCallNotFound(_)
            | Self::AttachmentNotFound(_)
            | Self::ChartPreviewNotFound(_)
            | Self::PendingQueueItemNotFound(_) => StatusCode::NOT_FOUND,
            Self::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            Self::CompactionConflict(_) => StatusCode::CONFLICT,
            Self::KbConflict(_) => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let message = match &self {
            Self::SessionNotFound(_)
            | Self::RunNotFound(_)
            | Self::ToolCallNotFound(_)
            | Self::AttachmentNotFound(_)
            | Self::ChartPreviewNotFound(_)
            | Self::PendingQueueItemNotFound(_)
            | Self::InvalidRequest(_)
            | Self::CompactionConflict(_)
            | Self::KbConflict(_) => self.to_string(),
            _ => {
                tracing::error!(error = ?self, "runtime request failed internally");
                "internal runtime error; retry the request or inspect runtime logs".to_string()
            }
        };
        let details = match &self {
            Self::KbConflict(report) => serde_json::to_value(report.as_ref()).ok(),
            _ => None,
        };
        let body = match details {
            Some(details) => json!({
                "error": {
                    "code": self.code(),
                    "message": message,
                    "hint": self.hint(),
                    "details": details,
                }
            }),
            None => json!({
                "error": {
                    "code": self.code(),
                    "message": message,
                    "hint": self.hint(),
                }
            }),
        };
        (status, Json(body)).into_response()
    }
}

pub type RuntimeResult<T> = Result<T, RuntimeError>;
