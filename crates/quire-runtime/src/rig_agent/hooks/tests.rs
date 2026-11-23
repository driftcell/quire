use super::*;
use rig::{
    completion::ResponseIdentity,
    effect::{Answered, EffectId, EffectKind},
    message::{CallId, Message, ToolCall, ToolFunction, ToolName},
    test_utils::HookTestHarness,
    tool::{ToolContext, ToolOutput, ToolResult},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::super::{SharedDocId, new_shared_doc_id};

fn execution() -> AgentExecution {
    AgentExecution {
        agent_id: "main".into(),
        model_route: "test/model".into(),
        allowed_tools: vec!["ask_user".into(), "apply_operations".into()],
        system_prompt: "test".into(),
        kb_branch: Some("main".into()),
        doc_id: new_shared_doc_id(None),
        session_id: "sess_test".into(),
        run_id: "run_test".into(),
        cancellation: CancellationToken::new(),
    }
}

fn push_tool(content: &mut Vec<AssistantContent>, call_id: &str, tool_name: &str) {
    let Ok(name) = ToolName::new(tool_name) else {
        return;
    };
    content.push(AssistantContent::ToolCall(ToolCall::from_wire(
        call_id,
        ToolFunction::new(name, json!({})),
    )));
}

fn question_turn(content: &mut Vec<AssistantContent>) {
    push_tool(content, "call_question", "ask_user");
}

#[tokio::test]
async fn mixed_question_batch_emits_one_skipped_start_and_finish() {
    let (sender, mut receiver) = mpsc::channel(8);
    let execution = execution();
    let scope = StreamEventSender::top(sender);
    let state = SharedTurnState::default();
    let previews = Arc::new(Mutex::new(PreviewState::for_test(
        "run_test",
        SharedDocId::default(),
    )));
    let harness = HookTestHarness::new()
        .with_hook(TurnControlHook::for_test(
            execution.clone(),
            scope.clone(),
            Arc::clone(&state),
        ))
        .with_hook(ToolLifecycleHook::for_test(
            execution,
            scope,
            previews,
            Arc::clone(&state),
        ));
    harness.set_turn(1);

    let mut content = Vec::new();
    question_turn(&mut content);
    assert_eq!(content.len(), 1);
    let identity = ResponseIdentity::default();
    let raw = Value::Null;
    let action = harness
        .hooks()
        .on_model_turn_finished(
            harness.context(),
            ModelTurnFinished {
                turn: 1,
                content: &content,
                usage: Usage::default(),
                identity: &identity,
                finish_reason: None,
                max_tokens: None,
                raw: &raw,
            },
        )
        .await;
    assert_eq!(action, ModelTurnAction::Continue);

    let kind = EffectKind::ToolCall {
        name: "apply_operations".into(),
        args: "{}".into(),
    };
    let call_id = CallId::from_wire("call_sibling");
    let action = harness
        .hooks()
        .on_dispatch(
            harness.context(),
            DispatchEvent {
                id: EffectId::from_raw(1),
                kind: &kind,
                turn: 1,
                call_id: Some(&call_id),
                context: None,
            },
        )
        .await;
    assert!(matches!(action, DispatchAction::Deny(_)));

    let outcome = Ok(Answered::ToolResult {
        result: ToolResult::skipped("ask_user_sibling"),
    });
    let action = harness
        .hooks()
        .on_outcome(
            harness.context(),
            OutcomeEvent {
                id: EffectId::from_raw(1),
                kind: &kind,
                outcome: &outcome,
                turn: 1,
                call_id: Some(&call_id),
                context: None,
            },
        )
        .await;
    assert!(matches!(action, OutcomeAction::Proceed));

    let events: Vec<ModelStreamEvent> = std::iter::from_fn(|| receiver.try_recv().ok())
        .map(|result| result.ok())
        .take_while(Option::is_some)
        .flatten()
        .collect();
    assert_eq!(events.len(), 2, "unexpected events: {events:?}");
    assert!(matches!(
        events.first(),
        Some(ModelStreamEvent::ToolCallStarted { .. })
    ));
    assert!(matches!(
        events.get(1),
        Some(ModelStreamEvent::ToolCallFinished { status, .. }) if status == "skipped"
    ));
}

#[tokio::test]
async fn question_batch_stops_at_next_completion_without_losing_tool_results() {
    let (sender, mut receiver) = mpsc::channel(16);
    let execution = execution();
    let scope = StreamEventSender::top(sender);
    let state = SharedTurnState::default();
    let previews = Arc::new(Mutex::new(PreviewState::for_test(
        "run_test",
        SharedDocId::default(),
    )));
    let harness = HookTestHarness::new()
        .with_hook(TurnControlHook::for_test(
            execution.clone(),
            scope.clone(),
            Arc::clone(&state),
        ))
        .with_hook(ToolLifecycleHook::for_test(
            execution,
            scope,
            previews,
            Arc::clone(&state),
        ));
    harness.set_turn(1);
    let mut content = Vec::new();
    question_turn(&mut content);
    push_tool(&mut content, "call_sibling", "apply_operations");
    let identity = ResponseIdentity::default();
    let raw = Value::Null;
    let _ = harness
        .hooks()
        .on_model_turn_finished(
            harness.context(),
            ModelTurnFinished {
                turn: 1,
                content: &content,
                usage: Usage::default(),
                identity: &identity,
                finish_reason: None,
                max_tokens: None,
                raw: &raw,
            },
        )
        .await;

    let question_kind = EffectKind::ToolCall {
        name: "ask_user".into(),
        args: "{}".into(),
    };
    let question_id = CallId::from_wire("call_question");
    let _ = harness
        .hooks()
        .on_dispatch(
            harness.context(),
            DispatchEvent {
                id: EffectId::from_raw(1),
                kind: &question_kind,
                turn: 1,
                call_id: Some(&question_id),
                context: None,
            },
        )
        .await;
    let mut tool_context = ToolContext::new();
    let _ = tool_context.insert_result(AskUserRequestedMeta {
        questions: Vec::new(),
    });
    let question_outcome = Ok(Answered::ToolResult {
        result: ToolResult::success(ToolOutput::json(json!({
            "status": "awaiting_input"
        }))),
    });
    let action = harness
        .hooks()
        .on_outcome(
            harness.context(),
            OutcomeEvent {
                id: EffectId::from_raw(1),
                kind: &question_kind,
                outcome: &question_outcome,
                turn: 1,
                call_id: Some(&question_id),
                context: Some(&tool_context),
            },
        )
        .await;
    assert!(matches!(action, OutcomeAction::Proceed));

    let sibling_kind = EffectKind::ToolCall {
        name: "apply_operations".into(),
        args: "{}".into(),
    };
    let sibling_id = CallId::from_wire("call_sibling");
    let action = harness
        .hooks()
        .on_dispatch(
            harness.context(),
            DispatchEvent {
                id: EffectId::from_raw(2),
                kind: &sibling_kind,
                turn: 1,
                call_id: Some(&sibling_id),
                context: None,
            },
        )
        .await;
    assert!(matches!(action, DispatchAction::Deny(_)));
    let sibling_outcome = Ok(Answered::ToolResult {
        result: ToolResult::skipped("ask_user_sibling"),
    });
    let action = harness
        .hooks()
        .on_outcome(
            harness.context(),
            OutcomeEvent {
                id: EffectId::from_raw(2),
                kind: &sibling_kind,
                outcome: &sibling_outcome,
                turn: 1,
                call_id: Some(&sibling_id),
                context: None,
            },
        )
        .await;
    assert!(matches!(action, OutcomeAction::Proceed));

    harness.set_turn(2);
    let prompt = Message::user("committed tool results");
    let history: Vec<Arc<Message>> = Vec::new();
    let action = harness
        .hooks()
        .on_completion_call(
            harness.context(),
            CompletionCallEvent {
                prompt: &prompt,
                history: &history,
                turn: 2,
            },
        )
        .await;
    assert!(matches!(
        action,
        CompletionCallAction::Stop(reason) if reason == ASK_USER_REASON
    ));

    let mut events = Vec::new();
    while let Ok(Ok(event)) = receiver.try_recv() {
        events.push(event);
    }
    assert_eq!(events.len(), 5, "unexpected events: {events:?}");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ModelStreamEvent::ToolCallStarted { .. }))
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, ModelStreamEvent::ToolCallFinished { .. }))
            .count(),
        2
    );
    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEvent::ToolCallFinished { status, .. } if status == "awaiting_input"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ModelStreamEvent::ToolCallFinished { status, .. } if status == "skipped"
    )));
}
