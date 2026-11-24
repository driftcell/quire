//! Concrete receipts returned by runtime built-in tools.

use quire_protocol::{Diagnostic, LintSummary};
use schemars::JsonSchema;
use serde::Serialize;

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
pub struct PreviewEchartsReceipt {
    pub preview_id: String,
    pub preview_url: String,
    pub lint_issues: Vec<Diagnostic>,
    pub lint_summary: LintSummary,
    pub image_included: bool,
    pub preview_error: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CommitChartReceipt {
    #[serde(flatten)]
    pub mutation: quire_server::report::ApplyOperationsReceipt,
    pub preview_id: String,
    pub block_id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ReadSkillResourceReceipt {
    pub skill: String,
    pub path: String,
    pub content: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DescribeTaskReceipt {
    pub title: String,
    pub detail: Option<String>,
    pub section_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub todos: Vec<quire_session_protocol::TaskTodoItem>,
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
