//! Thin rmcp adapters over the shared report application service.

use axum::http::request::Parts;
use quire_protocol::{COVER_VARIANTS, CoverConfig};
use rmcp::{
    ErrorData,
    handler::server::{
        router::tool::ToolRouter,
        wrapper::{Json, Parameters},
    },
    tool, tool_router,
};

use crate::{
    auth::RequestContext,
    error::{McpErrorDetails, invalid_params},
    report::{QuireReportService, schema::*},
};

use super::QuireMcpServer;

mod lifecycle;
mod mutations;
mod support;

pub(crate) use support::*;

impl QuireMcpServer {
    pub(crate) fn tool_router() -> ToolRouter<Self> {
        let mut router = Self::lifecycle_router();
        router.merge(Self::mutations_router());
        router
    }
}
