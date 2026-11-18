//! REST and Session-SSE wire contracts for the Quire agent runtime.
//!
//! This crate is pure synchronous domain code: no tokio, no network, no filesystem.

#![forbid(unsafe_code)]

pub mod events;
pub mod session;

pub use events::{SessionEvent, SessionEventKind};
pub use session::{
    ActiveRunStatus, AttachmentMeta, ContextUsage, CreateSessionRequest, MessageRole,
    PendingQuestion, QuestionAnswer, QuestionItem, QuestionOption, SendMessageRequest,
    SessionDetail, SessionMessage, SessionStatus, SessionSummary, ToolCallDetail,
};
