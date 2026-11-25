//! Shared report-tool contracts and transport-independent domain execution.
//!
//! Agent-runtime Rig tools and any retained transport adapters call the same
//! [`QuireReportService`] instead of owning separate report behavior.

use crate::state::AppState;

mod lint_hint;
pub mod render;
pub mod schema;
mod service;

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

/// Shared in-process report application service used by native Rig tools and transport adapters.
#[derive(Clone)]
pub struct QuireReportService {
    pub(crate) state: AppState,
}

impl QuireReportService {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }
}
