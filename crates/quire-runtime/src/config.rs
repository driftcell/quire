//! Runtime configuration. Secret values stay inside `SecretString`.

use std::{
    env,
    fmt::{Debug, Formatter},
    path::PathBuf,
};

use secrecy::SecretString;

use crate::{models::EvaluatorConfig, oss::OssConfig, sandbox::SandboxConfig};

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub redis_url: String,
    pub bind_addr: String,
    pub quire_base_url: String,
    pub jwt_secret: SecretString,
    pub jwt_ttl_secs: u64,
    pub admin_username: String,
    pub admin_email: Option<String>,
    pub admin_password: SecretString,
    pub model_api_key: Option<SecretString>,
    pub model_provider: String,
    pub model_base_url: Option<String>,
    pub master_key: Option<SecretString>,
    pub models_dev_url: String,
    pub workspace_dir: PathBuf,
    pub default_model: String,
    pub sse_replay_limit: usize,
    pub sse_heartbeat_ms: u64,
    pub preview_throttle_ms: u64,
    pub preview_base_url: String,
    pub preview_public_url: String,
    pub playwright_mcp_url: Option<String>,
    pub sandbox: SandboxConfig,
    pub oss: OssConfig,
    pub evaluator: EvaluatorConfig,
}

impl Debug for Config {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Config")
            .field("database_url", &"[REDACTED]")
            .field("redis_url", &"[REDACTED]")
            .field("bind_addr", &self.bind_addr)
            .field("quire_base_url", &self.quire_base_url)
            .field("jwt_secret", &"[REDACTED]")
            .field("jwt_ttl_secs", &self.jwt_ttl_secs)
            .field("admin_username", &self.admin_username)
            .field("admin_email", &self.admin_email)
            .field("admin_password", &"[REDACTED]")
            .field(
                "model_api_key",
                &self.model_api_key.as_ref().map(|_| "[REDACTED]"),
            )
            .field("model_provider", &self.model_provider)
            .field("model_base_url", &self.model_base_url)
            .field(
                "master_key",
                &self.master_key.as_ref().map(|_| "[REDACTED]"),
            )
            .field("models_dev_url", &self.models_dev_url)
            .field("workspace_dir", &self.workspace_dir)
            .field("default_model", &self.default_model)
            .field("sse_replay_limit", &self.sse_replay_limit)
            .field("sse_heartbeat_ms", &self.sse_heartbeat_ms)
            .field("preview_throttle_ms", &self.preview_throttle_ms)
            .field("preview_base_url", &self.preview_base_url)
            .field("preview_public_url", &self.preview_public_url)
            .field("playwright_mcp_url", &self.playwright_mcp_url)
            .field("sandbox", &self.sandbox)
            .field("oss", &self.oss)
            .field("evaluator", &self.evaluator)
            .finish()
    }
}

impl Config {
    pub fn from_env() -> Self {
        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://report:report@127.0.0.1:5432/report_core".into());
        let redis_url = env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
        let bind_addr = env::var("RUNTIME_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8081".into());
        let quire_base_url =
            env::var("QUIRE_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into());
        let master_key = env::var("RUNTIME_MASTER_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(SecretString::from);
        let jwt_secret_raw = env::var("JWT_SECRET")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                env::var("RUNTIME_MASTER_KEY")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            })
            .unwrap_or_else(|| "quire-dev-jwt-secret-change-in-production".into());
        let jwt_secret = SecretString::from(jwt_secret_raw);
        let jwt_ttl_secs = env::var("JWT_TTL_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(604_800)
            .clamp(300, 31_536_000);
        let admin_username = env::var("ADMIN_USERNAME")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "admin".into());
        let admin_email = env::var("ADMIN_EMAIL")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty());
        let admin_password = SecretString::from(
            env::var("ADMIN_PASSWORD")
                .ok()
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "admin123456".into()),
        );
        let model_api_key = env::var("MODEL_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(SecretString::from);
        let model_provider =
            env::var("RUNTIME_MODEL_PROVIDER").unwrap_or_else(|_| "mock".to_string());
        let model_base_url = env::var("MODEL_BASE_URL").ok();
        let models_dev_url = env::var("MODELS_DEV_URL")
            .unwrap_or_else(|_| "https://models.dev/api.json".to_string());
        let workspace_dir = env::var("QUIRE_RUNTIME_WORKSPACE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        let default_model = env::var("RUNTIME_MODEL").unwrap_or_else(|_| "stub/echo".to_string());
        let sse_replay_limit = env::var("RUNTIME_SSE_REPLAY_LIMIT")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(500)
            .clamp(1, 10_000);
        let sse_heartbeat_ms = env::var("RUNTIME_SSE_HEARTBEAT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(15_000)
            .clamp(50, 60_000);
        let preview_throttle_ms = env::var("RUNTIME_PREVIEW_THROTTLE_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(65)
            .clamp(50, 80);
        let preview_base_url =
            env::var("PREVIEW_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:5173".into());
        let preview_public_url =
            env::var("PREVIEW_PUBLIC_URL").unwrap_or_else(|_| "http://localhost:5173".into());
        let playwright_mcp_url = env::var("PLAYWRIGHT_MCP_URL")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let sandbox = SandboxConfig::from_env();
        let oss = OssConfig::from_env();
        let evaluator = EvaluatorConfig::from_env();
        Self {
            database_url,
            redis_url,
            bind_addr,
            quire_base_url,
            jwt_secret,
            jwt_ttl_secs,
            admin_username,
            admin_email,
            admin_password,
            model_api_key,
            model_provider,
            model_base_url,
            master_key,
            models_dev_url,
            workspace_dir,
            default_model,
            sse_replay_limit,
            sse_heartbeat_ms,
            preview_throttle_ms,
            preview_base_url,
            preview_public_url,
            playwright_mcp_url,
            sandbox,
            oss,
            evaluator,
        }
    }

    /// Host allowlist for the `/admin/mcp` Streamable HTTP endpoint.
    pub fn mcp_allowed_hosts(&self) -> Vec<String> {
        env::var("RUNTIME_MCP_ALLOWED_HOSTS")
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|host| !host.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|hosts| !hosts.is_empty())
            .unwrap_or_else(|| {
                vec![
                    "localhost".into(),
                    "127.0.0.1".into(),
                    "0.0.0.0".into(),
                    "[::1]".into(),
                ]
            })
    }

    /// Build the shared in-process `quire_server::Config` from this runtime config.
    pub fn to_report_server_config(&self) -> quire_server::Config {
        let bind_addr: std::net::SocketAddr = self
            .bind_addr
            .parse()
            .unwrap_or_else(|_| std::net::SocketAddr::from(([127, 0, 0, 1], 8081)));
        quire_server::Config {
            bind_addr,
            database_url: SecretString::from(self.database_url.clone()),
            redis_url: SecretString::from(self.redis_url.clone()),
            rw_token: None,
            ro_token: None,
            jwt_secret: Some(self.jwt_secret.clone()),
            allowed_hosts: self.mcp_allowed_hosts(),
            cors_allowed_origins: Vec::new(),
            sse_heartbeat: std::time::Duration::from_millis(self.sse_heartbeat_ms),
            auto_migrate: false,
            mcp_json_response: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_secrets() {
        let mut sandbox = SandboxConfig::default();
        sandbox.fc.api_key = Some(SecretString::from("sandbox-secret"));
        let oss = OssConfig {
            access_key_id: Some("oss-access-key".into()),
            access_key_secret: Some(SecretString::from("oss-secret-value")),
            ..OssConfig::default()
        };
        let config = Config {
            database_url: "postgres://user:secret@db/app".into(),
            redis_url: "redis://:secret@cache".into(),
            bind_addr: "127.0.0.1:8081".into(),
            quire_base_url: "http://localhost:8080".into(),
            jwt_secret: SecretString::from("jwt-secret-value"),
            jwt_ttl_secs: 604_800,
            admin_username: "admin".into(),
            admin_email: None,
            admin_password: SecretString::from("admin-secret-value"),
            model_api_key: Some(SecretString::from("model-secret")),
            model_provider: "mock".into(),
            model_base_url: None,
            master_key: Some(SecretString::from("master-secret")),
            models_dev_url: "https://models.dev/api.json".into(),
            workspace_dir: PathBuf::from("."),
            default_model: "stub/echo".into(),
            sse_replay_limit: 500,
            sse_heartbeat_ms: 15_000,
            preview_throttle_ms: 65,
            preview_base_url: "http://127.0.0.1:5173".into(),
            preview_public_url: "http://localhost:5173".into(),
            playwright_mcp_url: None,
            sandbox,
            oss,
            evaluator: EvaluatorConfig {
                api_key: Some(SecretString::from("jev-secret-value")),
                ..EvaluatorConfig::default()
            },
        };
        let rendered = format!("{config:?}");
        assert!(rendered.contains("[REDACTED]"));
        assert!(!rendered.contains("jwt-secret-value"));
        assert!(!rendered.contains("admin-secret-value"));
        assert!(!rendered.contains("model-secret"));
        assert!(!rendered.contains("master-secret"));
        assert!(!rendered.contains("sandbox-secret"));
        assert!(!rendered.contains("oss-access-key"));
        assert!(!rendered.contains("oss-secret-value"));
        assert!(!rendered.contains("jev-secret-value"));
    }
}
