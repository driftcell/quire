//! Vendor-neutral sandbox ports, environment configuration and run-scoped leases.

use std::{
    collections::{BTreeMap, HashMap},
    env,
    fmt::{Debug, Formatter},
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use quire_protocol::short_id;
use secrecy::SecretString;
use tokio::{runtime::Handle, sync::Mutex};
use tracing::warn;

pub mod fc;

use fc::FcSandboxProvider;

pub const DEFAULT_FC_TEMPLATE: &str = "code-interpreter-v1";
pub const DEFAULT_FC_REGION: &str = "cn-hangzhou";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxSpec {
    pub template: Option<String>,
    pub timeout_seconds: u64,
    pub metadata: BTreeMap<String, String>,
}

impl Default for SandboxSpec {
    fn default() -> Self {
        Self {
            template: None,
            timeout_seconds: 300,
            metadata: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxExecRequest {
    pub language: String,
    pub code: String,
    pub cwd: Option<String>,
    pub timeout_seconds: Option<u64>,
}

impl SandboxExecRequest {
    pub fn new(language: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            language: language.into(),
            code: code.into(),
            cwd: None,
            timeout_seconds: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxExecResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("sandbox `{0}` was not found")]
    NotFound(String),
    #[error("sandbox operation is not supported: {0}")]
    Unsupported(String),
    #[error("invalid sandbox configuration or input: {0}")]
    InvalidInput(String),
    #[error("sandbox operation failed: {0}")]
    Failed(String),
}

#[async_trait]
pub trait Sandbox: Send + Sync {
    fn id(&self) -> &str;

    async fn exec_code(
        &self,
        request: SandboxExecRequest,
    ) -> Result<SandboxExecResult, SandboxError>;

    async fn upload_file(&self, path: &str, contents: &[u8]) -> Result<(), SandboxError>;

    async fn download_file(&self, path: &str) -> Result<Vec<u8>, SandboxError>;

    async fn destroy(&self) -> Result<(), SandboxError>;
}

#[async_trait]
pub trait SandboxProvider: Send + Sync {
    async fn create_sandbox(&self, spec: SandboxSpec) -> Result<Arc<dyn Sandbox>, SandboxError>;
}

#[derive(Clone)]
pub struct FcSandboxConfig {
    pub api_key: Option<SecretString>,
    pub api_url: String,
    pub domain: String,
    pub sandbox_url: Option<String>,
    pub template: String,
}

impl Debug for FcSandboxConfig {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FcSandboxConfig")
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .field("api_url", &self.api_url)
            .field("domain", &self.domain)
            .field("sandbox_url", &self.sandbox_url)
            .field("template", &self.template)
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct SandboxConfig {
    pub provider: String,
    pub timeout_seconds: u64,
    pub fc: FcSandboxConfig,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            provider: "disabled".into(),
            timeout_seconds: 300,
            fc: FcSandboxConfig {
                api_key: None,
                api_url: format!("https://api.{DEFAULT_FC_REGION}.e2b.fc.aliyuncs.com"),
                domain: format!("{DEFAULT_FC_REGION}.e2b.fc.aliyuncs.com"),
                sandbox_url: None,
                template: DEFAULT_FC_TEMPLATE.into(),
            },
        }
    }
}

impl SandboxConfig {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        let region = env::var("SANDBOX_FC_REGION")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_FC_REGION.into());
        let api_url = env::var("SANDBOX_FC_API_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("https://api.{region}.e2b.fc.aliyuncs.com"));
        let domain = env::var("SANDBOX_FC_DOMAIN")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("{region}.e2b.fc.aliyuncs.com"));
        Self {
            provider: env::var("SANDBOX_PROVIDER").unwrap_or(defaults.provider),
            timeout_seconds: env::var("SANDBOX_TIMEOUT_SECONDS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(defaults.timeout_seconds)
                .clamp(60, 86_400),
            fc: FcSandboxConfig {
                api_key: env::var("SANDBOX_FC_API_KEY")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .map(SecretString::from),
                api_url,
                domain,
                sandbox_url: env::var("SANDBOX_FC_SANDBOX_URL")
                    .ok()
                    .filter(|value| !value.trim().is_empty()),
                template: env::var("SANDBOX_FC_TEMPLATE")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or(defaults.fc.template),
            },
        }
    }
}

pub fn build_sandbox_provider(
    config: &SandboxConfig,
) -> Result<Arc<dyn SandboxProvider>, SandboxError> {
    match config.provider.trim().to_ascii_lowercase().as_str() {
        "disabled" | "none" | "" => Ok(Arc::new(DisabledSandboxProvider)),
        "local" => Ok(Arc::new(LocalSandboxProvider)),
        "fc" | "aliyun-fc" => Ok(Arc::new(FcSandboxProvider::new(config.fc.clone())?)),
        provider => Err(SandboxError::InvalidInput(format!(
            "unknown SANDBOX_PROVIDER `{provider}`; expected disabled, local or fc"
        ))),
    }
}

pub struct DisabledSandboxProvider;

#[async_trait]
impl SandboxProvider for DisabledSandboxProvider {
    async fn create_sandbox(&self, _spec: SandboxSpec) -> Result<Arc<dyn Sandbox>, SandboxError> {
        Err(SandboxError::Unsupported(
            "sandbox support is disabled; set SANDBOX_PROVIDER=fc with SANDBOX_FC_API_KEY, or use SANDBOX_PROVIDER=local for the explicit test stub".into(),
        ))
    }
}

/// A run-scoped lazy sandbox. Dropping the lease schedules best-effort remote cleanup.
pub struct SandboxLease {
    provider: Arc<dyn SandboxProvider>,
    spec: SandboxSpec,
    operation: Mutex<()>,
    current: StdMutex<Option<Arc<dyn Sandbox>>>,
}

impl SandboxLease {
    pub fn new(provider: Arc<dyn SandboxProvider>, spec: SandboxSpec) -> Self {
        Self {
            provider,
            spec,
            operation: Mutex::new(()),
            current: StdMutex::new(None),
        }
    }

    pub async fn get_or_create(&self) -> Result<Arc<dyn Sandbox>, SandboxError> {
        let _operation = self.operation.lock().await;
        if let Some(sandbox) = self.current()? {
            return Ok(sandbox);
        }
        let sandbox = self.provider.create_sandbox(self.spec.clone()).await?;
        let mut current = self
            .current
            .lock()
            .map_err(|_| SandboxError::Failed("sandbox lease lock was poisoned".into()))?;
        *current = Some(Arc::clone(&sandbox));
        Ok(sandbox)
    }

    pub async fn close(&self) -> Result<(), SandboxError> {
        let sandbox = self.take_current()?;
        match sandbox {
            Some(sandbox) => sandbox.destroy().await,
            None => Ok(()),
        }
    }

    fn current(&self) -> Result<Option<Arc<dyn Sandbox>>, SandboxError> {
        self.current
            .lock()
            .map(|current| current.clone())
            .map_err(|_| SandboxError::Failed("sandbox lease lock was poisoned".into()))
    }

    fn take_current(&self) -> Result<Option<Arc<dyn Sandbox>>, SandboxError> {
        self.current
            .lock()
            .map(|mut current| current.take())
            .map_err(|_| SandboxError::Failed("sandbox lease lock was poisoned".into()))
    }
}

impl Drop for SandboxLease {
    fn drop(&mut self) {
        let Ok(sandbox) = self.take_current() else {
            return;
        };
        let Some(sandbox) = sandbox else {
            return;
        };
        if let Ok(runtime) = Handle::try_current() {
            runtime.spawn(async move {
                if let Err(error) = sandbox.destroy().await {
                    warn!(%error, "best-effort sandbox cleanup failed; provider TTL remains the backstop");
                }
            });
        }
    }
}

#[derive(Clone, Default)]
pub struct LocalSandboxProvider;

struct LocalSandbox {
    id: String,
    files: Mutex<HashMap<String, Vec<u8>>>,
    destroyed: AtomicBool,
}

impl LocalSandbox {
    fn ensure_active(&self) -> Result<(), SandboxError> {
        if self.destroyed.load(Ordering::Acquire) {
            Err(SandboxError::NotFound(self.id.clone()))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl SandboxProvider for LocalSandboxProvider {
    async fn create_sandbox(&self, _spec: SandboxSpec) -> Result<Arc<dyn Sandbox>, SandboxError> {
        Ok(Arc::new(LocalSandbox {
            id: short_id("local"),
            files: Mutex::new(HashMap::new()),
            destroyed: AtomicBool::new(false),
        }))
    }
}

#[async_trait]
impl Sandbox for LocalSandbox {
    fn id(&self) -> &str {
        &self.id
    }

    async fn exec_code(
        &self,
        request: SandboxExecRequest,
    ) -> Result<SandboxExecResult, SandboxError> {
        self.ensure_active()?;
        Ok(SandboxExecResult {
            exit_code: 0,
            stdout: format!(
                "local sandbox accepted {} program ({} chars); execution is stubbed",
                request.language,
                request.code.chars().count()
            ),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        })
    }

    async fn upload_file(&self, path: &str, contents: &[u8]) -> Result<(), SandboxError> {
        self.ensure_active()?;
        self.files
            .lock()
            .await
            .insert(path.to_string(), contents.to_vec());
        Ok(())
    }

    async fn download_file(&self, path: &str) -> Result<Vec<u8>, SandboxError> {
        self.ensure_active()?;
        self.files
            .lock()
            .await
            .get(path)
            .cloned()
            .ok_or_else(|| SandboxError::Failed(format!("file `{path}` was not uploaded")))
    }

    async fn destroy(&self) -> Result<(), SandboxError> {
        self.destroyed.store(true, Ordering::Release);
        self.files.lock().await.clear();
        Ok(())
    }
}

pub fn sandbox_spec_from_config(config: &SandboxConfig) -> SandboxSpec {
    SandboxSpec {
        timeout_seconds: config.timeout_seconds,
        ..SandboxSpec::default()
    }
}

pub fn sandbox_operation_timeout(request: &SandboxExecRequest) -> Duration {
    Duration::from_secs(request.timeout_seconds.unwrap_or(60).clamp(1, 3_600))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_sandbox_roundtrips_files() {
        let provider = LocalSandboxProvider;
        let sandbox = provider
            .create_sandbox(SandboxSpec::default())
            .await
            .unwrap();
        sandbox
            .upload_file("report.csv", b"region,revenue\nEMEA,10")
            .await
            .unwrap();
        let downloaded = sandbox.download_file("report.csv").await.unwrap();
        assert_eq!(downloaded, b"region,revenue\nEMEA,10");
        let result = sandbox
            .exec_code(SandboxExecRequest::new("python", "print('hi')"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        sandbox.destroy().await.unwrap();
        assert!(sandbox.download_file("report.csv").await.is_err());
    }

    #[tokio::test]
    async fn lease_is_lazy_and_reuses_one_instance() {
        let lease = SandboxLease::new(Arc::new(LocalSandboxProvider), SandboxSpec::default());
        let first = lease.get_or_create().await.unwrap();
        let second = lease.get_or_create().await.unwrap();
        assert_eq!(first.id(), second.id());
        first.upload_file("state.txt", b"kept").await.unwrap();
        assert_eq!(second.download_file("state.txt").await.unwrap(), b"kept");
        lease.close().await.unwrap();
        assert!(first.download_file("state.txt").await.is_err());
    }

    #[test]
    fn disabled_provider_is_the_default() {
        let config = SandboxConfig::default();
        assert_eq!(config.provider, "disabled");
        let rendered = format!("{config:?}");
        assert!(rendered.contains("disabled"));
    }

    #[test]
    fn fc_provider_requires_an_api_key() {
        let config = SandboxConfig {
            provider: "fc".into(),
            ..SandboxConfig::default()
        };
        let error = match build_sandbox_provider(&config) {
            Ok(_) => panic!("FC provider should require an API key"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("SANDBOX_FC_API_KEY"));
    }

    #[test]
    fn unknown_provider_fails_closed() {
        let config = SandboxConfig {
            provider: "host-shell".into(),
            ..SandboxConfig::default()
        };
        assert!(build_sandbox_provider(&config).is_err());
    }
}
