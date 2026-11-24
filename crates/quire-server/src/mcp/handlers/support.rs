use super::*;

// rmcp moves params._meta into RequestContext.meta before tool extraction; it is
// not guaranteed to remain in the tool's Extensions map.
pub(crate) fn request_context(
    context: &rmcp::service::RequestContext<rmcp::RoleServer>,
) -> Result<RequestContext, ErrorData> {
    let parts = context.extensions.get::<Parts>().ok_or_else(|| {
        invalid_params(
            "MCP request is missing HTTP context",
            "Use the Quire streamable HTTP transport.",
            None,
        )
    })?;
    RequestContext::from_mcp(parts, Some(&context.meta)).map_err(|error| {
        invalid_params(
            error,
            "Use matching X-Session-Id/X-Actor headers or _meta.quire.{session_id,run_id,agent_id}.",
            None,
        )
    })
}
