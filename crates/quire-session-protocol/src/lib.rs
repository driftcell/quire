//! REST and Session-SSE wire contracts for the Quire agent runtime.
//!
//! This crate is pure synchronous domain code: no tokio, no network, no filesystem.

#![forbid(unsafe_code)]

pub mod events;
pub mod session;
pub mod trace;

pub use events::{SessionEvent, SessionEventKind};
pub use session::{
    ActiveRunStatus, AskUserAnswerPayload, AttachmentMeta, ContextUsage, CreateSessionRequest,
    MessageRole, PendingQuestion, PendingQueueItem, PendingQueueSource, QuestionAnswer,
    QuestionItem, QuestionOption, QuestionnaireResolutionStatus, ResolvedQuestion,
    ResolvedQuestionItemAnswer, RunTaskFocus, RunTraceSegment, RunTraceSubagent, RunTraceSummary,
    RunTraceToolItem, SendMessageRequest, SessionCommentRef, SessionDetail, SessionMessage,
    SessionStatus, SessionSummary, TaskTodoItem, TaskTodoStatus, ToolCallDetail,
    UpdatePendingMessageRequest,
};
pub use trace::{aggregate_run_traces, merge_task_todos};
