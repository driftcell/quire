use super::*;

#[test]
fn document_binding_changes_extra_context_not_the_frozen_preamble() {
    let execution = AgentExecution {
        agent_id: "main".into(),
        model_route: "test/model".into(),
        allowed_tools: Vec::new(),
        system_prompt: "FROZEN PREFIX".into(),
        kb_branch: None,
        doc_id: new_shared_doc_id(None),
        session_id: "sess_test".into(),
        run_id: "run_test".into(),
        cancellation: CancellationToken::new(),
    };
    let before = static_preamble(&execution);
    let (sender, _receiver) = mpsc::channel(1);
    let control = TurnControlHook::for_test(
        execution.clone(),
        StreamEventSender::top(sender),
        SharedTurnState::default(),
    );
    let initial_documents = control.session_documents();
    assert!(
        initial_documents
            .first()
            .is_some_and(|document| { document.text.contains("doc_id: none") })
    );

    update_doc_id(&execution.doc_id, Some("doc_lazy".into()));
    assert_eq!(static_preamble(&execution), before);
    assert!(!before.contains("doc_lazy"));
    let documents = control.session_documents();
    assert!(
        documents
            .first()
            .is_some_and(|document| { document.text.contains("doc_id: doc_lazy") })
    );
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
