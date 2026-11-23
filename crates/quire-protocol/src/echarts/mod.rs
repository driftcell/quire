//! Shared ECharts 6.0 option inspection helpers.
//!
//! Modules separate option merging, structural inspection and renderer-agnostic data projection.

mod data_projection;
mod data_tables;
mod inspect;
mod merge;

pub use data_tables::*;
pub use inspect::*;
pub use merge::*;

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    block::{EchartsBlock, format_number},
    echarts_view::EchartsOptionView,
    text::truncate_chars,
};
