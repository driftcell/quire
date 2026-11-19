//! Alibaba Cloud FC Agent Sandbox through the E2B-compatible platform/envd APIs.

use std::{
    fmt::{Debug, Formatter},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use futures::StreamExt;
use reqwest::{Response, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    FcSandboxConfig, Sandbox, SandboxError, SandboxExecRequest, SandboxExecResult, SandboxProvider,
    SandboxSpec, sandbox_operation_timeout,
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
        let mut process = json!({
            "cmd": program,
            "args": args,
            "envs": {},
        });
        if let Some(cwd) = cwd {
            process
                .as_object_mut()
                .ok_or_else(|| SandboxError::Failed("failed to build process request".into()))?
                .insert("cwd".into(), Value::String(cwd));
        }
        let request = encode_connect_frame(&json!({
            "process": process,
            "stdin": false,
        }))?;
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

#[async_trait]
impl SandboxProvider for FcSandboxProvider {
    async fn create_sandbox(&self, spec: SandboxSpec) -> Result<Arc<dyn Sandbox>, SandboxError> {
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
            .json(&json!({
                "templateID": template,
                "timeout": spec.timeout_seconds.clamp(60, 86_400),
                "autoPause": false,
                "secure": true,
                "metadata": spec.metadata,
                "envVars": {},
                "allowInternetAccess": true,
            }))
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .map_err(|_| SandboxError::Failed("FC create sandbox request failed".into()))?;
        let response = expect_success(response, "FC create sandbox").await?;
        let created = response
            .json::<FcCreateResponse>()
            .await
            .map_err(|_| SandboxError::Failed("FC create sandbox returned invalid JSON".into()))?;
        if created.sandbox_id.trim().is_empty() {
            return Err(SandboxError::Failed(
                "FC create sandbox returned an empty sandbox ID".into(),
            ));
        }
        let domain = created.domain.unwrap_or_else(|| self.config.domain.clone());
        let envd_url =
            self.config.sandbox_url.clone().unwrap_or_else(|| {
                format!("https://{}-{}.{}", ENVD_PORT, created.sandbox_id, domain)
            });
        Ok(Arc::new(FcSandbox {
            id: created.sandbox_id,
            api_key: self.api_key()?.clone(),
            api_url: self.config.api_url.trim_end_matches('/').to_string(),
            envd_url: envd_url.trim_end_matches('/').to_string(),
            envd_access_token: created.envd_access_token.map(SecretString::from),
            http: self.http.clone(),
            destroyed: AtomicBool::new(false),
        }))
    }
}

#[async_trait]
impl Sandbox for FcSandbox {
    fn id(&self) -> &str {
        &self.id
    }

    async fn exec_code(
        &self,
        request: SandboxExecRequest,
    ) -> Result<SandboxExecResult, SandboxError> {
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
    }

    async fn upload_file(&self, path: &str, contents: &[u8]) -> Result<(), SandboxError> {
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
        let part = reqwest::multipart::Part::bytes(contents.to_vec()).file_name(path.to_string());
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
    }

    async fn download_file(&self, path: &str) -> Result<Vec<u8>, SandboxError> {
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
    }

    async fn destroy(&self) -> Result<(), SandboxError> {
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
                .map_err(|_| SandboxError::Failed("FC destroy sandbox request failed".into()))?;
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
enum ConnectFrame {
    Message(Value),
    EndStream(Value),
}

fn encode_connect_frame(message: &Value) -> Result<Vec<u8>, SandboxError> {
    let payload = serde_json::to_vec(message)
        .map_err(|_| SandboxError::Failed("failed to serialize Connect request".into()))?;
    let length = u32::try_from(payload.len())
        .map_err(|_| SandboxError::InvalidInput("Connect request is too large".into()))?;
    let mut frame = Vec::with_capacity(payload.len().saturating_add(5));
    frame.push(0);
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

#[derive(Default)]
struct ConnectFrameDecoder {
    buffer: Vec<u8>,
}

impl ConnectFrameDecoder {
    fn push(&mut self, chunk: &[u8]) {
        self.buffer.extend_from_slice(chunk);
    }

    fn drain(&mut self) -> Result<Vec<ConnectFrame>, SandboxError> {
        let mut frames = Vec::new();
        let mut offset = 0usize;
        while self.buffer.len().saturating_sub(offset) >= 5 {
            let header = self
                .buffer
                .get(offset..offset.saturating_add(5))
                .ok_or_else(|| SandboxError::Failed("truncated Connect header".into()))?;
            let flags = header
                .first()
                .copied()
                .ok_or_else(|| SandboxError::Failed("missing Connect flags".into()))?;
            let length_bytes: [u8; 4] = header
                .get(1..5)
                .ok_or_else(|| SandboxError::Failed("truncated Connect length".into()))?
                .try_into()
                .map_err(|_| SandboxError::Failed("invalid Connect length".into()))?;
            let length = u32::from_be_bytes(length_bytes) as usize;
            if length > MAX_CONNECT_FRAME_BYTES {
                return Err(SandboxError::Failed(
                    "Connect frame exceeded the size limit".into(),
                ));
            }
            let start = offset.saturating_add(5);
            let end = start.saturating_add(length);
            if self.buffer.len() < end {
                break;
            }
            let payload = self
                .buffer
                .get(start..end)
                .ok_or_else(|| SandboxError::Failed("truncated Connect payload".into()))?;
            let value = serde_json::from_slice::<Value>(payload)
                .map_err(|_| SandboxError::Failed("invalid Connect JSON payload".into()))?;
            frames.push(if flags & 0x02 != 0 {
                ConnectFrame::EndStream(value)
            } else {
                ConnectFrame::Message(value)
            });
            offset = end;
        }
        self.buffer.drain(..offset);
        Ok(frames)
    }
}

async fn collect_process_output(response: Response) -> Result<SandboxExecResult, SandboxError> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut stdout_truncated = false;
    let mut stderr_truncated = false;
    let mut exit_code = None;
    let mut decoder = ConnectFrameDecoder::default();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|_| SandboxError::Failed("failed to read FC process stream".into()))?;
        decoder.push(&chunk);
        for frame in decoder.drain()? {
            match frame {
                ConnectFrame::Message(value) => apply_process_event(
                    &value,
                    &mut stdout,
                    &mut stderr,
                    &mut stdout_truncated,
                    &mut stderr_truncated,
                    &mut exit_code,
                )?,
                ConnectFrame::EndStream(value) => {
                    if let Some(error) = connect_stream_error(&value) {
                        return Err(SandboxError::Failed(format!(
                            "FC envd rejected the command: {error}"
                        )));
                    }
                }
            }
        }
    }
    let exit_code = exit_code.ok_or_else(|| {
        SandboxError::Failed("FC process stream ended before the process finished".into())
    })?;
    Ok(SandboxExecResult {
        exit_code,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        stdout_truncated,
        stderr_truncated,
    })
}

fn append_output(
    sink: &mut Vec<u8>,
    encoded: &str,
    truncated: &mut bool,
) -> Result<(), SandboxError> {
    let bytes = BASE64
        .decode(encoded)
        .map_err(|_| SandboxError::Failed("invalid base64 in FC process output".into()))?;
    let remaining = MAX_OUTPUT_BYTES.saturating_sub(sink.len());
    if bytes.len() > remaining {
        *truncated = true;
    }
    sink.extend(bytes.iter().take(remaining));
    Ok(())
}

fn apply_process_event(
    value: &Value,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
    stdout_truncated: &mut bool,
    stderr_truncated: &mut bool,
    exit_code: &mut Option<i32>,
) -> Result<(), SandboxError> {
    let Some(event) = value.get("event") else {
        return Ok(());
    };
    if let Some(data) = event.get("data") {
        if let Some(encoded) = data.get("stdout").and_then(Value::as_str) {
            append_output(stdout, encoded, stdout_truncated)?;
        }
        if let Some(encoded) = data.get("stderr").and_then(Value::as_str) {
            append_output(stderr, encoded, stderr_truncated)?;
        }
        if let Some(encoded) = data.get("pty").and_then(Value::as_str) {
            append_output(stdout, encoded, stdout_truncated)?;
        }
    }
    if let Some(end) = event.get("end") {
        let code = end
            .get("exitCode")
            .and_then(|value| {
                value
                    .as_i64()
                    .or_else(|| value.as_str().and_then(|raw| raw.parse::<i64>().ok()))
            })
            .unwrap_or(0);
        *exit_code = Some(i32::try_from(code).unwrap_or(-1));
    }
    Ok(())
}

fn connect_stream_error(value: &Value) -> Option<String> {
    let error = value.get("error")?;
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("unknown error")
        .chars()
        .take(500)
        .collect::<String>();
    Some(format!("{code}: {message}"))
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Mutex};

    use axum::{
        Json, Router,
        body::Bytes,
        extract::{Path, Query, State},
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::{delete, get, post},
    };

    use super::*;

    #[derive(Default)]
    struct MockState {
        create_body: Mutex<Option<Value>>,
        upload_body: Mutex<Vec<u8>>,
        destroyed: AtomicBool,
    }

    fn authorized(headers: &HeaderMap, name: &str, expected: &str) -> bool {
        headers.get(name).and_then(|value| value.to_str().ok()) == Some(expected)
    }

    async fn create_sandbox(
        State(state): State<Arc<MockState>>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Result<Json<Value>, StatusCode> {
        if !authorized(&headers, "x-api-key", "test-api-key") {
            return Err(StatusCode::UNAUTHORIZED);
        }
        let mut slot = state.create_body.lock().unwrap();
        *slot = Some(body);
        Ok(Json(json!({
            "sandboxID": "sbx-test",
            "templateID": "code-interpreter-v1",
            "envdVersion": "0.5.2",
            "envdAccessToken": "test-envd-token",
            "domain": "cn-hangzhou.e2b.fc.aliyuncs.com"
        })))
    }

    async fn destroy_sandbox(
        State(state): State<Arc<MockState>>,
        Path(id): Path<String>,
        headers: HeaderMap,
    ) -> StatusCode {
        if id != "sbx-test" || !authorized(&headers, "x-api-key", "test-api-key") {
            return StatusCode::UNAUTHORIZED;
        }
        state.destroyed.store(true, Ordering::Release);
        StatusCode::NO_CONTENT
    }

    async fn download(headers: HeaderMap) -> Result<Vec<u8>, StatusCode> {
        if !authorized(&headers, "x-access-token", "test-envd-token") {
            return Err(StatusCode::UNAUTHORIZED);
        }
        Ok(b"sandbox file contents".to_vec())
    }

    async fn upload(
        State(state): State<Arc<MockState>>,
        headers: HeaderMap,
        Query(query): Query<BTreeMap<String, String>>,
        body: Bytes,
    ) -> Result<Json<Value>, StatusCode> {
        if !authorized(&headers, "x-access-token", "test-envd-token")
            || query.get("path").map(String::as_str) != Some("/tmp/report.txt")
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        let mut slot = state.upload_body.lock().unwrap();
        *slot = body.to_vec();
        Ok(Json(
            json!([{ "name": "report.txt", "path": "/tmp/report.txt" }]),
        ))
    }

    async fn process(headers: HeaderMap, body: Bytes) -> Result<impl IntoResponse, StatusCode> {
        if !authorized(&headers, "x-access-token", "test-envd-token") {
            return Err(StatusCode::UNAUTHORIZED);
        }
        let mut decoder = ConnectFrameDecoder::default();
        decoder.push(&body);
        let frames = decoder.drain().map_err(|_| StatusCode::BAD_REQUEST)?;
        if frames.len() != 1 {
            return Err(StatusCode::BAD_REQUEST);
        }
        let events = [
            json!({"event": {"start": {"pid": 42}}}),
            json!({"event": {"data": {"stdout": BASE64.encode("hello from FC\n")}}}),
            json!({"event": {"data": {"stderr": BASE64.encode("warning\n")}}}),
            json!({"event": {"end": {"exitCode": 0}}}),
        ];
        let mut response_body = Vec::new();
        for event in &events {
            response_body.extend_from_slice(&encode_connect_frame(event).unwrap());
        }
        let end_frame_start = response_body.len();
        response_body.extend_from_slice(&encode_connect_frame(&json!({})).unwrap());
        if let Some(flags) = response_body.get_mut(end_frame_start) {
            *flags = 0x02;
        }
        Ok((
            [("content-type", "application/connect+json")],
            response_body,
        ))
    }

    async fn mock_server() -> (String, Arc<MockState>) {
        let state = Arc::new(MockState::default());
        let app = Router::new()
            .route("/sandboxes", post(create_sandbox))
            .route("/sandboxes/{id}", delete(destroy_sandbox))
            .route("/files", get(download).post(upload))
            .route("/process.Process/Start", post(process))
            .with_state(Arc::clone(&state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{address}"), state)
    }

    fn config(base_url: String) -> FcSandboxConfig {
        FcSandboxConfig {
            api_key: Some(SecretString::from("test-api-key")),
            api_url: base_url.clone(),
            domain: "cn-hangzhou.e2b.fc.aliyuncs.com".into(),
            sandbox_url: Some(base_url),
            template: "code-interpreter-v1".into(),
        }
    }

    #[tokio::test]
    async fn fc_provider_covers_platform_envd_and_cleanup() {
        let (base_url, state) = mock_server().await;
        let provider = FcSandboxProvider::new(config(base_url)).unwrap();
        let mut metadata = BTreeMap::new();
        metadata.insert("run_id".into(), "run-test".into());
        let sandbox = provider
            .create_sandbox(SandboxSpec {
                template: None,
                timeout_seconds: 300,
                metadata,
            })
            .await
            .unwrap();
        assert_eq!(sandbox.id(), "sbx-test");
        let create_body = state.create_body.lock().unwrap().clone().unwrap();
        assert_eq!(create_body.get("autoPause"), Some(&json!(false)));
        assert_eq!(create_body.get("secure"), Some(&json!(true)));
        assert_eq!(
            create_body.pointer("/metadata/run_id"),
            Some(&json!("run-test"))
        );

        let result = sandbox
            .exec_code(SandboxExecRequest::new("python", "print('hello')"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hello from FC\n");
        assert_eq!(result.stderr, "warning\n");

        sandbox
            .upload_file("/tmp/report.txt", b"report")
            .await
            .unwrap();
        let upload_body = state.upload_body.lock().unwrap().clone();
        assert!(String::from_utf8_lossy(&upload_body).contains("report"));
        assert_eq!(
            sandbox.download_file("/tmp/report.txt").await.unwrap(),
            b"sandbox file contents"
        );
        sandbox.destroy().await.unwrap();
        assert!(state.destroyed.load(Ordering::Acquire));
    }

    #[test]
    fn connect_decoder_handles_fragmented_frames() {
        let encoded = encode_connect_frame(&json!({"event": {"end": {}}})).unwrap();
        let mut decoder = ConnectFrameDecoder::default();
        for byte in encoded {
            decoder.push(&[byte]);
        }
        let frames = decoder.drain().unwrap();
        assert_eq!(frames.len(), 1);
    }

    #[test]
    fn config_debug_does_not_expose_api_key() {
        let rendered = format!("{:?}", config("http://localhost".into()));
        assert!(!rendered.contains("test-api-key"));
        assert!(rendered.contains("[REDACTED]"));
    }
}
