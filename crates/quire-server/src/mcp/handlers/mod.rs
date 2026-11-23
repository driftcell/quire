//! MCP tool implementations, Smart CAS reload helpers and document projections.

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    num::NonZeroU32,
};

use axum::http::request::Parts;
use indexmap::IndexMap;
use quire_lint::{LintOptions, lint_document_with_options};
use quire_protocol::{
    Block, BlockId, BlockKind, COVER_VARIANTS, CommentAuthorType, CommentStatus, CoverConfig,
    Diagnostic, DocId, EphemeralClearEvent, EphemeralPayload, IdAssignment, MarkdownOptions,
    MarkdownProjector, PageChromeConfig, PatchOp, ProtocolError, ReportDocument, Revision,
    Severity, SourceId, TOC_VARIANTS, is_safe_css_token_value, is_valid_css_token_name,
    scan_document_citations, short_token,
};
use quire_store::{
    ApplyResult, CommitExtras, NewCommentReply, NewCommentThread, SourceUpsert, StoreError,
};
use rmcp::{
    ErrorData,
    handler::server::{
        router::tool::ToolRouter,
        wrapper::{Json, Parameters},
    },
    tool, tool_router,
};

use crate::{
    auth::RequestContext,
    error::{McpErrorDetails, invalid_params, mcp_error},
    guide::{guide, guide_workflow},
    state::AppState,
};

use super::QuireMcpServer;
use super::lint_hint;
use super::schema::*;

mod lifecycle;
mod mutations;
mod mutations_exec;
mod review;
mod sources_chrome;
mod support;

pub(crate) use support::*;

impl QuireMcpServer {
    pub(crate) fn tool_router() -> ToolRouter<Self> {
        let mut router = Self::lifecycle_router();
        router.merge(Self::mutations_router());
        router
    }
}
