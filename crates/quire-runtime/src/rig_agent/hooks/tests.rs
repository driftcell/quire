use super::*;
use rig::{
    completion::{ResponseIdentity, Usage},
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
        display_name: None,
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
async fn model_turn_finished_emits_per_call_usage_and_cost() {
    let (sender, mut receiver) = mpsc::channel(8);
    let harness = HookTestHarness::new().with_hook(TurnControlHook {
        execution: execution(),
        scope: StreamEventSender::top(sender),
        queue: None,
        kb_update_notice: None,
        pricing: Some(ModelTurnPricing {
            context_limit: 64_000,
            cost_input: Some(1.0),
            cost_output: Some(2.0),
            cost_cache_read: Some(0.5),
        }),
        state: SharedTurnState::default(),
        parent_history: super::super::new_shared_fork_history(),
        memory: None,
    });
    harness.set_turn(1);
    let content = Vec::new();
    let identity = ResponseIdentity::default();
    let raw = Value::Null;
    let calls = [
        (
            Usage::new()
                .input_tokens(120_u64)
                .output_tokens(30_u64)
                .cached_input_tokens(40_u64),
            (120, 30, 40, 150),
            0.00016,
        ),
        (
            Usage::new().input_tokens(10_u64).output_tokens(5_u64),
            (10, 5, 0, 15),
            0.00002,
        ),
    ];

    for (usage, expected, expected_cost) in calls {
        let action = harness
            .hooks()
            .on_model_turn_finished(
                harness.context(),
                ModelTurnFinished {
                    turn: 1,
                    content: &content,
                    usage,
                    identity: &identity,
                    finish_reason: None,
                    max_tokens: None,
                    raw: &raw,
                },
            )
            .await;
        assert_eq!(action, ModelTurnAction::Continue);

        let event = receiver.try_recv().ok().and_then(|result| result.ok());
        assert!(matches!(event, Some(ModelStreamEvent::Usage(_))));
        let Some(ModelStreamEvent::Usage(actual)) = event else {
            return;
        };
        assert_eq!(
            (
                actual.input_tokens,
                actual.output_tokens,
                actual.cached_tokens,
                actual.context_used,
            ),
            expected
        );
        assert_eq!(actual.context_limit, 64_000);
        let cost = actual.cost_rmb.unwrap_or_default();
        assert!((cost - expected_cost).abs() < f64::EPSILON);
    }
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

#[tokio::test]
async fn completion_call_captures_main_history_snapshot_and_skips_subagents() {
    use super::super::current_fork_history;

    let parent_history = super::super::new_shared_fork_history();
    let (sender, _receiver) = mpsc::channel(8);
    let main_hook = TurnControlHook {
        execution: execution(),
        scope: StreamEventSender::top(sender.clone()),
        queue: None,
        kb_update_notice: None,
        pricing: None,
        state: SharedTurnState::default(),
        parent_history: Arc::clone(&parent_history),
        memory: None,
    };
    let harness = HookTestHarness::new();
    harness.set_turn(1);

    let prior = [Arc::new(Message::user("先前的多轮对话背景"))];
    let prompt = Message::user("请派生子代理调研");
    let _ = main_hook
        .on_completion_call(
            harness.context(),
            CompletionCallEvent {
                prompt: &prompt,
                history: &prior,
                turn: 1,
            },
        )
        .await;
    let captured = current_fork_history(&parent_history);
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0], Message::user("先前的多轮对话背景"));
    assert_eq!(captured[1], Message::user("请派生子代理调研"));

    let mut sub_exec = execution();
    sub_exec.run_id = "run_sub_child".into();
    let sub_hook = TurnControlHook {
        execution: sub_exec,
        scope: StreamEventSender::top(sender),
        queue: None,
        kb_update_notice: None,
        pricing: None,
        state: SharedTurnState::default(),
        parent_history: Arc::clone(&parent_history),
        memory: None,
    };
    let sub_harness = HookTestHarness::new();
    sub_harness.set_turn(1);
    let sub_prompt = Message::user("子代理内部提示");
    let _ = sub_hook
        .on_completion_call(
            sub_harness.context(),
            CompletionCallEvent {
                prompt: &sub_prompt,
                history: &[],
                turn: 1,
            },
        )
        .await;
    assert_eq!(current_fork_history(&parent_history).len(), 2);
}
