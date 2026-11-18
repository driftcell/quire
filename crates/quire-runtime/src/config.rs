//! Runtime configuration. Secret values stay inside `SecretString`.

use std::{
    env,
    fmt::{Debug, Formatter},
    path::PathBuf,
};

use secrecy::SecretString;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub redis_url: String,
    pub bind_addr: String,
    pub quire_base_url: String,
    pub quire_rw_token: SecretString,
    pub quire_ro_token: SecretString,
    pub model_api_key: Option<SecretString>,
    pub workspace_dir: PathBuf,
    pub default_model: String,
    pub sse_replay_limit: usize,
    pub preview_throttle_ms: u64,
}

impl Debug for Config {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Config")
            .field("database_url", &"[REDACTED]")
            .field("redis_url", &"[REDACTED]")
            .field("bind_addr", &self.bind_addr)
            .field("quire_base_url", &self.quire_base_url)
            .field("quire_rw_token", &"[REDACTED]")
            .field("quire_ro_token", &"[REDACTED]")
            .field(
                "model_api_key",
                &self.model_api_key.as_ref().map(|_| "[REDACTED]"),
            )
            .field("workspace_dir", &self.workspace_dir)
            .field("default_model", &self.default_model)
            .field("sse_replay_limit", &self.sse_replay_limit)
            .field("preview_throttle_ms", &self.preview_throttle_ms)
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
        let quire_rw_token = SecretString::from(env::var("QUIRE_RW_TOKEN").unwrap_or_default());
        let quire_ro_token = SecretString::from(env::var("QUIRE_RO_TOKEN").unwrap_or_default());
        let model_api_key = env::var("MODEL_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(SecretString::from);
        let workspace_dir = env::var("QUIRE_RUNTIME_WORKSPACE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."));
        let default_model = env::var("RUNTIME_MODEL").unwrap_or_else(|_| "stub/echo".to_string());
        let sse_replay_limit = env::var("RUNTIME_SSE_REPLAY_LIMIT")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(500)
            .clamp(1, 10_000);
        let preview_throttle_ms = env::var("RUNTIME_PREVIEW_THROTTLE_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(65)
            .clamp(50, 80);
        Self {
            database_url,
            redis_url,
            bind_addr,
            quire_base_url,
            quire_rw_token,
            quire_ro_token,
            model_api_key,
            workspace_dir,
            default_model,
            sse_replay_limit,
            preview_throttle_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_secrets() {
        let config = Config {
            database_url: "postgres://user:secret@db/app".into(),
            redis_url: "redis://:secret@cache".into(),
            bind_addr: "127.0.0.1:8081".into(),
            quire_base_url: "http://localhost:8080".into(),
            quire_rw_token: SecretString::from("rw-secret"),
            quire_ro_token: SecretString::from("ro-secret"),
            model_api_key: Some(SecretString::from("model-secret")),
            workspace_dir: PathBuf::from("."),
            default_model: "stub/echo".into(),
            sse_replay_limit: 500,
            preview_throttle_ms: 65,
        };
        let rendered = format!("{config:?}");
        assert!(rendered.contains("[REDACTED]"));
        assert!(!rendered.contains("rw-secret"));
        assert!(!rendered.contains("model-secret"));
    }
}
