//! Lint output and configuration contracts shared by the linter, store and frontend.
//!
//! The pure rule engine lives in the `quire-lint` crate. This module owns only the wire types
//! and document-level configuration so every crate can exchange diagnostics without depending
//! on the engine.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::BlockId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Diagnostic {
    /// Stable rule code, for example `QR202`.
    pub code: String,
    /// Kebab-case semantic rule name, for example `table-empty-column`.
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    /// Actionable next step, including valid IDs/enums when possible.
    pub hint: String,
    /// Whether callers may suppress this diagnostic through `LintConfig.ignore` or
    /// `Block.lint_ignore`. Core contract and safety rules are never suppressible.
    pub suppressible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<BlockId>,
    /// JSON-ish path such as `blocks.financials.columns`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub valid_values: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LintSummary {
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
    /// Suppressible diagnostics hidden by document/block/call ignore filters.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub suppressed: usize,
}

const fn is_zero(value: &usize) -> bool {
    *value == 0
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LintReport {
    pub diagnostics: Vec<Diagnostic>,
    pub summary: LintSummary,
}

impl LintReport {
    pub fn has_errors(&self) -> bool {
        self.summary.errors > 0
    }

    pub fn is_clean(&self) -> bool {
        self.diagnostics.is_empty()
    }

    pub fn error_codes(&self) -> Vec<&str> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Error)
            .map(|diagnostic| diagnostic.code.as_str())
            .collect()
    }
}

/// Document-level lint policy. Ignore entries accept a rule code (`QR202`), a semantic rule name
/// (`table-empty-column`) or a category prefix (`QR2`). Non-suppressible rules cannot be ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LintConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore: Vec<String>,
}
