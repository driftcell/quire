use super::*;

#[test]
fn document_binding_keeps_prefix_and_extra_context_stable() {
    let execution = AgentExecution {
        agent_id: "main".into(),
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
    assert!(control.session_documents().is_empty());

    update_doc_id(&execution.doc_id, Some("doc_lazy".into()));
    assert_eq!(static_preamble(&execution), before);
    assert!(control.session_documents().is_empty());
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
