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

use quire_runtime::{
    Config, KbService, RuntimeService, RuntimeStore, StaticQuireControl, ToolCallRecord,
    bus::{InterruptCommand, RedisBus},
    control::RunRegistry,
};
use quire_session_protocol::{ActiveRunStatus, SendMessageRequest, SessionEventKind};
use secrecy::SecretString;
use tokio::sync::Mutex;

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
    let config = Arc::new(Config {
        database_url: database_url(),
        redis_url: redis_url(),
        bind_addr: "127.0.0.1:0".into(),
        quire_base_url: "http://127.0.0.1:1".into(),
        quire_rw_token: SecretString::from("test-token"),
        quire_ro_token: SecretString::from("test-token"),
        model_api_key: None,
        workspace_dir: std::env::temp_dir(),
        default_model: "stub/echo".into(),
        sse_replay_limit: 500,
        preview_throttle_ms: 65,
    });
    let service = RuntimeService::new(store.clone(), kb, config, Arc::new(StaticQuireControl));
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
