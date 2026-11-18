//! Sandbox abstraction reserved for ACK Kata / E2B providers.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxSpec {
    pub image: Option<String>,
    pub cpu_millis: u32,
    pub memory_mb: u32,
}

impl Default for SandboxSpec {
    fn default() -> Self {
        Self {
            image: None,
            cpu_millis: 500,
            memory_mb: 512,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxExecResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("sandbox `{0}` was not found")]
    NotFound(String),
    #[error("sandbox operation failed: {0}")]
    Failed(String),
}

#[async_trait]
pub trait SandboxProvider: Send + Sync {
    async fn create_sandbox(&self, spec: SandboxSpec) -> Result<String, SandboxError>;
    async fn exec_code(
        &self,
        sandbox_id: &str,
        language: &str,
        code: &str,
    ) -> Result<SandboxExecResult, SandboxError>;
    async fn upload_file(
        &self,
        sandbox_id: &str,
        path: &str,
        contents: &[u8],
    ) -> Result<(), SandboxError>;
    async fn download_file(&self, sandbox_id: &str, path: &str) -> Result<Vec<u8>, SandboxError>;
    async fn destroy(&self, sandbox_id: &str) -> Result<(), SandboxError>;
}

#[derive(Clone, Default)]
pub struct LocalSandboxProvider {
    inner: Arc<Mutex<HashMap<String, LocalSandbox>>>,
}

#[derive(Default)]
struct LocalSandbox {
    files: HashMap<String, Vec<u8>>,
}

#[async_trait]
impl SandboxProvider for LocalSandboxProvider {
    async fn create_sandbox(&self, _spec: SandboxSpec) -> Result<String, SandboxError> {
        let id = format!("local_{}", uuid::Uuid::new_v4().simple());
        self.inner
            .lock()
            .await
            .insert(id.clone(), LocalSandbox::default());
        Ok(id)
    }

    async fn exec_code(
        &self,
        sandbox_id: &str,
        language: &str,
        code: &str,
    ) -> Result<SandboxExecResult, SandboxError> {
        if !self.inner.lock().await.contains_key(sandbox_id) {
            return Err(SandboxError::NotFound(sandbox_id.to_string()));
        }
        Ok(SandboxExecResult {
            exit_code: 0,
            stdout: format!(
                "local sandbox accepted {language} program ({} chars); execution is stubbed",
                code.chars().count()
            ),
            stderr: String::new(),
        })
    }

    async fn upload_file(
        &self,
        sandbox_id: &str,
        path: &str,
        contents: &[u8],
    ) -> Result<(), SandboxError> {
        let mut inner = self.inner.lock().await;
        let sandbox = inner
            .get_mut(sandbox_id)
            .ok_or_else(|| SandboxError::NotFound(sandbox_id.to_string()))?;
        sandbox.files.insert(path.to_string(), contents.to_vec());
        Ok(())
    }

    async fn download_file(&self, sandbox_id: &str, path: &str) -> Result<Vec<u8>, SandboxError> {
        let inner = self.inner.lock().await;
        let sandbox = inner
            .get(sandbox_id)
            .ok_or_else(|| SandboxError::NotFound(sandbox_id.to_string()))?;
        sandbox
            .files
            .get(path)
            .cloned()
            .ok_or_else(|| SandboxError::Failed(format!("file `{path}` was not uploaded")))
    }

    async fn destroy(&self, sandbox_id: &str) -> Result<(), SandboxError> {
        self.inner
            .lock()
            .await
            .remove(sandbox_id)
            .map(|_| ())
            .ok_or_else(|| SandboxError::NotFound(sandbox_id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_sandbox_roundtrips_files() {
        let provider = LocalSandboxProvider::default();
        let sandbox = provider
            .create_sandbox(SandboxSpec::default())
            .await
            .unwrap();
        provider
            .upload_file(&sandbox, "report.csv", b"region,revenue\nEMEA,10")
            .await
            .unwrap();
        let downloaded = provider
            .download_file(&sandbox, "report.csv")
            .await
            .unwrap();
        assert_eq!(downloaded, b"region,revenue\nEMEA,10");
        let result = provider
            .exec_code(&sandbox, "python", "print('hi')")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        provider.destroy(&sandbox).await.unwrap();
        assert!(
            provider
                .download_file(&sandbox, "report.csv")
                .await
                .is_err()
        );
    }
}
