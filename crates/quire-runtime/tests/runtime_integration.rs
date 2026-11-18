//! Live PostgreSQL/Redis integration tests for the independent runtime.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::{
    path::Path,
    sync::{Arc, OnceLock},
    time::Duration,
};

use base64::Engine as _;
use quire_runtime::{
    Config, KbService, MockStreamProvider, ModelRegistry, ModelStreamEvent, ModelsDevImportRequest,
    ProviderInput, ProviderProtocol, RuntimeService, RuntimeStore, StaticQuireControl,
    ToolCallRecord,
    bus::{InterruptCommand, RedisBus},
    control::RunRegistry,
};
use quire_session_protocol::{ActiveRunStatus, SendMessageRequest, SessionEventKind};
use secrecy::SecretString;
use tokio::sync::Mutex;
use tower::ServiceExt;

fn test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn unique(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

fn database_url() -> String {
    let _ = dotenvy::dotenv();
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://report:report@127.0.0.1:5432/report_core".into())
}

fn redis_url() -> String {
    let _ = dotenvy::dotenv();
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into())
}

async fn test_service() -> (RuntimeService, RuntimeStore) {
    let store = RuntimeStore::connect(&database_url(), &redis_url())
        .await
        .expect("PostgreSQL and Redis are required");
    store.migrate().await.expect("runtime migrations");
    let kb = KbService::new(store.clone());
    let models = ModelRegistry::new(
        store.clone(),
        Some(SecretString::from("test-master-key")),
        "https://models.dev/api.json",
    );
    let config = Arc::new(Config {
        database_url: database_url(),
        redis_url: redis_url(),
        bind_addr: "127.0.0.1:0".into(),
        quire_base_url: "http://127.0.0.1:1".into(),
        quire_rw_token: SecretString::from("test-token"),
        quire_ro_token: SecretString::from("test-token"),
        api_token: SecretString::from("runtime-test-token"),
        model_api_key: None,
        model_provider: "mock".into(),
        model_base_url: None,
        master_key: Some(SecretString::from("test-master-key")),
        models_dev_url: "https://models.dev/api.json".into(),
        workspace_dir: std::env::temp_dir(),
        default_model: "stub/echo".into(),
        sse_replay_limit: 500,
        preview_throttle_ms: 65,
    });
    let service = RuntimeService::new(
        store.clone(),
        kb,
        Arc::new(models),
        config,
        Arc::new(StaticQuireControl),
    );
    (service, store)
}

async fn wait_for_idle(store: &RuntimeStore, session_id: &str) {
    for _ in 0..200 {
        let session = store.get_session_summary(session_id).await.unwrap();
        if session.active_run_status != ActiveRunStatus::Running {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("session {session_id} never became idle");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_recovery_marks_running_runs_interrupted_and_appends_failure() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let session = service
        .create_session(&unique("recovery"), Some("runtime-doc"))
        .await
        .unwrap();
    let run = store
        .create_run(&session.session_id, None, "main", "stub/echo")
        .await
        .unwrap();
    let recovered = service.boot().await.unwrap();
    assert!(recovered >= 1);
    let run = store.get_run(&run.run_id).await.unwrap();
    assert_eq!(run.status, "interrupted");
    let detail = service.session_detail(&session.session_id).await.unwrap();
    assert_eq!(
        detail.summary.active_run_status,
        ActiveRunStatus::Interrupted
    );
    let events = store
        .list_events(&session.session_id, 0, 100)
        .await
        .unwrap();
    assert!(events.iter().any(|event| matches!(
        &event.kind,
        SessionEventKind::TurnFailed { recoverable, .. } if *recoverable
    )));
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_message_id_is_idempotent_and_does_not_duplicate_runs() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let session = service
        .create_session(&unique("idempotent"), Some("runtime-doc"))
        .await
        .unwrap();
    let request = SendMessageRequest {
        client_msg_id: unique("client-msg"),
        content: "Draft an executive summary".into(),
        attachment_ids: Vec::new(),
    };
    let (first, inserted_first, run_id) = service
        .post_message(&session.session_id, &request)
        .await
        .unwrap();
    assert!(inserted_first);
    assert!(run_id.is_some());
    wait_for_idle(&store, &session.session_id).await;
    let idle = service.session_detail(&session.session_id).await.unwrap();
    assert_eq!(idle.summary.context_usage.context_limit, 200_000);
    let (second, inserted_second, run_id_second) = service
        .post_message(&session.session_id, &request)
        .await
        .unwrap();
    assert!(!inserted_second);
    assert_eq!(second.message_id, first.message_id);
    assert!(run_id_second.is_none());
    let messages = store.list_messages(&session.session_id).await.unwrap();
    let user_messages: Vec<_> = messages
        .iter()
        .filter(|message| message.role == quire_session_protocol::MessageRole::User)
        .collect();
    assert_eq!(user_messages.len(), 1);
    let run_id = run_id.expect("run id");
    let tool_call = service
        .record_tool_call(ToolCallRecord {
            session_id: session.session_id.clone(),
            run_id,
            tool_name: "apply_operations".into(),
            input: serde_json::json!({"ops":[]}),
            output: serde_json::json!({"committed":true}),
            status: "completed".into(),
            duration_ms: 12,
        })
        .await
        .unwrap();
    let loaded = service
        .get_tool_call(&session.session_id, &tool_call.tool_call_id)
        .await
        .unwrap();
    assert_eq!(loaded.output["committed"], true);
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_replay_returns_events_after_the_requested_seq() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let session = service
        .create_session(&unique("replay"), Some("runtime-doc"))
        .await
        .unwrap();
    for index in 0..3 {
        store
            .append_event(
                &session.session_id,
                "run_replay",
                "main",
                None,
                SessionEventKind::TextDelta {
                    delta: format!("chunk-{index}"),
                },
            )
            .await
            .unwrap();
    }
    let replay = store
        .list_events(&session.session_id, 1, 100)
        .await
        .unwrap();
    assert_eq!(replay.len(), 2);
    assert_eq!(replay.first().map(|event| event.seq), Some(2));
    assert_eq!(replay.last().map(|event| event.seq), Some(3));
    assert_eq!(
        store
            .count_events_after(&session.session_id, 1)
            .await
            .unwrap(),
        2
    );
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn knowledge_update_keeps_the_pinned_prompt_frozen() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let session = service
        .create_session(&unique("prompt-cache"), Some("runtime-doc"))
        .await
        .unwrap();
    let initial_version = service.kb().current_version().await.unwrap();
    store
        .set_pinned_prompt(&session.session_id, "FROZEN SYSTEM PREFIX", initial_version)
        .await
        .unwrap();
    let bumped = service
        .kb()
        .upsert_document(
            "prompt-cache-test",
            "Prompt cache test",
            "Updated runtime guidance",
        )
        .await
        .unwrap();
    assert!(bumped.version >= 1);
    let context = service
        .resume_context(&session.session_id, "Next request")
        .await
        .unwrap();
    assert_eq!(context.pinned_system_prompt, "FROZEN SYSTEM PREFIX");
    assert!(
        context
            .messages
            .iter()
            .any(|message| message.content.starts_with("<knowledge_update>"))
    );
    let (pinned, _) = store.pinned_prompt(&session.session_id).await.unwrap();
    assert_eq!(pinned.as_deref(), Some("FROZEN SYSTEM PREFIX"));
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn redis_interrupt_reaches_two_independent_runtimes() {
    let _guard = test_lock().lock().await;
    let bus = RedisBus::connect(&redis_url()).unwrap();
    let first = RunRegistry::default();
    let second = RunRegistry::default();
    let token_a = first.register("run_cross_instance").await;
    let token_b = second.register("run_cross_instance").await;
    let _a = quire_runtime::control::spawn_interrupt_listener(bus.clone(), first.clone());
    let _b = quire_runtime::control::spawn_interrupt_listener(bus.clone(), second.clone());
    tokio::time::sleep(Duration::from_millis(100)).await;
    bus.publish_interrupt(&InterruptCommand {
        session_id: "sess_cross".into(),
        run_id: "run_cross_instance".into(),
    })
    .await
    .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline
        && !(token_a.is_cancelled() && token_b.is_cancelled())
    {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(token_a.is_cancelled());
    assert!(token_b.is_cancelled());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn seed_is_idempotent_and_preserves_online_edits() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    for statement in [
        "DELETE FROM skill_resources",
        "DELETE FROM skill_definitions",
        "DELETE FROM agent_definitions",
        "DELETE FROM kb_documents",
    ] {
        sqlx::query(statement).execute(store.pool()).await.unwrap();
    }
    let workspace = tempfile::tempdir().unwrap();
    write_seed_workspace(workspace.path());
    let first = service
        .kb()
        .seed(workspace.path(), "stub/echo")
        .await
        .unwrap();
    assert_eq!(first.documents, 1);
    assert_eq!(first.agents, 1);
    assert_eq!(first.skills, 1);
    assert!(first.kb_version > 1);
    let documents = service.kb().list_documents().await.unwrap();
    assert!(documents.iter().any(|doc| doc.doc_id == "claude-md"));
    let skills = service.kb().list_skills().await.unwrap();
    assert!(
        skills
            .iter()
            .any(|skill| skill.name == "report-review" && !skill.resources.is_empty())
    );
    let tools = quire_runtime::rig_agent::build_builtin_tools(service.kb().clone()).unwrap();
    assert_eq!(tools.len(), 4);
    let describe = tools
        .iter()
        .find(|tool| tool.name().as_str() == "describe_task")
        .unwrap();
    let output = describe
        .run(serde_json::json!({"title":"Review chapter","section_id":"ch1"}))
        .await
        .unwrap();
    assert_eq!(
        output
            .as_json()
            .and_then(|value| value.get("title"))
            .and_then(serde_json::Value::as_str),
        Some("Review chapter")
    );
    let load_skill = tools
        .iter()
        .find(|tool| tool.name().as_str() == "load_skill")
        .unwrap();
    let output = load_skill
        .run(serde_json::json!({"name":"report-review"}))
        .await
        .unwrap();
    assert_eq!(
        output
            .as_json()
            .and_then(|value| value.get("name"))
            .and_then(serde_json::Value::as_str),
        Some("report-review")
    );
    let subagents = tools
        .iter()
        .find(|tool| tool.name().as_str() == "spawn_subagents")
        .unwrap();
    let output = subagents
        .run(serde_json::json!({
            "tasks": [
                {"agent_name":"reviewer-a","task":"Review chapter 1"},
                {"agent_name":"reviewer-b","task":"Review chapter 2"}
            ]
        }))
        .await
        .unwrap();
    assert_eq!(
        output
            .as_json()
            .and_then(|value| value.get("outcomes"))
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(2)
    );
    service
        .kb()
        .upsert_document("claude-md", "CLAUDE.md", "ONLINE EDIT")
        .await
        .unwrap();
    let second = service.seed().await.unwrap();
    assert_eq!(second.documents, 0);
    let documents = service.kb().list_documents().await.unwrap();
    let claude = documents
        .iter()
        .find(|doc| doc.doc_id == "claude-md")
        .unwrap();
    assert_eq!(claude.content, "ONLINE EDIT");
    for statement in [
        "DELETE FROM skill_resources",
        "DELETE FROM skill_definitions",
        "DELETE FROM agent_definitions",
        "DELETE FROM kb_documents",
    ] {
        sqlx::query(statement).execute(store.pool()).await.unwrap();
    }
}

fn write_seed_workspace(root: &Path) {
    std::fs::write(root.join("CLAUDE.md"), "Workspace rules from CLAUDE.md").unwrap();
    let agents = root.join("agents");
    std::fs::create_dir_all(&agents).unwrap();
    std::fs::write(
        agents.join("reviewer.md"),
        "---\nname: reviewer\nmodel: stub/reviewer\nallowed_tools: [lint_document, read_blocks]\n---\nReview every chapter.",
    )
    .unwrap();
    let skill = root.join("skills").join("report-review");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: report-review\ndescription: Review report structure and citations\n---\nUse the review checklist.",
    )
    .unwrap();
    std::fs::write(
        skill.join("checklist.md"),
        "- Check citations\n- Check tables",
    )
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attachments_reject_traversal_and_decode_base64() {
    let _guard = test_lock().lock().await;
    let (service, _store) = test_service().await;
    let session = service
        .create_session(&unique("attachments"), Some("runtime-doc"))
        .await
        .unwrap();
    let traversal = service
        .create_attachment(&session.session_id, "../escape.txt", "text/plain", 5, None)
        .await;
    assert!(traversal.is_err());
    let invalid = service
        .create_attachment(
            &session.session_id,
            "note.txt",
            "text/plain",
            3,
            Some("not-base64!!!"),
        )
        .await;
    assert!(invalid.is_err());

    let filename = format!("{}.txt", unique("attachment"));
    let encoded = base64::engine::general_purpose::STANDARD.encode(b"hello attachment");
    let meta = service
        .create_attachment(
            &session.session_id,
            &filename,
            "text/plain",
            16,
            Some(&encoded),
        )
        .await
        .unwrap();
    assert_eq!(meta.filename, filename);
    let attachments_dir = std::env::temp_dir()
        .join(".quire-runtime")
        .join("attachments");
    let stored = std::fs::read_dir(&attachments_dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(&filename))
        })
        .expect("attachment file");
    assert_eq!(std::fs::read(&stored).unwrap(), b"hello attachment");
    let _ = std::fs::remove_file(stored);
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_bearer_auth_protects_api_routes() {
    let _guard = test_lock().lock().await;
    let (service, _store) = test_service().await;
    let provider_id = unique("auth-provider");
    service
        .models()
        .upsert_provider(ProviderInput {
            provider_id: provider_id.clone(),
            name: "Auth Provider".into(),
            protocol: ProviderProtocol::OpenaiCompatible,
            base_url: "https://example.invalid".into(),
            api_key_ref: "sk-plaintext-must-not-leak".into(),
            enabled: true,
        })
        .await
        .unwrap();
    let cleanup_service = service.clone();
    let app = quire_runtime::http::router(service);
    let unauthorized = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/sessions")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), axum::http::StatusCode::UNAUTHORIZED);
    let health = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/health")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(health.status(), axum::http::StatusCode::OK);
    let wrong_token = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/sessions")
                .header("authorization", "Bearer wrong-token")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wrong_token.status(), axum::http::StatusCode::UNAUTHORIZED);
    let authorized = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/sessions")
                .header("authorization", "Bearer runtime-test-token")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(authorized.status(), axum::http::StatusCode::OK);
    let providers = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/models/providers")
                .header("authorization", "Bearer runtime-test-token")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(providers.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(providers.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(!text.contains("sk-plaintext-must-not-leak"));
    assert!(text.contains("enc:***"));
    cleanup_service
        .models()
        .delete_provider(&provider_id)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn models_registry_imports_catalog_and_routes_context_limit() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let provider_id = unique("provider");
    let provider = service
        .models()
        .upsert_provider(ProviderInput {
            provider_id: provider_id.clone(),
            name: "DeepSeek".into(),
            protocol: ProviderProtocol::OpenaiCompatible,
            base_url: "https://api.deepseek.com".into(),
            api_key_ref: "sk-raw-secret-value".into(),
            enabled: true,
        })
        .await
        .unwrap();
    assert!(provider.api_key_masked.starts_with("enc:"));
    assert!(!provider.api_key_masked.contains("sk-raw-secret-value"));

    let catalog = serde_json::json!({
        "deepseek": {
            "id": "deepseek",
            "name": "DeepSeek",
            "models": {
                "deepseek-chat": {
                    "id": "deepseek-chat",
                    "name": "DeepSeek Chat",
                    "reasoning": true,
                    "tool_call": true,
                    "attachment": false,
                    "limit": {"context": 64000, "output": 4096},
                    "cost": {"input": 0.14, "output": 0.28, "cache_read": 0.014}
                },
                "deepseek-coder": {
                    "id": "deepseek-coder",
                    "name": "DeepSeek Coder",
                    "tool_call": true
                }
            }
        }
    });
    let imported = service
        .models()
        .import_models_dev(ModelsDevImportRequest {
            provider_id: provider_id.clone(),
            provider_filter: Some("deepseek".into()),
            model_ids: None,
            payload: Some(catalog),
            url: None,
        })
        .await
        .unwrap();
    assert_eq!(imported.imported, 2);
    let route = imported
        .models
        .iter()
        .find(|model| model.model_id == "deepseek-chat")
        .unwrap();
    assert_eq!(route.context_limit, 64000);
    assert_eq!(route.max_output_tokens, Some(4096));
    assert_eq!(route.cost_cache_read, Some(0.014));
    let fallback = imported
        .models
        .iter()
        .find(|model| model.model_id == "deepseek-coder")
        .unwrap();
    assert_eq!(fallback.context_limit, 128_000);
    assert_eq!(fallback.max_output_tokens, None);
    let resolved = service.models().resolve(&route.id).await.unwrap();
    assert_eq!(resolved.protocol, ProviderProtocol::OpenaiCompatible);
    assert_eq!(resolved.base_url, "https://api.deepseek.com");
    assert_eq!(resolved.context_limit, 64000);
    assert_eq!(resolved.model_id, "deepseek-chat");

    service
        .kb()
        .upsert_agent("main", &route.id, "Runtime test agent", &[], true)
        .await
        .unwrap();
    let session = service
        .create_session(&unique("model-route"), Some("runtime-doc"))
        .await
        .unwrap();
    let request = SendMessageRequest {
        client_msg_id: unique("model-msg"),
        content: "Use the registered model".into(),
        attachment_ids: Vec::new(),
    };
    service
        .post_message(&session.session_id, &request)
        .await
        .unwrap();
    wait_for_idle(&store, &session.session_id).await;
    let detail = service.session_detail(&session.session_id).await.unwrap();
    assert_eq!(detail.summary.context_usage.context_limit, 64000);
    service.delete_session(&session.session_id).await.unwrap();
    service.kb().delete_agent("main").await.unwrap();
    service
        .models()
        .delete_provider(&provider_id)
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn knowledge_update_is_injected_once_and_synced_version_advances() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let session = service
        .create_session(&unique("prompt-once"), Some("runtime-doc"))
        .await
        .unwrap();
    let initial_version = service.kb().current_version().await.unwrap();
    store
        .set_pinned_prompt(&session.session_id, "FROZEN PREFIX", initial_version)
        .await
        .unwrap();
    let document_id = unique("kb-update");
    service
        .kb()
        .upsert_document(&document_id, "Update", "New guidance")
        .await
        .unwrap();
    let bumped_version = service.kb().current_version().await.unwrap();
    let first = service
        .resume_context(&session.session_id, "First resume")
        .await
        .unwrap();
    assert_eq!(first.pinned_system_prompt, "FROZEN PREFIX");
    assert!(
        first
            .messages
            .iter()
            .any(|message| message.content.starts_with("<knowledge_update>"))
    );

    let request = SendMessageRequest {
        client_msg_id: unique("kb-msg"),
        content: "Run one turn".into(),
        attachment_ids: Vec::new(),
    };
    service
        .post_message(&session.session_id, &request)
        .await
        .unwrap();
    wait_for_idle(&store, &session.session_id).await;
    let (pinned, synced) = store.pinned_prompt(&session.session_id).await.unwrap();
    assert_eq!(pinned.as_deref(), Some("FROZEN PREFIX"));
    assert_eq!(synced, bumped_version);

    let second = service
        .resume_context(&session.session_id, "Second resume")
        .await
        .unwrap();
    assert_eq!(second.pinned_system_prompt, "FROZEN PREFIX");
    assert!(
        !second
            .messages
            .iter()
            .any(|message| message.content.starts_with("<knowledge_update>"))
    );
    service.kb().delete_document(&document_id).await.unwrap();
    service.delete_session(&session.session_id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streaming_tokens_are_coalesced_and_flushed_before_tool_and_turn_events() {
    let _guard = test_lock().lock().await;
    let (service, store) = test_service().await;
    let mut events = Vec::new();
    for _ in 0..30 {
        events.push(ModelStreamEvent::ThinkingDelta("t".into()));
    }
    for _ in 0..50 {
        events.push(ModelStreamEvent::TextDelta("x".into()));
    }
    events.push(ModelStreamEvent::ToolCallStarted {
        tool_call_id: "tc_stream".into(),
        tool_name: "describe_task".into(),
        input_preview: "{\"title\":\"Review\"}".into(),
        truncated: false,
    });
    let service =
        service.with_model_stream_provider(Arc::new(MockStreamProvider::new(vec![events])));
    let session = service
        .create_session(&unique("coalesce"), Some("runtime-doc"))
        .await
        .unwrap();
    let request = SendMessageRequest {
        client_msg_id: unique("coalesce-msg"),
        content: "Stream fifty fragments".into(),
        attachment_ids: Vec::new(),
    };
    service
        .post_message(&session.session_id, &request)
        .await
        .unwrap();
    wait_for_idle(&store, &session.session_id).await;

    let events = store
        .list_events(&session.session_id, 0, 500)
        .await
        .unwrap();
    let thinking_count = events
        .iter()
        .filter(|event| matches!(event.kind, SessionEventKind::ThinkingDelta { .. }))
        .count();
    let text_deltas: Vec<&str> = events
        .iter()
        .filter_map(|event| match &event.kind {
            SessionEventKind::TextDelta { delta } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    let text_count = text_deltas.len();
    assert_eq!(thinking_count, 1);
    assert_eq!(text_count, 1);
    assert_eq!(text_deltas.concat(), "x".repeat(50));
    assert!(thinking_count + text_count < 80);

    let last_text_seq = events
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                SessionEventKind::ThinkingDelta { .. } | SessionEventKind::TextDelta { .. }
            )
        })
        .map(|event| event.seq)
        .max()
        .unwrap();
    let tool_seq = events
        .iter()
        .find(|event| matches!(event.kind, SessionEventKind::ToolCallStarted { .. }))
        .map(|event| event.seq)
        .unwrap();
    let finished_seq = events
        .iter()
        .find(|event| matches!(event.kind, SessionEventKind::TurnFinished))
        .map(|event| event.seq)
        .unwrap();
    assert!(last_text_seq < tool_seq, "text must flush before tool call");
    assert!(
        tool_seq < finished_seq,
        "tool event must precede turn finish"
    );
    let detail = service.session_detail(&session.session_id).await.unwrap();
    assert_eq!(
        detail
            .messages
            .iter()
            .find(|message| message.role == quire_session_protocol::MessageRole::Assistant)
            .map(|message| message.content.as_str()),
        Some("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx")
    );
    service.delete_session(&session.session_id).await.unwrap();
}
