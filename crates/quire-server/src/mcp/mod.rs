//! Quire MCP tool server (19 domain tools) mounted at `/mcp` and callable in-process.
//!
//! The implementation is split into `schema` (request/response contracts and compact receipts)
//! and `handlers` (tool methods plus rebase helpers).

use std::{collections::HashMap, sync::OnceLock};

use rmcp::{
    ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{Implementation, ServerCapabilities, ServerConfig},
    tool_handler,
};
use serde::Serialize;
use serde_json::Value;

use crate::state::AppState;

mod handlers;
mod lint_hint;
mod schema;

pub use schema::*;

/// Canonical names of the 19 Quire report domain tools.
pub const REPORT_TOOL_NAMES: &[&str] = &[
    "get_protocol_guide",
    "create_report",
    "list_reports",
    "get_document_digest",
    "read_as_markdown",
    "read_blocks",
    "apply_operations",
    "rollback_revision",
    "upsert_sources",
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
    state: AppState,
    tool_router: ToolRouter<Self>,
}

#[derive(Serialize)]
struct ObjectSchema {
    r#type: &'static str,
}

impl QuireMcpServer {
    /// Construct the MCP server and register all 19 domain tools on the router.
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }

    /// Return the static description and JSON Schema for one of the 19 report tools.
    pub fn tool_metadata(name: &str) -> (String, Value) {
        static CACHE: OnceLock<HashMap<String, (String, Value)>> = OnceLock::new();
        let catalog = CACHE.get_or_init(|| {
            Self::tool_router()
                .list_all()
                .into_iter()
                .map(|tool| {
                    let description = tool
                        .description
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    let schema = tool.schema_as_json_value();
                    (tool.name.to_string(), (description, schema))
                })
                .collect()
        });
        catalog.get(name).cloned().unwrap_or_else(|| {
            let schema =
                serde_json::to_value(ObjectSchema { r#type: "object" }).unwrap_or_else(|error| {
                    tracing::error!(%error, "failed to serialize fallback tool schema");
                    Value::Object(Default::default())
                });
            (String::new(), schema)
        })
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
