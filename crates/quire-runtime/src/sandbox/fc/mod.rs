//! Alibaba Cloud FC Agent Sandbox through the E2B-compatible platform/envd APIs.

use std::{
    collections::BTreeMap,
    fmt::{Debug, Formatter},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures::StreamExt;
use reqwest::{Response, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(test)]
use serde_json::json;

use super::{
    FcSandboxConfig, Sandbox, SandboxError, SandboxExecRequest, SandboxExecResult, SandboxFuture,
    SandboxProvider, SandboxSpec, sandbox_operation_timeout,
};

const ENVD_PORT: u16 = 49_983;
const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_CONNECT_FRAME_BYTES: usize = 16 * 1024 * 1024;

pub struct FcSandboxProvider {
    config: FcSandboxConfig,
    http: reqwest::Client,
}

impl FcSandboxProvider {
    pub fn new(config: FcSandboxConfig) -> Result<Self, SandboxError> {
        let api_key = config.api_key.as_ref().ok_or_else(|| {
            SandboxError::InvalidInput(
                "SANDBOX_FC_API_KEY is required when SANDBOX_PROVIDER=fc".into(),
            )
        })?;
        if api_key.expose_secret().trim().is_empty() {
            return Err(SandboxError::InvalidInput(
                "SANDBOX_FC_API_KEY cannot be empty".into(),
            ));
        }
        reqwest::Url::parse(&config.api_url)
            .map_err(|_| SandboxError::InvalidInput("SANDBOX_FC_API_URL is invalid".into()))?;
        if let Some(url) = &config.sandbox_url {
            reqwest::Url::parse(url).map_err(|_| {
                SandboxError::InvalidInput("SANDBOX_FC_SANDBOX_URL is invalid".into())
            })?;
        }
        if config.domain.trim().is_empty() {
            return Err(SandboxError::InvalidInput(
                "SANDBOX_FC_DOMAIN cannot be empty".into(),
            ));
        }
        if config.template.trim().is_empty() {
            return Err(SandboxError::InvalidInput(
                "SANDBOX_FC_TEMPLATE cannot be empty".into(),
            ));
        }
        Ok(Self {
            config,
            http: reqwest::Client::new(),
        })
    }

    fn api_key(&self) -> Result<&SecretString, SandboxError> {
        self.config.api_key.as_ref().ok_or_else(|| {
            SandboxError::InvalidInput("SANDBOX_FC_API_KEY is not configured".into())
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FcCreateResponse {
    #[serde(rename = "sandboxID")]
    sandbox_id: String,
    domain: Option<String>,
    envd_access_token: Option<String>,
}

#[derive(Serialize)]
struct EnvdProcessConfig<'a> {
    cmd: &'a str,
    args: &'a [String],
    envs: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<&'a str>,
}

#[derive(Serialize)]
struct EnvdStartProcessRequest<'a> {
    process: EnvdProcessConfig<'a>,
    stdin: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FcCreateSandboxRequest {
    #[serde(rename = "templateID")]
    template_id: String,
    timeout: u64,
    auto_pause: bool,
    secure: bool,
    metadata: BTreeMap<String, String>,
    env_vars: BTreeMap<String, String>,
    allow_internet_access: bool,
}

pub struct FcSandbox {
    id: String,
    api_key: SecretString,
    api_url: String,
    envd_url: String,
    envd_access_token: Option<SecretString>,
    http: reqwest::Client,
    destroyed: AtomicBool,
}

impl Debug for FcSandbox {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FcSandbox")
            .field("id", &self.id)
            .field("envd_url", &self.envd_url)
            .finish_non_exhaustive()
    }
}

impl FcSandbox {
    fn ensure_active(&self) -> Result<(), SandboxError> {
        if self.destroyed.load(Ordering::Acquire) {
            Err(SandboxError::NotFound(self.id.clone()))
        } else {
            Ok(())
        }
    }

    fn envd(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let builder = builder
            .header("E2b-Sandbox-Id", &self.id)
            .header("E2b-Sandbox-Port", ENVD_PORT);
        match &self.envd_access_token {
            Some(token) => builder.header("X-Access-Token", token.expose_secret()),
            None => builder,
        }
    }

    async fn execute(
        &self,
        program: String,
        args: Vec<String>,
        cwd: Option<String>,
        timeout_seconds: u64,
    ) -> Result<SandboxExecResult, SandboxError> {
        self.ensure_active()?;
        let request = encode_connect_frame(&EnvdStartProcessRequest {
            process: EnvdProcessConfig {
                cmd: &program,
                args: &args,
                envs: BTreeMap::new(),
                cwd: cwd.as_deref(),
            },
            stdin: false,
        })?;
        let response = self
            .envd(
                self.http
                    .post(format!("{}/process.Process/Start", self.envd_url)),
            )
            .header("Connect-Protocol-Version", "1")
            .header("Content-Type", "application/connect+json")
            .header(
                "Connect-Timeout-Ms",
                (u128::from(timeout_seconds) * 1_000).to_string(),
            )
            .timeout(Duration::from_secs(timeout_seconds.saturating_add(5)))
            .body(request)
            .send()
            .await
            .map_err(|_| SandboxError::Failed("FC envd process request failed".into()))?;
        let response = expect_success(response, "FC envd process request").await?;
        collect_process_output(response).await
    }
}

impl SandboxProvider for FcSandboxProvider {
    fn create_sandbox(&self, spec: SandboxSpec) -> SandboxFuture<'_, Arc<dyn Sandbox>> {
        Box::pin(async move {
            let template = spec
                .template
                .unwrap_or_else(|| self.config.template.clone());
            let response = self
                .http
                .post(format!(
                    "{}/sandboxes",
                    self.config.api_url.trim_end_matches('/')
                ))
                .header("X-API-Key", self.api_key()?.expose_secret())
                .json(&FcCreateSandboxRequest {
                    template_id: template,
                    timeout: spec.timeout_seconds.clamp(60, 86_400),
                    auto_pause: false,
                    secure: true,
                    metadata: spec.metadata,
                    env_vars: BTreeMap::new(),
                    allow_internet_access: true,
                })
                .timeout(Duration::from_secs(60))
                .send()
                .await
                .map_err(|_| SandboxError::Failed("FC create sandbox request failed".into()))?;
            let response = expect_success(response, "FC create sandbox").await?;
            let created = response.json::<FcCreateResponse>().await.map_err(|_| {
                SandboxError::Failed("FC create sandbox returned invalid JSON".into())
            })?;
            if created.sandbox_id.trim().is_empty() {
                return Err(SandboxError::Failed(
                    "FC create sandbox returned an empty sandbox ID".into(),
                ));
            }
            let domain = created.domain.unwrap_or_else(|| self.config.domain.clone());
            let envd_url = self.config.sandbox_url.clone().unwrap_or_else(|| {
                format!("https://{}-{}.{}", ENVD_PORT, created.sandbox_id, domain)
            });
            let sandbox: Arc<dyn Sandbox> = Arc::new(FcSandbox {
                id: created.sandbox_id,
                api_key: self.api_key()?.clone(),
                api_url: self.config.api_url.trim_end_matches('/').to_string(),
                envd_url: envd_url.trim_end_matches('/').to_string(),
                envd_access_token: created.envd_access_token.map(SecretString::from),
                http: self.http.clone(),
                destroyed: AtomicBool::new(false),
            });
            Ok(sandbox)
        })
    }
}

impl Sandbox for FcSandbox {
    fn id(&self) -> &str {
        &self.id
    }

    fn exec_code(&self, request: SandboxExecRequest) -> SandboxFuture<'_, SandboxExecResult> {
        Box::pin(async move {
            let timeout = sandbox_operation_timeout(&request);
            let timeout_seconds = timeout.as_secs();
            let language = request.language.trim().to_ascii_lowercase();
            let code = request.code;
            let (program, args) = match language.as_str() {
                "python" | "python3" => ("python3".to_string(), vec!["-c".into(), code]),
                "javascript" | "node" | "nodejs" => ("node".to_string(), vec!["-e".into(), code]),
                "bash" | "sh" | "shell" => (
                    "/bin/bash".to_string(),
                    vec!["-l".into(), "-c".into(), code],
                ),
                _ => {
                    return Err(SandboxError::Unsupported(format!(
                        "language `{language}`; expected python, javascript or bash"
                    )));
                }
            };
            self.execute(program, args, request.cwd, timeout_seconds)
                .await
        })
    }

    fn upload_file<'a>(&'a self, path: &'a str, contents: &'a [u8]) -> SandboxFuture<'a, ()> {
        Box::pin(async move {
            self.ensure_active()?;
            if path.trim().is_empty() {
                return Err(SandboxError::InvalidInput(
                    "sandbox file path cannot be empty".into(),
                ));
            }
            if contents.len() > MAX_FILE_BYTES {
                return Err(SandboxError::InvalidInput(format!(
                    "sandbox file exceeds the {MAX_FILE_BYTES}-byte limit"
                )));
            }
            let part =
                reqwest::multipart::Part::bytes(contents.to_vec()).file_name(path.to_string());
            let form = reqwest::multipart::Form::new().part("file", part);
            let response = self
                .envd(self.http.post(format!("{}/files", self.envd_url)))
                .query(&[("path", path)])
                .multipart(form)
                .timeout(Duration::from_secs(60))
                .send()
                .await
                .map_err(|_| SandboxError::Failed("FC file upload failed".into()))?;
            expect_success(response, "FC file upload").await?;
            Ok(())
        })
    }

    fn download_file<'a>(&'a self, path: &'a str) -> SandboxFuture<'a, Vec<u8>> {
        Box::pin(async move {
            self.ensure_active()?;
            let response = self
                .envd(self.http.get(format!("{}/files", self.envd_url)))
                .query(&[("path", path)])
                .timeout(Duration::from_secs(60))
                .send()
                .await
                .map_err(|_| SandboxError::Failed("FC file download failed".into()))?;
            if response.status() == StatusCode::NOT_FOUND {
                return Err(SandboxError::NotFound(format!("{}:{path}", self.id)));
            }
            let response = expect_success(response, "FC file download").await?;
            read_bounded(response, MAX_FILE_BYTES).await
        })
    }

    fn destroy(&self) -> SandboxFuture<'_, ()> {
        Box::pin(async move {
            if self.destroyed.swap(true, Ordering::AcqRel) {
                return Ok(());
            }
            let result = async {
                let response = self
                    .http
                    .delete(format!("{}/sandboxes/{}", self.api_url, self.id))
                    .header("X-API-Key", self.api_key.expose_secret())
                    .timeout(Duration::from_secs(15))
                    .send()
                    .await
                    .map_err(|_| {
                        SandboxError::Failed("FC destroy sandbox request failed".into())
                    })?;
                if response.status() == StatusCode::NOT_FOUND {
                    return Ok(());
                }
                expect_success(response, "FC destroy sandbox").await?;
                Ok(())
            }
            .await;
            if result.is_err() {
                self.destroyed.store(false, Ordering::Release);
            }
            result
        })
    }
}

async fn expect_success(response: Response, operation: &str) -> Result<Response, SandboxError> {
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(SandboxError::Failed(format!(
            "{operation} returned HTTP {}",
            response.status()
        )))
    }
}

async fn read_bounded(response: Response, limit: usize) -> Result<Vec<u8>, SandboxError> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(SandboxError::InvalidInput(format!(
            "sandbox file exceeds the {limit}-byte limit"
        )));
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|_| SandboxError::Failed("failed to read FC response body".into()))?;
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(SandboxError::InvalidInput(format!(
                "sandbox file exceeds the {limit}-byte limit"
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConnectFrame {
    Message(Value),
    EndStream(Value),
}

mod protocol;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use protocol::ConnectFrameDecoder;
pub(crate) use protocol::{collect_process_output, encode_connect_frame};
