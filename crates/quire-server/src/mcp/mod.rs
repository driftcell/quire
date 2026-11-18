//! Quire MCP tool server (15 domain tools) mounted at `/mcp`.
//!
//! The implementation is split into `schema` (request/response contracts and compact receipts)
//! and `handlers` (tool methods plus rebase/draft helpers).

use rmcp::{
    ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{Implementation, ServerCapabilities, ServerConfig},
    tool_handler,
};

use crate::state::AppState;

mod handlers;
mod schema;

pub use schema::*;

const SERVER_INSTRUCTIONS: &str = r#"QUIRE MULTI-AGENT REPORT ENGINE

Workflow state machine: INIT -> SCHEMA -> AUTHOR -> VERIFY.

INIT: send X-Session-Id on MCP HTTP requests. Call get_protocol_guide("quickstart"), then create_report or list_reports.
SCHEMA: call get_document_digest or read_as_markdown before editing. Block IDs are short semantic strings, never UUIDv4. Tables use recursive ColumnNode trees; never compute spans yourself.
AUTHOR: mutate only via apply_operations(doc_id, expected_revision, summary, ops) or apply_draft. Register citations with upsert_sources. Use disjoint block-ID prefixes when writing in parallel; Smart CAS rebases disjoint edits and returns detailed conflicts for overlapping edits.
VERIFY: call lint_document until there are zero errors, then read_as_markdown for a reviewer view. The external frontend is read-only and consumes GET /api/docs/{doc_id} plus SSE revision deltas.

Every tool receipt includes `_next_hint`. Every error includes a structured hint with valid IDs/enums where possible. Never send HTML or Tiptap rendering payloads; the external frontend owns rendering."#;

#[derive(Clone)]
pub struct QuireMcpServer {
    state: AppState,
    tool_router: ToolRouter<Self>,
}

impl QuireMcpServer {
    /// Construct the MCP server and register all 15 domain tools on the router.
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
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
