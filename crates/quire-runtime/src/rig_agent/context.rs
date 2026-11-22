//! Serializable per-run ToolContext values and host-only tool result metadata.

use quire_session_protocol::QuestionItem;
use rig::ContextValue;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, ContextValue)]
#[context(key = "quire.turn_meta")]
pub struct QuireTurnMeta {
    pub session_id: String,
    pub run_id: String,
    pub agent_id: String,
    pub kb_branch: Option<String>,
    pub doc_id: Option<String>,
    pub supports_vision: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ContextValue)]
#[context(key = "quire.created_report")]
pub struct CreatedReportMeta {
    pub doc_id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ContextValue)]
#[context(key = "quire.ask_user_requested")]
pub struct AskUserRequestedMeta {
    pub questions: Vec<QuestionItem>,
}
