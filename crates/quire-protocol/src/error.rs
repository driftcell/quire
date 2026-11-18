//! Structured protocol errors with actionable recovery fields.

use schemars::JsonSchema;
use serde::Serialize;
use thiserror::Error;

/// Errors raised while constructing validated primitives or resolving protocol constraints.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, JsonSchema)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum ProtocolError {
    #[error("invalid doc_id `{value}`: {reason}")]
    InvalidDocId {
        value: String,
        reason: String,
        valid_examples: Vec<String>,
    },
    #[error("invalid block_id `{value}`: {reason}")]
    InvalidBlockId {
        value: String,
        reason: String,
        valid_examples: Vec<String>,
    },
    #[error("invalid source_id `{value}`: {reason}")]
    InvalidSourceId {
        value: String,
        reason: String,
        valid_examples: Vec<String>,
    },
    #[error("invalid session_id `{value}`: {reason}")]
    InvalidSessionId {
        value: String,
        reason: String,
        valid_examples: Vec<String>,
    },
    #[error("invalid revision `{value}`: revisions must be non-negative")]
    InvalidRevision { value: i64 },
    #[error("invalid rank `{value}`: {reason}")]
    InvalidRank {
        value: String,
        reason: String,
        valid_examples: Vec<String>,
    },
    #[error("citation `[^{source_id}]` in block `{block_id}` has no registered source")]
    DanglingCitation {
        source_id: String,
        block_id: String,
        available_sources: Vec<String>,
    },
    #[error("block `{block_id}` repeats its outer title `{title}` inside the visual payload")]
    DuplicateInnerTitle {
        block_id: String,
        title: String,
        recovery: String,
    },
    #[error("block `{block_id}` title contains manual numbering: `{title}`")]
    ManualPrefixInTitle {
        block_id: String,
        title: String,
        recovery: String,
    },
    #[error("invalid table schema in block `{block_id}`: {message}")]
    InvalidTableSchema {
        block_id: String,
        message: String,
        valid_columns: Vec<String>,
    },
    #[error("unbalanced CSS braces at line {line}, column {column}: {message}")]
    UnbalancedCssBraces {
        line: usize,
        column: usize,
        message: String,
        recovery: String,
    },
}

impl ProtocolError {
    /// Stable machine-readable code used in MCP and HTTP error envelopes.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidDocId { .. } => "invalid_doc_id",
            Self::InvalidBlockId { .. } => "invalid_block_id",
            Self::InvalidSourceId { .. } => "invalid_source_id",
            Self::InvalidSessionId { .. } => "invalid_session_id",
            Self::InvalidRevision { .. } => "invalid_revision",
            Self::InvalidRank { .. } => "invalid_rank",
            Self::DanglingCitation { .. } => "dangling_citation",
            Self::DuplicateInnerTitle { .. } => "duplicate_inner_title",
            Self::ManualPrefixInTitle { .. } => "manual_prefix_in_title",
            Self::InvalidTableSchema { .. } => "invalid_table_schema",
            Self::UnbalancedCssBraces { .. } => "unbalanced_css_braces",
        }
    }

    /// Actionable, self-healing recovery hint for agents and API clients.
    pub fn hint(&self) -> String {
        match self {
            Self::InvalidDocId { valid_examples, .. } => {
                format!("Use a short semantic doc_id; {}", examples(valid_examples))
            }
            Self::InvalidBlockId { valid_examples, .. } => {
                format!(
                    "Use a short semantic block_id; {}",
                    examples(valid_examples)
                )
            }
            Self::InvalidSourceId { valid_examples, .. } => {
                format!("Use a short citation key; {}", examples(valid_examples))
            }
            Self::InvalidSessionId { valid_examples, .. } => {
                format!(
                    "Use a stable agent session id; {}",
                    examples(valid_examples)
                )
            }
            Self::InvalidRevision { .. } => {
                "Revisions are non-negative and monotonic; re-read current_rev and retry.".into()
            }
            Self::InvalidRank { valid_examples, .. } => {
                format!("Use an existing base-62 rank; {}", examples(valid_examples))
            }
            Self::DanglingCitation {
                source_id,
                available_sources,
                ..
            } => {
                if available_sources.is_empty() {
                    format!(
                        "Register `{source_id}` with upsert_sources before citing it, or remove the `[^{source_id}]` marker."
                    )
                } else {
                    format!(
                        "Register `{source_id}` with upsert_sources or cite an existing source: {}.",
                        available_sources.join(", ")
                    )
                }
            }
            Self::DuplicateInnerTitle { recovery, .. }
            | Self::ManualPrefixInTitle { recovery, .. }
            | Self::UnbalancedCssBraces { recovery, .. } => recovery.clone(),
            Self::InvalidTableSchema { valid_columns, .. } => {
                if valid_columns.is_empty() {
                    "Rebuild the table from the documented column schema.".into()
                } else {
                    format!(
                        "Use only the declared leaf columns ({}); every row key must match one of them.",
                        valid_columns.join(", ")
                    )
                }
            }
        }
    }

    pub fn invalid_doc_id(value: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidDocId {
            value: value.into(),
            reason: reason.into(),
            valid_examples: vec!["q3-report".into(), "report_2026".into()],
        }
    }

    pub fn invalid_block_id(value: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidBlockId {
            value: value.into(),
            reason: reason.into(),
            valid_examples: vec!["sec_intro".into(), "b_01".into(), "q3-revenue".into()],
        }
    }

    pub fn invalid_source_id(value: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidSourceId {
            value: value.into(),
            reason: reason.into(),
            valid_examples: vec!["sec10".into(), "src_market".into()],
        }
    }

    pub fn invalid_session_id(value: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidSessionId {
            value: value.into(),
            reason: reason.into(),
            valid_examples: vec!["sess_01".into(), "agent-run-42".into()],
        }
    }

    pub fn invalid_rank(value: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidRank {
            value: value.into(),
            reason: reason.into(),
            valid_examples: vec!["U".into(), "V".into(), "b_01".into()],
        }
    }
}

fn examples(values: &[String]) -> String {
    if values.is_empty() {
        "supply a valid value from the documented grammar".to_string()
    } else {
        format!("examples: {}", values.join(", "))
    }
}
