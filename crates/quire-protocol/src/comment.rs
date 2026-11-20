//! Side-channel review comments.
//!
//! Comment threads deliberately live outside the revision/DAG model. They reference block IDs as
//! soft anchors and retain an `anchor_snapshot` so review context survives block deletion or
//! replacement. Only the human REST surface may archive or reopen a thread.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{BlockId, DocId, Revision};

/// Who wrote a comment or reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CommentAuthorType {
    Human,
    Agent,
}

impl CommentAuthorType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Agent => "agent",
        }
    }

    /// Parse a persisted or wire author type.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "human" => Some(Self::Human),
            "agent" => Some(Self::Agent),
            _ => None,
        }
    }
}

/// Review-thread lifecycle.
///
/// Agents may move `open` to `addressed`; only the human REST surface may archive or reopen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CommentStatus {
    Open,
    Addressed,
    Archived,
}

impl CommentStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Addressed => "addressed",
            Self::Archived => "archived",
        }
    }

    /// Parse a query-string or persisted status.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "open" => Some(Self::Open),
            "addressed" => Some(Self::Addressed),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

/// Text-selection anchor that avoids brittle character offsets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextQuoteSelector {
    pub exact: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
}

impl TextQuoteSelector {
    pub fn validate(&self) -> Result<(), String> {
        if self.exact.trim().is_empty() {
            return Err("quote.exact must not be empty".into());
        }
        if self.exact.chars().count() > 2_000 {
            return Err("quote.exact must be at most 2000 characters".into());
        }
        Ok(())
    }
}

/// One message in a review thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CommentMessage {
    pub message_id: String,
    pub thread_id: String,
    pub author_type: CommentAuthorType,
    pub author: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

/// A side-channel review thread anchored to one or more blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CommentThread {
    pub thread_id: String,
    pub doc_id: DocId,
    /// Starting block. Kept as the canonical section/order anchor.
    pub block_id: BlockId,
    /// Ending block for a cross-block selection. `None` for a single-block anchor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_block_id: Option<BlockId>,
    /// Every selected block in document order, including intermediate tables, images and charts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anchor_blocks: Vec<BlockId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_id: Option<BlockId>,
    /// Truncated visible text captured when the thread was created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_snapshot: Option<String>,
    /// Start-block text snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<TextQuoteSelector>,
    /// End-block text snapshot for cross-block selections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_quote: Option<TextQuoteSelector>,
    pub status: CommentStatus,
    pub created_rev: Revision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addressed_rev: Option<Revision>,
    /// True when none of `block_id`, `end_block_id` or `anchor_blocks` still exist.
    pub anchor_detached: bool,
    pub messages: Vec<CommentMessage>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{block_id, doc_id, revision};

    fn message() -> CommentMessage {
        CommentMessage {
            message_id: "msg_1".into(),
            thread_id: "th_1".into(),
            author_type: CommentAuthorType::Human,
            author: "reviewer".into(),
            body: "Tighten this paragraph.".into(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn comment_thread_roundtrips() {
        let thread = CommentThread {
            thread_id: "th_1".into(),
            doc_id: doc_id("report_1"),
            block_id: block_id("ch1_summary"),
            end_block_id: Some(block_id("ch1_chart")),
            anchor_blocks: vec![
                block_id("ch1_summary"),
                block_id("ch1_table"),
                block_id("ch1_chart"),
            ],
            section_id: Some(block_id("ch1")),
            anchor_snapshot: Some("Revenue grew 18% … [图表: Margin]".into()),
            quote: Some(TextQuoteSelector {
                exact: "18%".into(),
                prefix: Some("grew ".into()),
                suffix: None,
            }),
            end_quote: Some(TextQuoteSelector {
                exact: "margin".into(),
                prefix: None,
                suffix: None,
            }),
            status: CommentStatus::Open,
            created_rev: revision(4),
            addressed_rev: None,
            anchor_detached: false,
            messages: vec![message()],
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let json = serde_json::to_value(&thread).unwrap();
        assert_eq!(json["status"], "open");
        assert_eq!(json["messages"][0]["author_type"], "human");
        let decoded: CommentThread = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, thread);
    }

    #[test]
    fn status_and_author_helpers_match_serde_values() {
        for status in [
            CommentStatus::Open,
            CommentStatus::Addressed,
            CommentStatus::Archived,
        ] {
            assert_eq!(CommentStatus::parse(status.as_str()), Some(status));
            assert_eq!(serde_json::to_value(status).unwrap(), status.as_str());
        }
        for author_type in [CommentAuthorType::Human, CommentAuthorType::Agent] {
            assert_eq!(
                CommentAuthorType::parse(author_type.as_str()),
                Some(author_type)
            );
            assert_eq!(
                serde_json::to_value(author_type).unwrap(),
                author_type.as_str()
            );
        }
        assert!(CommentStatus::parse("resolved").is_none());
    }

    #[test]
    fn quote_selector_requires_non_empty_exact_text() {
        let empty = TextQuoteSelector {
            exact: "  ".into(),
            prefix: None,
            suffix: None,
        };
        assert!(empty.validate().is_err());
        let valid = TextQuoteSelector {
            exact: "Revenue".into(),
            prefix: Some("grew ".into()),
            suffix: Some(" year".into()),
        };
        assert!(valid.validate().is_ok());
    }
}
