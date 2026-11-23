//! Independent Quire session backend and agent runtime.

#![forbid(unsafe_code)]

pub mod admin_mcp;
pub mod auth;
pub mod bus;
pub mod comment_refs;
pub mod compaction;
pub mod config;
pub mod control;
pub mod delta;
pub mod error;
pub mod events;
pub mod http;
pub(crate) mod http_client;
pub mod kb;
pub mod mcp_client;
pub mod mcp_registry;
pub mod memory;
pub mod model_stream;
pub mod models;
pub mod oss;
pub mod preview;
pub mod rig_agent;
pub mod sandbox;
pub mod service;
pub mod store;

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
pub use service::{RuntimeService, SessionInspection};
pub use store::RuntimeStore;
