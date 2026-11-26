//! Built-in tool definitions, schemas and credential/URL redaction helpers.

use std::{collections::BTreeSet, sync::Arc};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use quire_protocol::{Block, BlockId, PatchOp};
use quire_session_protocol::QuestionItem;
use rig::tool::server::ToolServer;
use rig::{
    message::{ImageMediaType, MimeType as _, ToolResultContent},
    rig_tool,
    tool::{ToolContext, ToolExecutionError, ToolOutput},
};
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use tracing::warn;

use crate::{
    mcp_registry::tool_pattern_matches,
    model_stream::ModelStreamEvent,
    sandbox::{SandboxExecRequest, SandboxLease},
    service::{SANDBOX_ATTACHMENT_GET_TTL_SECS, safe_attachment_filename, safe_attachment_id},
    store::SessionPatch,
};

use super::{
    AgentExecution, RigModelCore, SharedDocId, StreamEventSender,
    context::{AskUserRequestedMeta, QuireTurnMeta},
    subagents::RunSubagentsTool,
    tool_schemas::{
        AskUserReceipt, AttachmentSummaryView, DescribeTaskReceipt, ListAttachmentsReceipt,
        ListSkillsReceipt, ListSubagentsReceipt, ReadSkillResourceReceipt, SandboxExecuteReceipt,
        SandboxMountAttachmentReceipt, SandboxReadFileReceipt, SandboxWriteFileReceipt,
        SetSessionTitleReceipt, SkillSummaryView, SubagentSummaryView, TaskTodoInput,
        UpdateTodosReceipt,
    },
};

fn image_media_type(mime_type: &str) -> ImageMediaType {
    ImageMediaType::from_mime_type(mime_type).unwrap_or(ImageMediaType::PNG)
}

pub const BUILTIN_TOOLS: &[&str] = &[
    "list_skills",
    "load_skill",
    "read_skill_resource",
    "describe_task",
    "update_todos",
    "set_session_title",
    "list_subagents",
    "spawn_subagents",
    "ask_user",
    "preview_report",
    "preview_echarts",
    "commit_chart",
    "sandbox_execute",
    "sandbox_read_file",
    "sandbox_write_file",
    "list_attachments",
    "sandbox_mount_attachment",
    "get_protocol_guide",
    "create_report",
    "list_reports",
    "get_document_digest",
    "read_as_markdown",
    "read_blocks",
    "apply_operations",
    "rollback_revision",
    "upsert_sources",
    "delete_sources",
    "list_sources",
    "set_cover",
    "configure_toc",
    "configure_references",
    "configure_page_chrome",
    "update_style_and_css",
    "lint_document",
    "list_comments",
    "add_comment",
    "reply_comment",
];

/// Return configured patterns that match no assembled tool.
///
/// Wildcards are expanded against the final tool map rather than looked up as
/// literal names. A scoped pattern such as `server_a:subjectcloud_*` is checked
/// against the tool-name portion because assembled MCP tools keep bare names.
fn missing_allowed_tool_patterns(
    allowed_tools: &[String],
    available_tools: &BTreeSet<String>,
) -> Vec<String> {
    let mut missing = Vec::new();
    for pattern in allowed_tools {
        let trimmed = pattern.trim();
        if trimmed.is_empty() {
            continue;
        }
        let effective = trimmed
            .split_once(':')
            .map(|(_, tool_name)| tool_name)
            .unwrap_or(trimmed)
            .trim();
        if effective.is_empty() || effective == "*" {
            continue;
        }
        let available = if effective.contains('*') {
            available_tools
                .iter()
                .any(|name| tool_pattern_matches(effective, name))
        } else {
            available_tools.contains(effective)
        };
        if !available {
            missing.push(trimmed.to_string());
        }
    }
    missing
}

pub(super) fn warn_missing_allowed_tools(
    agent_id: &str,
    allowed_tools: &[String],
    available_tools: &BTreeSet<String>,
) -> Vec<String> {
    let missing = missing_allowed_tool_patterns(allowed_tools, available_tools);
    for pattern in &missing {
        warn!(
            agent_id = %agent_id,
            allowed_tool = %pattern,
            "configured allowed_tool does not exist or is unavailable; skipping without blocking workflow"
        );
    }
    missing
}

fn allow_list_matches(allowed_tools: &[String], tool_name: &str) -> bool {
    let is_report_tool = quire_server::report::REPORT_TOOL_NAMES.contains(&tool_name);
    allowed_tools.iter().any(|pattern| {
        let trimmed = pattern.trim();
        tool_pattern_matches(trimmed, tool_name)
            || (is_report_tool
                && trimmed
                    .strip_prefix("quire-report:")
                    .is_some_and(|scoped| tool_pattern_matches(scoped.trim(), tool_name)))
    })
}

pub(super) struct ToolBuildContext {
    pub(super) execution: AgentExecution,
    pub(super) scope: StreamEventSender,
    pub(super) engine: Arc<RigModelCore>,
    pub(super) sandbox: Arc<SandboxLease>,
    pub(super) parent_history: super::SharedForkHistory,
}

pub(super) struct BuiltInTools {
    pub(super) server: ToolServer,
    pub(super) names: BTreeSet<String>,
    pub(super) runtime: Arc<RuntimeToolScope>,
}

pub(super) struct RuntimeToolScope {
    pub(super) engine: Arc<RigModelCore>,
    pub(super) report: quire_server::report::QuireReportService,
    pub(super) event_scope: StreamEventSender,
    sandbox: Arc<SandboxLease>,
    pub(super) cancellation: tokio_util::sync::CancellationToken,
    /// Latest bound report ID, shared across the turn so subagents spawned
    /// after a lazy `create_report` inherit the new document.
    pub(super) doc_id: SharedDocId,
    /// Snapshot of the parent agent's live conversation history for `fork: true` subagents.
    pub(super) parent_history: super::SharedForkHistory,
    /// Live work focus and todo checklist for this agent run so `describe_task`, `update_todos`,
    /// and `spawn_subagents` receipts share state without holding locks across `.await`.
    task_focus_title: std::sync::Mutex<Option<String>>,
    task_todos: std::sync::Mutex<Vec<quire_session_protocol::TaskTodoItem>>,
}

impl RuntimeToolScope {
    pub(super) fn set_task_focus_title(&self, title: &str) {
        let mut guard = self
            .task_focus_title
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = Some(title.to_string());
    }

    pub(super) fn task_focus_title_snapshot(&self) -> Option<String> {
        self.task_focus_title
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(super) fn replace_task_todos(
        &self,
        incoming: Vec<quire_session_protocol::TaskTodoItem>,
    ) -> Vec<quire_session_protocol::TaskTodoItem> {
        let mut guard = self
            .task_todos
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = incoming.clone();
        incoming
    }

    pub(super) fn task_todo_snapshot(&self) -> Vec<quire_session_protocol::TaskTodoItem> {
        self.task_todos
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

pub(super) fn runtime_tool_scope(
    context: &ToolContext,
) -> Result<Arc<RuntimeToolScope>, ToolExecutionError> {
    context
        .scope::<RuntimeToolScope>()
        .ok_or_else(|| ToolExecutionError::other("missing runtime tool context"))
}

pub(super) fn quire_turn_meta(context: &ToolContext) -> Result<QuireTurnMeta, ToolExecutionError> {
    context
        .require::<QuireTurnMeta>()
        .map_err(|error| ToolExecutionError::other(error.to_string()))
}

pub(super) fn tool_other(error: impl std::fmt::Display) -> ToolExecutionError {
    ToolExecutionError::other(error.to_string())
}

pub(super) fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

const MAX_TOOL_CODE_BYTES: usize = 256 * 1024;
const MAX_TOOL_FILE_BYTES: usize = 256 * 1024;
const MAX_TOOL_OUTPUT_BYTES: usize = 64 * 1024;

fn bounded_tool_text(text: String) -> (String, bool) {
    if text.len() <= MAX_TOOL_OUTPUT_BYTES {
        return (text, false);
    }
    let mut end = MAX_TOOL_OUTPUT_BYTES;
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    (text.get(..end).unwrap_or_default().to_string(), true)
}

pub(crate) use preview::*;
pub(crate) use report_inproc::*;
pub(crate) use sandbox::*;
pub(crate) use skills_tasks::*;
mod preview;
mod report_inproc;
mod sandbox;
mod skills_tasks;
#[cfg(test)]
mod tests;

impl RigModelCore {
    pub(super) fn build_tools(&self, context: ToolBuildContext) -> BuiltInTools {
        let ToolBuildContext {
            execution,
            scope,
            engine,
            sandbox,
            parent_history,
        } = context;
        let report = self.build_report_service();
        let runtime = Arc::new(RuntimeToolScope {
            engine,
            report,
            event_scope: scope,
            sandbox,
            cancellation: execution.cancellation.clone(),
            doc_id: Arc::clone(&execution.doc_id),
            parent_history,
            task_focus_title: std::sync::Mutex::new(None),
            task_todos: std::sync::Mutex::new(Vec::new()),
        });
        let mut server = ToolServer::new();
        let mut names = BTreeSet::new();
        macro_rules! register {
            ($name:literal, $enabled:expr, $tool:expr) => {
                if $enabled {
                    names.insert($name.to_string());
                    server = server.tool($tool);
                }
            };
        }

        register!(
            "preview_report",
            allow_list_matches(&execution.allowed_tools, "preview_report"),
            PreviewReportTool
        );
        register!(
            "preview_echarts",
            allow_list_matches(&execution.allowed_tools, "preview_echarts"),
            PreviewEchartsTool
        );
        register!(
            "commit_chart",
            allow_list_matches(&execution.allowed_tools, "commit_chart"),
            CommitChartTool
        );
        register!(
            "sandbox_execute",
            allow_list_matches(&execution.allowed_tools, "sandbox_execute"),
            SandboxExecuteTool
        );
        register!(
            "sandbox_read_file",
            allow_list_matches(&execution.allowed_tools, "sandbox_read_file"),
            SandboxReadFileTool
        );
        register!(
            "sandbox_write_file",
            allow_list_matches(&execution.allowed_tools, "sandbox_write_file"),
            SandboxWriteFileTool
        );
        register!(
            "list_attachments",
            allow_list_matches(&execution.allowed_tools, "list_attachments")
                || allow_list_matches(&execution.allowed_tools, "sandbox_mount_attachment"),
            ListAttachmentsTool
        );
        register!(
            "sandbox_mount_attachment",
            allow_list_matches(&execution.allowed_tools, "sandbox_mount_attachment"),
            SandboxMountAttachmentTool
        );
        register!(
            "list_skills",
            allow_list_matches(&execution.allowed_tools, "list_skills")
                || allow_list_matches(&execution.allowed_tools, "load_skill"),
            ListSkillsTool
        );
        register!(
            "load_skill",
            allow_list_matches(&execution.allowed_tools, "load_skill"),
            LoadSkillTool
        );
        register!(
            "read_skill_resource",
            allow_list_matches(&execution.allowed_tools, "read_skill_resource"),
            ReadSkillResourceTool
        );
        register!(
            "describe_task",
            allow_list_matches(&execution.allowed_tools, "describe_task"),
            DescribeTaskTool
        );
        register!(
            "update_todos",
            allow_list_matches(&execution.allowed_tools, "update_todos")
                || allow_list_matches(&execution.allowed_tools, "describe_task"),
            UpdateTodosTool
        );
        register!(
            "set_session_title",
            !execution.run_id.starts_with("run_sub_")
                && (allow_list_matches(&execution.allowed_tools, "set_session_title")
                    || allow_list_matches(&execution.allowed_tools, "describe_task")),
            SetSessionTitleTool
        );
        register!(
            "ask_user",
            !execution.run_id.starts_with("run_sub_")
                && allow_list_matches(&execution.allowed_tools, "ask_user"),
            AskUserTool
        );
        register!(
            "list_subagents",
            allow_list_matches(&execution.allowed_tools, "list_subagents")
                || (!execution.run_id.starts_with("run_sub_")
                    && allow_list_matches(&execution.allowed_tools, "spawn_subagents")),
            ListSubagentsTool
        );
        register!(
            "spawn_subagents",
            allow_list_matches(&execution.allowed_tools, "spawn_subagents"),
            RunSubagentsTool
        );
        register!(
            "get_protocol_guide",
            allow_list_matches(&execution.allowed_tools, "get_protocol_guide"),
            GetProtocolGuideTool
        );
        register!(
            "create_report",
            allow_list_matches(&execution.allowed_tools, "create_report"),
            CreateReportTool
        );
        register!(
            "list_reports",
            allow_list_matches(&execution.allowed_tools, "list_reports"),
            ListReportsTool
        );
        register!(
            "get_document_digest",
            allow_list_matches(&execution.allowed_tools, "get_document_digest"),
            GetDocumentDigestTool
        );
        register!(
            "read_as_markdown",
            allow_list_matches(&execution.allowed_tools, "read_as_markdown"),
            ReadAsMarkdownTool
        );
        register!(
            "read_blocks",
            allow_list_matches(&execution.allowed_tools, "read_blocks"),
            ReadBlocksTool
        );
        register!(
            "apply_operations",
            allow_list_matches(&execution.allowed_tools, "apply_operations"),
            ApplyOperationsTool
        );
        register!(
            "rollback_revision",
            allow_list_matches(&execution.allowed_tools, "rollback_revision"),
            RollbackRevisionTool
        );
        register!(
            "upsert_sources",
            allow_list_matches(&execution.allowed_tools, "upsert_sources"),
            UpsertSourcesTool
        );
        register!(
            "delete_sources",
            allow_list_matches(&execution.allowed_tools, "delete_sources"),
            DeleteSourcesTool
        );
        register!(
            "list_sources",
            allow_list_matches(&execution.allowed_tools, "list_sources"),
            ListSourcesTool
        );
        register!(
            "set_cover",
            allow_list_matches(&execution.allowed_tools, "set_cover"),
            SetCoverTool
        );
        register!(
            "configure_toc",
            allow_list_matches(&execution.allowed_tools, "configure_toc"),
            ConfigureTocTool
        );
        register!(
            "configure_references",
            allow_list_matches(&execution.allowed_tools, "configure_references"),
            ConfigureReferencesTool
        );
        register!(
            "configure_page_chrome",
            allow_list_matches(&execution.allowed_tools, "configure_page_chrome"),
            ConfigurePageChromeTool
        );
        register!(
            "update_style_and_css",
            allow_list_matches(&execution.allowed_tools, "update_style_and_css"),
            UpdateStyleAndCssTool
        );
        register!(
            "lint_document",
            allow_list_matches(&execution.allowed_tools, "lint_document"),
            LintDocumentTool
        );
        register!(
            "list_comments",
            allow_list_matches(&execution.allowed_tools, "list_comments"),
            ListCommentsTool
        );
        register!(
            "add_comment",
            allow_list_matches(&execution.allowed_tools, "add_comment"),
            AddCommentTool
        );
        register!(
            "reply_comment",
            allow_list_matches(&execution.allowed_tools, "reply_comment"),
            ReplyCommentTool
        );
        BuiltInTools {
            server,
            names,
            runtime,
        }
    }
}
