//! Pure, synchronous static analysis for Quire report documents.
//!
//! The crate owns the centralized rule registry, layered select/ignore filtering and the
//! rule engine. It performs no I/O and has no CLI. `quire-protocol` owns the wire types
//! (`Diagnostic`, `LintSummary`, `LintReport`, `LintConfig`) and document configuration.

#![forbid(unsafe_code)]

pub mod engine;
pub mod normalize;
pub mod registry;
pub(crate) mod rules;

pub use engine::{LintOptions, lint_document, lint_document_with_options, lint_echarts_block};
pub use registry::{RULES, RuleId, RuleMeta, rule_matches_token};
