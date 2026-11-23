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
