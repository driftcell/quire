use super::*;

#[test]
fn document_binding_keeps_prefix_and_extra_context_stable() {
    let execution = AgentExecution {
        agent_id: "main".into(),
        display_name: None,
        model_route: "test/model".into(),
        allowed_tools: Vec::new(),
        system_prompt: "FROZEN PREFIX".into(),
        kb_branch: Some("main".into()),
        doc_id: new_shared_doc_id(None),
        session_id: "sess_test".into(),
        run_id: "run_test".into(),
        cancellation: CancellationToken::new(),
    };
    let before = static_preamble(&execution);
    assert_eq!(before, "FROZEN PREFIX");

    let (sender, _receiver) = mpsc::channel(1);
    let control = TurnControlHook::for_test(
        execution.clone(),
        StreamEventSender::top(sender),
        SharedTurnState::default(),
    );
    assert!(control.session_document().is_none());

    update_doc_id(&execution.doc_id, Some("doc_lazy".into()));
    assert_eq!(static_preamble(&execution), before);
    assert!(control.session_document().is_none());
}

#[test]
fn usage_carries_real_tokens_and_context_limit() {
    let usage = Usage::new()
        .input_tokens(120_u64)
        .output_tokens(30_u64)
        .cached_input_tokens(40_u64);
    let mapped = context_usage(&usage, 64_000);
    assert_eq!(mapped.input_tokens, 120);
    assert_eq!(mapped.output_tokens, 30);
    assert_eq!(mapped.cached_tokens, 40);
    assert_eq!(mapped.context_used, 150);
    assert_eq!(mapped.context_limit, 64_000);
}

#[test]
fn static_preamble_marks_derived_subagents_without_changing_main_agent() {
    let main_exec = AgentExecution {
        agent_id: "main".into(),
        display_name: None,
        model_route: "stub/main".into(),
        allowed_tools: Vec::new(),
        system_prompt: "Lead prompt".into(),
        kb_branch: Some("main".into()),
        doc_id: new_shared_doc_id(None),
        session_id: "sess_1".into(),
        run_id: "run_1".into(),
        cancellation: CancellationToken::new(),
    };
    assert!(!static_preamble(&main_exec).contains("derived subagent"));

    let mut sub_exec = main_exec;
    sub_exec.agent_id = "writer".into();
    sub_exec.run_id = "run_sub_1".into();
    assert!(static_preamble(&sub_exec).contains("derived subagent"));
}

#[test]
fn resolve_turn_prompt_and_history_resumes_from_canonical_memory() {
    let recorded = vec![Message::user("最初的用户请求")];
    let fallback_prompt = Message::user("");
    let (resolved_prompt, resolved_history) =
        resolve_turn_prompt_and_history(&recorded, &fallback_prompt, true, None)
            .expect("resume from initial prompt");
    assert_eq!(resolved_prompt, Message::user("最初的用户请求"));
    assert!(resolved_history.is_empty());

    let stepped = vec![
        Message::user("最初的用户请求"),
        Message::assistant("已创建报告框架"),
        Message::user("tool result: rev=1"),
    ];
    let (resumed_prompt, resumed_history) =
        resolve_turn_prompt_and_history(&stepped, &fallback_prompt, true, None)
            .expect("resume from incrementally persisted memory");
    assert_eq!(resumed_prompt, Message::user("tool result: rev=1"));
    assert_eq!(resumed_history.len(), 2);
    assert_eq!(resumed_history[0], stepped[0]);
}
