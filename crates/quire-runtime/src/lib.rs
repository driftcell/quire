//! Independent Quire session backend and agent runtime.

#![forbid(unsafe_code)]

pub mod admin_mcp;
pub mod bus;
pub mod compaction;
pub mod config;
pub mod control;
pub mod delta;
pub mod error;
pub mod http;
pub mod kb;
pub mod mcp_client;
pub mod mcp_registry;
pub mod model_stream;
pub mod models;
pub mod preview;
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
pub use delta::{CoalescedDelta, DeltaCoalescer, DeltaKind};
pub use error::{RuntimeError, RuntimeResult};
pub use kb::{KbService, SeedReport};
pub use mcp_registry::{
    McpRegistry, McpServerBinding, McpServerInput, McpServerPatch, McpServerRecord, McpSyncReport,
    McpToolRecord,
};
pub use model_stream::{
    MockStreamProvider, ModelEventStream, ModelStreamEvent, ModelStreamProvider, ModelTurnRequest,
    StubStreamProvider,
};
pub use models::{
    ModelInput, ModelRecord, ModelRegistry, ModelRouter, ModelsDevImportReport,
    ModelsDevImportRequest, ProviderInput, ProviderProtocol, ProviderRecord, ResolvedModel,
};
pub use rig_agent::RigModelStreamProvider;
pub use service::{
    HttpQuireControl, QuireControl, RuntimeService, SessionInspection, StaticQuireControl,
    ToolCallRecord,
};
pub use store::RuntimeStore;
