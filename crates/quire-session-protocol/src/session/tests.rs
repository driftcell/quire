use super::*;

#[test]
fn session_detail_roundtrips_and_flattens_summary() {
    let now = Utc::now();
    let detail = SessionDetail {
        summary: SessionSummary {
            user_id: None,
            session_id: "sess_1".into(),
            title: "Q3 review".into(),
            doc_id: Some("q3-review".into()),
            linked_doc_id: Some("q3-review".into()),
            kb_branch: Some("main".into()),
            status: SessionStatus::Active,
            active_run_status: ActiveRunStatus::Idle,
            last_event_seq: 4,
            context_usage: ContextUsage {
                context_used: 120,
                context_limit: 200_000,
                ..ContextUsage::default()
            },
            compacting: false,
            compacted_at: None,
            pinned_at: None,
            created_at: now,
            updated_at: now,
        },
        messages: vec![SessionMessage {
            message_id: "msg_1".into(),
            session_id: "sess_1".into(),
            role: MessageRole::User,
            content: "Draft chapter 1".into(),
            comment_refs: Vec::new(),
            attachment_ids: Vec::new(),
            run_id: None,
            question_id: None,
            created_at: now,
        }],
        attachments: Vec::new(),
        resolved_questions: Vec::new(),
        pending_question: Some(PendingQuestion {
            question_id: "q_1".into(),
            run_id: "run_1".into(),
            tool_call_id: Some("run_1:call_1".into()),
            questions: vec![QuestionItem {
                id: "scope".into(),
                title: "Which scope?".into(),
                description: None,
                multiple: false,
                required: true,
                options: vec![QuestionOption {
                    id: "q3".into(),
                    label: "Q3".into(),
                    description: None,
                }],
            }],
            created_at: now,
        }),
        pending_queue: Vec::new(),
        active_run_trace: None,
        active_subagent_traces: Vec::new(),
    };
    let json = serde_json::to_value(&detail).unwrap();
    assert_eq!(json["session_id"], "sess_1");
    assert_eq!(json["active_run_status"], "idle");
    assert_eq!(json["messages"][0]["role"], "user");
    assert_eq!(json["pending_question"]["question_id"], "q_1");
    let decoded: SessionDetail = serde_json::from_value(json).unwrap();
    assert_eq!(decoded, detail);
}

#[test]
fn structured_comment_refs_roundtrip_and_default() {
    let comment_ref = SessionCommentRef {
        thread_id: "ct_1".into(),
        block_id: "block_1".into(),
        end_block_id: Some("block_2".into()),
        anchor_blocks: vec!["block_1".into(), "block_2".into()],
        status: "open".into(),
        quote: Some("quoted text".into()),
        end_quote: None,
        anchor_snapshot: Some("quoted text … [表格: Metrics]".into()),
        comment: "Revise this scope".into(),
    };
    let request = SendMessageRequest {
        client_msg_id: "msg_client_1".into(),
        content: String::new(),
        comment_refs: vec![comment_ref],
        attachment_ids: Vec::new(),
    };
    let json = serde_json::to_value(&request).unwrap();
    assert_eq!(json["comment_refs"][0]["thread_id"], "ct_1");
    assert!(json["content"].as_str().is_some_and(str::is_empty));
    let decoded: SendMessageRequest = serde_json::from_value(json).unwrap();
    assert_eq!(decoded, request);

    let update: UpdatePendingMessageRequest =
        serde_json::from_value(serde_json::json!({"content": "edited"})).unwrap();
    assert!(update.comment_refs.is_empty());
}

#[test]
fn ask_user_answer_payload_is_structured_and_resolves_labels() {
    let question = PendingQuestion {
        question_id: "q_1".into(),
        run_id: "run_1".into(),
        tool_call_id: Some("run_1:call_1".into()),
        questions: vec![QuestionItem {
            id: "scope".into(),
            title: "Which scope?".into(),
            description: None,
            multiple: false,
            required: true,
            options: vec![QuestionOption {
                id: "q3".into(),
                label: "Q3".into(),
                description: None,
            }],
        }],
        created_at: Utc::now(),
    };
    let answers = vec![QuestionAnswer {
        question_id: "scope".into(),
        selected_option_ids: vec!["q3".into()],
        freeform_text: None,
        skipped: false,
    }];
    let payload = AskUserAnswerPayload::from_resolution(&question, &answers, false);
    let value = serde_json::to_value(payload).unwrap();
    assert_eq!(value["kind"], "ask_user_response");
    assert_eq!(value["status"], "answered");
    assert_eq!(value["answers"][0]["question_title"], "Which scope?");
    assert_eq!(value["answers"][0]["selected_option_labels"][0], "Q3");
    assert!(value.get("The user answered questionnaire").is_none());
}

#[test]
fn task_todo_status_is_required_in_the_wire_contract() {
    let schema = serde_json::to_value(schemars::schema_for!(TaskTodoItem)).unwrap();
    let required = schema["required"]
        .as_array()
        .expect("TaskTodoItem schema should declare required fields");
    assert!(
        required.iter().any(|field| field == "status"),
        "status must be required for frontend wire types"
    );
    assert!(
        schema["properties"]["status"].get("default").is_none(),
        "status must not be optional or defaulted in the wire schema"
    );
    assert!(
        serde_json::from_value::<TaskTodoItem>(serde_json::json!({"title": "缺少状态"})).is_err(),
        "wire payloads must carry status explicitly"
    );
}

#[test]
fn status_helpers_match_wire_values() {
    assert_eq!(SessionStatus::parse("active"), Some(SessionStatus::Active));
    assert_eq!(
        ActiveRunStatus::parse("interrupted"),
        Some(ActiveRunStatus::Interrupted)
    );
    assert_eq!(
        ActiveRunStatus::parse("awaiting_input"),
        Some(ActiveRunStatus::AwaitingInput)
    );
    assert_eq!(
        MessageRole::parse("assistant"),
        Some(MessageRole::Assistant)
    );
    assert_eq!(MessageRole::parse("question"), Some(MessageRole::Question));
    assert!(ActiveRunStatus::parse("bogus").is_none());
}
