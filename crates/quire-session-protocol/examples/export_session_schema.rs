//! Export the canonical session-protocol JSON Schema used by the frontend.
//!
//! ```sh
//! cargo run -p quire-session-protocol --example export_session_schema
//! ```

use std::{fs, path::Path};

use quire_session_protocol::{
    AttachmentMeta, CreateSessionRequest, PendingQuestion, QuestionAnswer, QuestionItem,
    QuestionOption, SendMessageRequest, SessionDetail, SessionEvent, SessionMessage,
    SessionSummary, ToolCallDetail,
};
use schemars::JsonSchema;

#[allow(dead_code)]
#[derive(Debug, JsonSchema)]
#[schemars(title = "Quire Session Protocol")]
struct SessionProtocolContract {
    session_event: SessionEvent,
    session_summary: SessionSummary,
    session_detail: SessionDetail,
    session_message: SessionMessage,
    attachment_meta: AttachmentMeta,
    create_session_request: CreateSessionRequest,
    send_message_request: SendMessageRequest,
    tool_call_detail: ToolCallDetail,
    question_option: QuestionOption,
    question_item: QuestionItem,
    question_answer: QuestionAnswer,
    pending_question: PendingQuestion,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let schema = schemars::schema_for!(SessionProtocolContract);
    let rendered = serde_json::to_string_pretty(&schema)?;
    let contract_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("contracts")
        .join("session-protocol.schema.json");
    if let Some(parent) = contract_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&contract_path, format!("{rendered}\n"))?;
    Ok(())
}
