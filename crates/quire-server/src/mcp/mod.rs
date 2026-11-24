//! Optional rmcp transport for the shared report application service.
//!
//! Agent-runtime report tools do not use this adapter; they call
//! `quire_server::report::QuireReportService` directly through native Rig tools.

use std::ops::Deref;

use rmcp::{
    ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{Implementation, ServerCapabilities, ServerConfig},
    tool_handler,
};

use crate::{report::QuireReportService, state::AppState};

mod handlers;

pub use crate::report::{REPORT_TOOL_NAMES, schema};
pub use schema::*;

const SERVER_INSTRUCTIONS: &str = r#"QUIRE MULTI-AGENT REPORT ENGINE

Workflow state machine: INIT -> SCHEMA -> AUTHOR -> VERIFY.

INIT: send X-Session-Id headers or per-call _meta.quire.{session_id,run_id,agent_id}. Call get_protocol_guide("quickstart"), then create_report or list_reports.
SCHEMA: call get_document_digest or read_as_markdown before editing. Block IDs are short semantic strings, never UUIDv4. Tables use recursive ColumnNode trees; never compute spans yourself.
AUTHOR: mutate only via apply_operations(doc_id, expected_revision, summary, ops). Register citations with upsert_sources and inspect existing records/evidence with list_sources. Use disjoint block-ID prefixes when writing in parallel; Smart CAS rebases disjoint edits and returns detailed conflicts for overlapping edits.
REVIEW: list_comments returns side-channel threads with anchor_snapshot and anchor_detached. add_comment and reply_comment are revision-free; mark_addressed moves open to addressed. Only humans may archive or reopen via REST.
VERIFY: call lint_document until there are zero errors and review warnings. Diagnostics carry `code`, `rule`, `hint` and `suppressible`; ignore only suppressible rules that genuinely do not apply. Then read_as_markdown for a reviewer view. The external frontend is read-only and consumes GET /api/docs/{doc_id} plus SSE revision deltas.

Every tool receipt includes `_next_hint`. Every error includes a structured hint with valid IDs/enums where possible. Never send HTML or Tiptap rendering payloads; the external frontend owns rendering."#;

#[derive(Clone)]
pub struct QuireMcpServer {
    service: QuireReportService,
    tool_router: ToolRouter<Self>,
}

impl QuireMcpServer {
    pub fn new(state: AppState) -> Self {
        Self {
            service: QuireReportService::new(state),
            tool_router: Self::tool_router(),
        }
    }
}

impl Deref for QuireMcpServer {
    type Target = QuireReportService;

    fn deref(&self) -> &Self::Target {
        &self.service
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for QuireMcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("quire", env!("CARGO_PKG_VERSION")))
            .with_instructions(SERVER_INSTRUCTIONS)
    }
}
