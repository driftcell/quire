//! Quire server configuration.

use std::{env, net::SocketAddr, path::PathBuf, time::Duration};

use secrecy::SecretString;
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub database_url: SecretString,
    pub redis_url: SecretString,
    /// Token allowed to call MCP and mutating admin routes.
    pub rw_token: Option<SecretString>,
    /// Token allowed to call read-only frontend routes. RW tokens are also accepted.
    pub ro_token: Option<SecretString>,
    /// Allowed Host headers for the MCP Streamable HTTP endpoint.
    pub allowed_hosts: Vec<String>,
    /// Browser origins allowed to call the read-only API. Empty means permissive (dev default).
    pub cors_allowed_origins: Vec<String>,
    /// Root directory for offline draft JSON files.
    pub workspace_dir: PathBuf,
    pub max_draft_bytes: usize,
    pub sse_heartbeat: Duration,
    pub auto_migrate: bool,
    pub mcp_json_response: bool,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing required environment variable {0}")]
    Missing(&'static str),
    #[error("invalid environment variable {name}: {message}")]
    Invalid { name: &'static str, message: String },
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let bind_addr_raw = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());
        let bind_addr: SocketAddr =
            bind_addr_raw
                .parse()
                .map_err(|error| ConfigError::Invalid {
                    name: "BIND_ADDR",
                    message: format!("{error}; expected HOST:PORT"),
                })?;
        let database_url = SecretString::from(
            env::var("DATABASE_URL").map_err(|_| ConfigError::Missing("DATABASE_URL"))?,
        );
        let redis_url = SecretString::from(
            env::var("REDIS_URL").map_err(|_| ConfigError::Missing("REDIS_URL"))?,
        );
        let rw_token = env::var("QUIRE_RW_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(SecretString::from);
        let ro_token = env::var("QUIRE_RO_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(SecretString::from);
        let allowed_hosts = env::var("MCP_ALLOWED_HOSTS")
            .ok()
            .map(|value| split_csv(&value))
            .filter(|values| !values.is_empty())
            .unwrap_or_else(|| {
                vec![
                    "localhost".into(),
                    "127.0.0.1".into(),
                    "0.0.0.0".into(),
                    "[::1]".into(),
                ]
            });
        let cors_allowed_origins = env::var("CORS_ALLOW_ORIGINS")
            .ok()
            .map(|value| split_csv(&value))
            .unwrap_or_default();
        let workspace_dir = env::var("QUIRE_WORKSPACE_DIR")
            .map(PathBuf::from)
            .unwrap_or(env::current_dir().map_err(|error| ConfigError::Invalid {
                name: "QUIRE_WORKSPACE_DIR",
                message: error.to_string(),
            })?);
        let max_draft_bytes = env::var("QUIRE_MAX_DRAFT_BYTES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(5 * 1024 * 1024);
        let sse_heartbeat = env::var("SSE_HEARTBEAT_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(15));
        let auto_migrate = env::var("AUTO_MIGRATE")
            .ok()
            .map(|value| !matches!(value.to_ascii_lowercase().as_str(), "0" | "false" | "no"))
            .unwrap_or(true);
        let mcp_json_response = env::var("MCP_JSON_RESPONSE")
            .ok()
            .map(|value| !matches!(value.to_ascii_lowercase().as_str(), "0" | "false" | "no"))
            .unwrap_or(true);
        Ok(Self {
            bind_addr,
            database_url,
            redis_url,
            rw_token,
            ro_token,
            allowed_hosts,
            cors_allowed_origins,
            workspace_dir,
            max_draft_bytes,
            sse_heartbeat,
            auto_migrate,
            mcp_json_response,
        })
    }
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}
