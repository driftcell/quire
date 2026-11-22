//! Official Rig MCP client wrapper over `rig-rmcp`.

use std::collections::HashMap;
use std::time::Duration;

use http::{HeaderName, HeaderValue};
use rig::tool::rmcp::{
    McpCallToolResult, McpConnection, McpMeta, McpTool, rmcp,
    rmcp::{
        ServiceExt,
        transport::streamable_http_client::{
            StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
        },
    },
};
use rig::tool::{DynamicTool, ToolContext};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;

use crate::error::{RuntimeError, RuntimeResult};
use crate::mcp_registry::tool_pattern_matches;

#[derive(Debug, Clone, PartialEq)]
pub struct McpToolDescriptor {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub struct RmcpClient {
    connection: McpConnection,
    tools: Vec<McpTool>,
}

impl RmcpClient {
    pub async fn connect(
        url: &str,
        token: Option<SecretString>,
        session_id: &str,
        agent_id: &str,
    ) -> RuntimeResult<Self> {
        Self::connect_inner(url, token, Some((session_id, agent_id))).await
    }

    pub async fn connect_shared(url: &str, token: Option<SecretString>) -> RuntimeResult<Self> {
        Self::connect_inner(url, token, None).await
    }

    async fn connect_inner(
        url: &str,
        token: Option<SecretString>,
        identity: Option<(&str, &str)>,
    ) -> RuntimeResult<Self> {
        let endpoint = if url.trim_end_matches('/').ends_with("/mcp") {
            url.trim_end_matches('/').to_string()
        } else {
            format!("{}/mcp", url.trim_end_matches('/'))
        };
        let mut config = StreamableHttpClientTransportConfig::with_uri(endpoint);
        if let Some(token) = token {
            config = config.auth_header(token.expose_secret().to_string());
        }
        let mut headers = HashMap::new();
        if let Some((session_id, agent_id)) = identity {
            for name in ["x-session-id", "x-agent-id", "x-actor"] {
                let header = HeaderName::from_static(name);
                let value = HeaderValue::from_str(if name == "x-session-id" {
                    session_id
                } else {
                    agent_id
                })
                .map_err(|error| {
                    RuntimeError::InvalidRequest(format!("invalid MCP header value: {error}"))
                })?;
                headers.insert(header, value);
            }
        }
        config.custom_headers = headers;

        let http = rig::rig_reqwest::reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|error| {
                RuntimeError::Internal(format!("MCP HTTP client init failed: {error}"))
            })?;
        let transport = StreamableHttpClientTransport::with_client(http, config);
        let service = rmcp::model::ClientConfig::default()
            .serve(transport)
            .await
            .map_err(|error| RuntimeError::Internal(format!("MCP initialize failed: {error}")))?;
        let connection = McpConnection::new(service);
        let tools = connection
            .tools()
            .await
            .map_err(|error| RuntimeError::Internal(format!("MCP tools/list failed: {error}")))?;
        Ok(Self { connection, tools })
    }

    pub fn dynamic_tools(&self, allowed_tools: &[String]) -> RuntimeResult<Vec<DynamicTool>> {
        self.tools
            .iter()
            .filter(|tool| {
                allowed_tools
                    .iter()
                    .any(|pattern| tool_pattern_matches(pattern, tool.name()))
            })
            .cloned()
            .map(|tool| {
                DynamicTool::try_from(tool)
                    .map_err(|error| RuntimeError::Internal(format!("invalid MCP tool: {error}")))
            })
            .collect()
    }

    pub async fn list_tools(&self) -> RuntimeResult<Vec<McpToolDescriptor>> {
        Ok(self
            .tools
            .iter()
            .map(|tool| {
                let definition = tool.definition();
                McpToolDescriptor {
                    name: definition.name.to_string(),
                    description: definition
                        .description
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_default(),
                    input_schema: definition.schema_as_json_value(),
                }
            })
            .collect())
    }

    async fn run_tool(
        &self,
        name: &str,
        arguments: Value,
        meta: Option<McpMeta>,
    ) -> RuntimeResult<rmcp::model::CallToolResult> {
        let tool = self
            .tools
            .iter()
            .find(|tool| tool.name() == name)
            .cloned()
            .ok_or_else(|| {
                RuntimeError::InvalidRequest(format!("MCP tool `{name}` is not available"))
            })?;
        let dynamic = DynamicTool::try_from(tool)
            .map_err(|error| RuntimeError::Internal(format!("invalid MCP tool: {error}")))?;
        let mut context = ToolContext::new();
        if let Some(meta) = meta {
            context.insert(meta).map_err(|error| {
                RuntimeError::Internal(format!("MCP request context failed: {error}"))
            })?;
        }
        dynamic
            .run_with(&mut context, arguments)
            .await
            .map_err(|error| {
                RuntimeError::Internal(format!("MCP tool `{name}` failed: {error}"))
            })?;
        context
            .result::<McpCallToolResult>()
            .map_err(|error| RuntimeError::Internal(format!("MCP result context failed: {error}")))?
            .map(|result| result.0)
            .ok_or_else(|| {
                RuntimeError::Internal(format!("MCP tool `{name}` returned no raw result"))
            })
    }

    pub async fn call_tool_raw(&self, name: &str, arguments: Value) -> RuntimeResult<Value> {
        Ok(serde_json::to_value(
            self.run_tool(name, arguments, None).await?,
        )?)
    }

    fn tool_value(result: rmcp::model::CallToolResult) -> Value {
        if let Some(structured) = result.structured_content {
            return structured;
        }
        if let Some(text) = result
            .content
            .first()
            .and_then(rmcp::model::ContentBlock::as_text)
            .map(|text| text.text.as_str())
        {
            return serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.into()));
        }
        Value::Null
    }

    pub async fn call_tool(&self, name: &str, arguments: Value) -> RuntimeResult<Value> {
        Ok(Self::tool_value(
            self.run_tool(name, arguments, None).await?,
        ))
    }

    pub async fn call_tool_with_meta(
        &self,
        name: &str,
        arguments: Value,
        meta: McpMeta,
    ) -> RuntimeResult<Value> {
        Ok(Self::tool_value(
            self.run_tool(name, arguments, Some(meta)).await?,
        ))
    }

    pub fn is_closed(&self) -> bool {
        self.connection.is_closed()
    }
}
