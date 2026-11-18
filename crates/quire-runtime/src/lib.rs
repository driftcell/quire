//! Independent Quire session backend and agent runtime.

#![forbid(unsafe_code)]

pub mod bus;
pub mod config;
pub mod control;
pub mod error;
pub mod http;
pub mod kb;
pub mod prompt;
pub mod rig_agent;
pub mod sandbox;
pub mod service;
pub mod store;

/// Compatibility module for callers that refer to the runtime's `rig` integration.
pub mod rig {
    pub use crate::rig_agent::*;
}

pub use config::Config;
pub use error::{RuntimeError, RuntimeResult};
pub use kb::KbService;
pub use service::{
    HttpQuireControl, QuireControl, RuntimeService, StaticQuireControl, ToolCallRecord,
};
pub use store::RuntimeStore;
