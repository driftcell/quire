//! Concrete receipts returned by runtime built-in tools.

use quire_session_protocol::{TaskTodoItem, TaskTodoStatus};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, JsonSchema)]
pub struct SandboxExecuteReceipt {
    pub sandbox_id: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SandboxReadFileReceipt {
    pub sandbox_id: String,
    pub path: String,
    pub encoding: String,
    pub content: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SandboxWriteFileReceipt {
    pub sandbox_id: String,
    pub path: String,
    pub bytes_written: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SandboxMountAttachmentReceipt {
    pub sandbox_id: String,
    pub attachment_id: String,
    pub path: String,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReadSkillResourceReceipt {
    pub skill: String,
    pub path: String,
    pub content: String,
}

/// One `update_todos` checklist argument. Unlike the wire-level [`TaskTodoItem`], missing `status`
/// defaults to `pending` so the model may omit it when adding a new item.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TaskTodoInput {
    /// Optional opaque identifier used only to preserve item identity across updates. It is not
    /// user-facing. Omit it for ordinary new items and never copy it into `title` or `detail`.
    #[serde(default)]
    pub id: Option<String>,
    /// Required human-facing stage name in concise natural Chinese (usually 8–16 characters, as a
    /// verb-object phrase). Good: `核验高校立项口径` or `撰写第三章`. Bad: `更新 sec_2`,
    /// `update_todos step_1`, or `完成 run_8`. Never include tool names or raw IDs.
    pub title: String,
    /// Workflow state only (`pending` by default). Do not repeat this enum or other status codes in
    /// the human-facing `title` or `detail`.
    #[serde(default)]
    pub status: TaskTodoStatus,
    /// Optional human-facing one-sentence scope, evidence target, or outcome in natural Chinese.
    /// Good: `统一2021—2025年项目类型与时间窗`. Bad: `调用工具处理 rev_3`. Never include tool
    /// names, block/run/session IDs, revision numbers, JSON keys, or raw parameter values.
    #[serde(default)]
    pub detail: Option<String>,
    /// Optional internal heading block ID used only to locate the related report section in the UI.
    /// This is not user-facing text: omit it when no section link is needed and never repeat it in
    /// `title` or `detail`.
    #[serde(default)]
    pub section_id: Option<String>,
}

impl From<TaskTodoInput> for TaskTodoItem {
    fn from(input: TaskTodoInput) -> Self {
        Self {
            id: input.id,
            title: input.title,
            status: input.status,
            detail: input.detail,
            section_id: input.section_id,
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DescribeTaskReceipt {
    pub title: String,
    pub detail: Option<String>,
    pub section_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_todo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub todo_progress: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub _next_hint: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct UpdateTodosReceipt {
    pub todos: Vec<quire_session_protocol::TaskTodoItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_focus: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub _next_hint: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SetSessionTitleReceipt {
    pub session_id: String,
    pub title: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AskUserReceipt {
    pub status: &'static str,
    pub question_count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SubagentSummaryView {
    pub name: String,
    pub description: String,
    pub model: String,
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ListSubagentsReceipt {
    pub subagents: Vec<SubagentSummaryView>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SkillSummaryView {
    pub name: String,
    pub description: String,
    pub resources: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ListSkillsReceipt {
    pub skills: Vec<SkillSummaryView>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct AttachmentSummaryView {
    pub attachment_id: String,
    pub filename: String,
    pub mime_type: String,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ListAttachmentsReceipt {
    pub attachments: Vec<AttachmentSummaryView>,
}
