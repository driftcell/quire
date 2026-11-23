use std::collections::BTreeMap;

use quire_protocol::{CommentStatus, CommentThread, Diagnostic, LintSummary, TextQuoteSelector};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Input for `lint_document`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LintParams {
    /// Target report `doc_id`.
    pub doc_id: String,
    /// Optional rule filter: only report rules matching these codes (`"QR204"`), rule names, or category prefixes (`"QR1"`, `"QR2"`, `"QR3"`, `"QR4"`). Omit to run the full lint suite.
    #[serde(default)]
    pub select: Vec<String>,
    /// Optional list of suppressible rule codes, names, or prefixes to hide during this check. Core structural and safety rules (`suppressible: false`) cannot be ignored.
    #[serde(default)]
    pub ignore: Vec<String>,
}

/// Output for `lint_document`. Clean documents return only `{"ok":true,"rev":N}`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct LintReceipt {
    pub ok: bool,
    pub rev: i64,
    /// Error, warning and info counts (omitted when clean).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<LintSummary>,
    /// Blocks with non-empty `lint_ignore` lists (`block_id -> ignored codes`), omitted when empty.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub suppressed_blocks: BTreeMap<String, Vec<String>>,
    /// Diagnostics (omitted when clean).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(
        rename = "_next_hint",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub _next_hint: Option<String>,
}

/// Input for `list_comments`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListCommentsParams {
    /// Target report `doc_id`.
    pub doc_id: String,
    /// Optional thread status filter (`"open"`, `"addressed"`, or `"archived"`). When omitted, returns `open` and `addressed` threads and hides `archived` threads.
    #[serde(default)]
    pub status: Option<CommentStatus>,
    /// Optional `block_id` filter returning only threads anchored to that block.
    #[serde(default)]
    pub block_id: Option<String>,
}

/// Output for `list_comments`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListCommentsReceipt {
    pub doc_id: String,
    pub comments: Vec<CommentThread>,
    pub count: usize,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// Input for `add_comment`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddCommentParams {
    /// Target report `doc_id`.
    pub doc_id: String,
    /// Existing `block_id` where the review thread anchor starts.
    pub block_id: String,
    /// Optional ending `block_id` when the selection spans multiple blocks.
    #[serde(default)]
    pub end_block_id: Option<String>,
    /// Optional ordered list of all selected `block_id`s in a multi-block selection.
    #[serde(default)]
    pub anchor_blocks: Vec<String>,
    /// Actionable review comment text explaining the issue and expected fix.
    pub body: String,
    /// Optional exact text-quote selector (`exact`, optional `prefix`/`suffix`) inside `block_id`.
    #[serde(default)]
    pub quote: Option<TextQuoteSelector>,
    /// Optional ending text-quote selector inside `end_block_id` for cross-block selections.
    #[serde(default)]
    pub end_quote: Option<TextQuoteSelector>,
    /// Optional markdown/text snapshot of the anchored content at comment creation time.
    #[serde(default)]
    pub anchor_snapshot: Option<String>,
}

/// Output for `add_comment`. Thread fields are flattened for compact receipts.
#[derive(Debug, Serialize, JsonSchema)]
pub struct AddCommentReceipt {
    #[serde(flatten)]
    pub thread: CommentThread,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

/// Input for `reply_comment`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplyCommentParams {
    /// Target report `doc_id`.
    pub doc_id: String,
    /// Comment thread ID (`thread_id`) from `list_comments` or `<comment_ref>`.
    pub thread_id: String,
    /// Concise reply explaining how the comment was resolved or answered.
    pub body: String,
    /// Set `true` after committing the fix via `apply_operations` to transition the thread from `open` to `addressed`.
    #[serde(default)]
    pub mark_addressed: bool,
    /// Document revision number (`new_revision` from `apply_operations`) that contains the fix. Defaults to the current document revision when omitted.
    #[serde(default)]
    pub addressed_rev: Option<i64>,
}

/// Output for `reply_comment`. Thread fields are flattened for compact receipts.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ReplyCommentReceipt {
    #[serde(flatten)]
    pub thread: CommentThread,
    #[serde(rename = "_next_hint")]
    pub _next_hint: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::schema::{
        ApplyOperationsParams, ConfigureTocParams, CreateReportParams, ListReportsParams,
        SetCoverParams,
    };
    use serde_json::json;

    #[test]
    fn tool_params_reject_unknown_fields_with_field_name() {
        let error = serde_json::from_value::<ListReportsParams>(json!({
            "query": "atlas",
            "limitt": 5
        }))
        .expect_err("unknown field must be rejected");
        assert!(error.to_string().contains("limitt"), "{error}");

        let error = serde_json::from_value::<CreateReportParams>(json!({
            "title": "Synthetic",
            "enable_cvoer": true
        }))
        .expect_err("misspelled field must be rejected");
        assert!(error.to_string().contains("enable_cvoer"), "{error}");
    }

    #[test]
    fn nested_cover_patch_rejects_unknown_fields() {
        let error = serde_json::from_value::<SetCoverParams>(json!({
            "doc_id": "synthetic",
            "cover_patch": {"enabeld": true}
        }))
        .expect_err("unknown nested field must be rejected");
        assert!(error.to_string().contains("enabeld"), "{error}");
    }

    #[test]
    fn valid_params_still_deserialize() {
        let params: ApplyOperationsParams = serde_json::from_value(json!({
            "doc_id": "synthetic",
            "expected_revision": 7,
            "summary": "write section",
            "ops": [{"type": "append", "blocks": []}]
        }))
        .expect("valid params");
        assert_eq!(params.expected_revision, 7);

        let reply: ReplyCommentParams = serde_json::from_value(json!({
            "doc_id": "synthetic",
            "thread_id": "th_1",
            "body": "Fixed",
            "mark_addressed": true,
            "addressed_rev": 4
        }))
        .expect("valid comment reply");
        assert!(reply.mark_addressed);
        assert_eq!(reply.addressed_rev, Some(4));
    }

    #[test]
    fn configure_toc_accepts_page_break_options() {
        let params: ConfigureTocParams = serde_json::from_value(json!({
            "doc_id": "synthetic",
            "enabled": true,
            "page_break_before": true,
            "page_break_after": false
        }))
        .expect("valid TOC page-break params");
        assert_eq!(params.page_break_before, Some(true));
        assert_eq!(params.page_break_after, Some(false));
    }

    #[test]
    fn agent_comment_tools_reject_archive_status() {
        let error = serde_json::from_value::<ReplyCommentParams>(json!({
            "doc_id": "synthetic",
            "thread_id": "th_1",
            "body": "Archived",
            "status": "archived"
        }))
        .expect_err("agents cannot set archived through reply_comment");
        assert!(error.to_string().contains("status"), "{error}");
    }
}
