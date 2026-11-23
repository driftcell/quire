//! Block AST definitions and shared block metadata.

mod columns;
mod model;
mod style;
mod table;
#[cfg(test)]
mod tests;
mod visual;

pub use columns::*;
pub use model::*;
pub use style::*;
pub use table::*;
pub use visual::*;

use std::collections::{BTreeMap, BTreeSet};

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    ids::{BlockId, SourceId},
    text::slugify,
};
