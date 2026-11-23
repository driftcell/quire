//! Quire server library: Axum REST/SSE routes and the MCP tool server.

#![forbid(unsafe_code)]

pub mod auth;
pub mod config;
pub mod error;
pub mod guide;
pub mod jwt;
pub mod mcp;
pub mod routes;
pub mod sse;
pub mod state;

pub use config::Config;
pub use routes::{build_document_router, build_router};
pub use state::AppState;
