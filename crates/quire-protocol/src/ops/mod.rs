//! Declarative block patch operations.
//!
//! Operations are intentionally frontend-agnostic. The store applies them transactionally,
//! computes inverse operations, and broadcasts the same JSON to read-only SSE clients.

mod apply;
mod block_patch;
mod model;
mod remap;
#[cfg(test)]
mod tests;
mod wire;

pub use apply::*;
pub(crate) use block_patch::apply_block_patch;
pub use model::*;
pub use remap::*;

use std::collections::{BTreeMap, BTreeSet};

use indexmap::IndexMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    block::{
        Block, BlockKind, BlockStyle, CellValue, ColumnNode, NumberingKind, is_semantic_block_id,
    },
    echarts::merge_chart_option_patch,
    ids::{BlockId, SourceId},
    lint::LintConfig,
    model::{CoverConfig, ReferencesConfig, ReportDocument, SourceEntry, ThemeConfig, TocConfig},
    table::validate_table,
};
