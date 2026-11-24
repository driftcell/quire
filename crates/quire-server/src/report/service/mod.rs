//! Report domain execution, Smart CAS reload helpers and document projections.

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    num::NonZeroU32,
};

use indexmap::IndexMap;
use quire_lint::{LintOptions, lint_document_with_options};
use quire_protocol::{
    Block, BlockId, BlockKind, CommentAuthorType, CommentStatus, Diagnostic, DocId,
    EphemeralClearEvent, EphemeralPayload, IdAssignment, MarkdownOptions, MarkdownProjector,
    PageChromeConfig, PatchOp, ProtocolError, ReportDocument, Revision, Severity, SourceId,
    TOC_VARIANTS, is_safe_css_token_value, is_valid_css_token_name, scan_document_citations,
    short_token,
};
use quire_store::{
    ApplyResult, CommitExtras, NewCommentReply, NewCommentThread, SourceUpsert, StoreError,
};
use rmcp::ErrorData;

use crate::{
    auth::RequestContext,
    error::{McpErrorDetails, invalid_params, mcp_error},
    guide::{guide, guide_workflow},
    state::AppState,
};

use super::{QuireReportService, lint_hint, schema::*};

mod chrome;
mod lifecycle;
mod lifecycle_reads;
mod mutations;
mod reads;
mod review;
mod support;

pub(crate) use support::*;
