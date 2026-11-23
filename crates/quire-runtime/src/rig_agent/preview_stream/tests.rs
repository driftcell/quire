use super::super::{new_shared_doc_id, update_doc_id};
use super::*;

#[test]
fn preview_throttle_respects_the_interval() {
    let mut throttle = PreviewThrottle::new(60);
    let start = Instant::now();
    assert!(throttle.should_emit_at(start));
    assert!(!throttle.should_emit_at(start + Duration::from_millis(20)));
    assert!(throttle.should_emit_at(start + Duration::from_millis(61)));
}

#[test]
fn partial_apply_operations_arguments_produce_block_previews() {
    let raw = r#"{"ops":[{"type":"append","blocks":[{"id":"q3-revenue","type":"paragraph","text":"Revenue grew"#;
    let arguments = Value::Object(parse_partial_arguments(raw));
    let blocks = extract_preview_blocks(&arguments);
    assert_eq!(blocks.len(), 1);
    let block = blocks.first().expect("preview block");
    assert_eq!(block.block_id.as_deref(), Some("q3-revenue"));
    assert_eq!(block.block_type, "paragraph");
    assert_eq!(block.text.as_deref(), Some("Revenue grew"));
}

#[test]
fn table_and_echarts_blocks_produce_single_rich_previews_without_inner_duplicates() {
    let arguments = json!({
        "ops": [{
            "type": "append",
            "blocks": [
                {
                    "id": "ch1_table",
                    "type": "table",
                    "caption": "核心学科经费对比",
                    "columns": [
                        {"key": "school", "label": "高校"},
                        {"key": "budget", "label": "经费（亿元）"}
                    ],
                    "rows": [
                        {"school": "A校", "budget": "14.2"},
                        {"school": "B校", "budget": "11.8"}
                    ]
                },
                {
                    "id": "ch1_chart",
                    "type": "echarts",
                    "title": "近五年经费趋势",
                    "fallback_summary": "A校近五年持续增长",
                    "option": {
                        "title": {"text": "内部子标题不应生成独立段落块"},
                        "series": [{"type": "line", "data": [10, 12, 14]}]
                    }
                }
            ]
        }]
    });
    let blocks = extract_preview_blocks(&arguments);
    assert_eq!(
        blocks.len(),
        2,
        "nested option.title.text or table columns must not create extra blocks: {:?}",
        blocks
            .iter()
            .map(|b| (&b.block_id, &b.block_type))
            .collect::<Vec<_>>()
    );
    assert_eq!(blocks[0].block_id.as_deref(), Some("ch1_table"));
    assert_eq!(blocks[0].block_type, "table");
    assert_eq!(blocks[0].title.as_deref(), Some("核心学科经费对比"));
    assert!(blocks[0].text.as_deref().is_some_and(
            |text| text.contains("高校 | 经费（亿元）") && text.contains("A校 | 14.2")
        ));
    assert_eq!(blocks[1].block_id.as_deref(), Some("ch1_chart"));
    assert_eq!(blocks[1].block_type, "echarts");
    assert_eq!(blocks[1].title.as_deref(), Some("近五年经费趋势"));
    assert_eq!(blocks[1].text.as_deref(), Some("A校近五年持续增长"));
}

#[test]
fn preview_anchors_chain_inserted_blocks_and_patch_targets() {
    let arguments = json!({
        "ops": [
            {
                "type": "insert_after",
                "target_id": "sec3_h1",
                "blocks": [
                    {"id": "sec3_p1", "type": "paragraph", "text": "hello"},
                    {"id": "sec3_p2", "type": "paragraph", "text": "world"}
                ]
            },
            {
                "block_id": "sec2_p1",
                "patch": {"text": "updated"},
                "type": "patch_block"
            }
        ]
    });
    let blocks = extract_preview_blocks(&arguments);
    assert_eq!(blocks.len(), 3);
    assert_eq!(blocks[0].block_id.as_deref(), Some("sec3_p1"));
    assert_eq!(
        blocks[0].anchor,
        PreviewAnchor::AfterBlock {
            block_id: BlockId::parse("sec3_h1").expect("valid target")
        }
    );
    assert_eq!(blocks[1].block_id.as_deref(), Some("sec3_p2"));
    assert_eq!(
        blocks[1].anchor,
        PreviewAnchor::AfterBlock {
            block_id: BlockId::parse("sec3_p1").expect("valid previous block")
        }
    );
    assert_eq!(blocks[2].block_id.as_deref(), Some("sec2_p1"));
    assert_eq!(
        blocks[2].anchor,
        PreviewAnchor::ReplaceBlock {
            block_id: BlockId::parse("sec2_p1").expect("valid patch target")
        }
    );
    assert_eq!(blocks[2].text.as_deref(), Some("updated"));
}

#[test]
fn preview_anchor_chains_insert_before_blocks_in_forward_order() {
    let arguments = json!({
        "ops": [{
            "type": "insert_before",
            "target_id": "sec3_p3",
            "blocks": [
                {"id": "sec3_p1", "type": "paragraph", "text": "first"},
                {"id": "sec3_p2", "type": "paragraph", "text": "second"}
            ]
        }]
    });
    let blocks = extract_preview_blocks(&arguments);
    assert_eq!(blocks.len(), 2);
    assert_eq!(
        blocks[0].anchor,
        PreviewAnchor::BeforeBlock {
            block_id: BlockId::parse("sec3_p3").expect("valid target")
        }
    );
    assert_eq!(
        blocks[1].anchor,
        PreviewAnchor::AfterBlock {
            block_id: BlockId::parse("sec3_p1").expect("valid previous block")
        }
    );
}

#[test]
fn partial_patch_operation_anchors_from_block_id_before_type_arrives() {
    let arguments = json!({
        "ops": [{
            "block_id": "ch2_p2",
            "patch": {"text": "updated"}
        }]
    });
    let blocks = extract_preview_blocks(&arguments);
    assert_eq!(blocks.len(), 1);
    let block = blocks.first().expect("partial patch preview");
    assert_eq!(block.block_id.as_deref(), Some("ch2_p2"));
    assert_eq!(block.block_type, "paragraph");
    assert_eq!(block.text.as_deref(), Some("updated"));
    assert_eq!(
        block.anchor,
        PreviewAnchor::ReplaceBlock {
            block_id: BlockId::parse("ch2_p2").expect("patch target")
        }
    );
}

#[test]
fn preview_anchors_follow_real_type_discriminator_wire_format() {
    let arguments = json!({
        "doc_id": "report",
        "ops": [
            {
                "blocks": [
                    {"id": "ch2_sec", "level": 2, "text": "（四）配套建议", "type": "heading"},
                    {"id": "ch2_p1", "text": "人才引育", "type": "paragraph"}
                ],
                "target_id": "ch2_p_update",
                "type": "insert_after"
            },
            {
                "block_id": "ch2_p2",
                "patch": {"text": "updated"},
                "type": "patch_block"
            }
        ]
    });
    let blocks = extract_preview_blocks(&arguments);
    assert_eq!(blocks.len(), 3);
    assert_eq!(
        blocks[0].anchor,
        PreviewAnchor::AfterBlock {
            block_id: BlockId::parse("ch2_p_update").expect("real target")
        }
    );
    assert_eq!(
        blocks[1].anchor,
        PreviewAnchor::AfterBlock {
            block_id: BlockId::parse("ch2_sec").expect("chained previous block")
        }
    );
    assert_eq!(
        blocks[2].anchor,
        PreviewAnchor::ReplaceBlock {
            block_id: BlockId::parse("ch2_p2").expect("patch target")
        }
    );
    assert_eq!(blocks[2].block_type, "paragraph");
    assert_eq!(blocks[2].text.as_deref(), Some("updated"));
}

#[test]
fn preview_anchor_preserves_after_before_and_replace_targets()
-> Result<(), Box<dyn std::error::Error>> {
    let target = BlockId::parse("sec2_h1")?;
    let current = BlockId::parse("sec2_p1")?;
    let block = PartialBlockPreview {
        block_id: Some(current.clone()),
        block_type: "paragraph".into(),
        title: None,
        level: None,
        text: Some("streaming".into()),
    };
    let make_streamer = |preview_id: &str, anchor: PreviewAnchor| {
        ToolPreviewStreamer::new(
            "http://127.0.0.1:1",
            SecretString::from("token"),
            "doc",
            preview_id,
            "actor",
            "sec2_p1",
            0,
        )
        .with_anchor(anchor)
    };

    let mut after = make_streamer(
        "preview-after",
        PreviewAnchor::AfterBlock {
            block_id: target.clone(),
        },
    );
    let event = after.next_preview_event(block.clone())?;
    assert_eq!(
        event.anchor,
        PreviewAnchor::AfterBlock {
            block_id: target.clone()
        }
    );

    let mut before = make_streamer(
        "preview-before",
        PreviewAnchor::BeforeBlock {
            block_id: target.clone(),
        },
    );
    let event = before.next_preview_event(block.clone())?;
    assert_eq!(
        event.anchor,
        PreviewAnchor::BeforeBlock {
            block_id: target.clone()
        }
    );

    let mut replace = make_streamer(
        "preview-replace",
        PreviewAnchor::ReplaceBlock { block_id: target },
    );
    let event = replace.next_preview_event(block)?;
    assert_eq!(
        event.anchor,
        PreviewAnchor::ReplaceBlock { block_id: current }
    );
    Ok(())
}

#[test]
fn incomplete_anchors_stay_deferred_but_explicit_append_is_resolved() {
    let partial = json!({
        "ops": [{"blocks": [{"text": "draft"}]}]
    });
    let blocks = extract_preview_blocks(&partial);
    assert_eq!(blocks.len(), 1);
    assert!(!blocks[0].anchor_resolved);

    let explicit = json!({
        "ops": [{"type": "append", "blocks": [{"text": "draft"}]}]
    });
    let blocks = extract_preview_blocks(&explicit);
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].anchor_resolved);
    assert_eq!(blocks[0].anchor, PreviewAnchor::AppendToDoc);
}

#[tokio::test]
async fn preview_identity_is_unique_per_tool_call_and_stable_while_id_arrives() {
    let tool = ToolName::new("apply_operations").expect("tool name");
    let partial = r#"{"ops":[{"type":"append_to_section","section_id":"sec1","blocks":[{"type":"paragraph","text":"draft""#;
    let suffix = r#","id":"sec1_p"}]}]}"#;
    let complete = r#"{"ops":[{"type":"append_to_section","section_id":"sec1","blocks":[{"type":"paragraph","text":"draft","id":"sec1_p"}]}]}"#;

    let mut first = PreviewState::for_test("run_test", new_shared_doc_id(Some("doc".into())));
    first.on_start(1, PartKind::ToolCall, Some(&tool));
    first.on_arguments(1, partial).await;
    let first_id = first.streams[&1].streamers[&0].preview_id().to_string();
    first.on_arguments(1, suffix).await;
    let stream = &first.streams[&1];
    assert_eq!(stream.streamers.len(), 1);
    assert_eq!(stream.streamers[&0].preview_id(), first_id);
    assert_eq!(stream.streamers[&0].block_id, "sec1_p");

    let mut second = PreviewState::for_test("run_test", new_shared_doc_id(Some("doc".into())));
    second.on_start(1, PartKind::ToolCall, Some(&tool));
    second.on_arguments(1, complete).await;
    let second_id = second.streams[&1].streamers[&0].preview_id();
    assert_ne!(first_id, second_id);
}

#[tokio::test]
async fn lazy_document_binding_enables_subsequent_tool_deltas() {
    let doc_id = new_shared_doc_id(None);
    let mut state = PreviewState::for_test("run_test", Arc::clone(&doc_id));
    state
        .on_tool_delta(1, 7, "apply_operations", r#"{"ops":[]}"#)
        .await;
    assert!(!state.streams.contains_key(&7));

    update_doc_id(&doc_id, Some("doc_lazy".into()));
    state
        .on_tool_delta(1, 7, "apply_operations", r#"{"ops":[]}"#)
        .await;
    assert!(state.streams.contains_key(&7));
}

#[test]
fn preview_ids_are_merged_into_tool_arguments() {
    let mut arguments = json!({"doc_id": "report", "preview_ids": ["old"]});
    inject_preview_ids(&mut arguments, &["new".into(), "old".into()]);
    let ids = arguments
        .get("preview_ids")
        .and_then(Value::as_array)
        .expect("preview ids");
    assert_eq!(ids.len(), 2);
    assert_eq!(ids.first().and_then(Value::as_str), Some("old"));
    assert_eq!(ids.last().and_then(Value::as_str), Some("new"));
}

#[cfg(test)]
mod handoff_tests {
    use super::*;
    use std::sync::Arc;

    use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
    use tokio::sync::Mutex;

    async fn record_preview(
        State(requests): State<Arc<Mutex<Vec<Value>>>>,
        Json(payload): Json<Value>,
    ) -> StatusCode {
        let mut requests = requests.lock().await;
        let delay = if requests.is_empty() { 200 } else { 0 };
        requests.push(payload);
        drop(requests);
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        StatusCode::OK
    }

    pub(super) async fn spawn_server() -> (String, Arc<Mutex<Vec<Value>>>) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/api/docs/doc/ephemeral", post(record_preview))
            .with_state(Arc::clone(&requests));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{address}"), requests)
    }

    pub(super) fn block(text: &str) -> PartialBlockPreview {
        PartialBlockPreview {
            block_id: Some(BlockId::pending()),
            block_type: "paragraph".into(),
            title: None,
            level: None,
            text: Some(text.to_string()),
        }
    }

    fn preview_text(payload: &Value) -> String {
        payload
            .get("event")
            .unwrap_or(payload)
            .get("block")
            .and_then(|block| block.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    #[tokio::test]
    async fn intermediate_frames_are_latest_wins_and_flush_waits_for_the_final_frame() {
        let (base_url, requests) = spawn_server().await;
        let mut streamer = ToolPreviewStreamer::new(
            &base_url,
            SecretString::from("test-token"),
            "doc",
            "preview-1",
            "main",
            "block-1",
            50,
        );

        streamer.push_block(block("first")).unwrap();
        streamer.push_block(block("second")).unwrap();
        streamer.push_block(block("third")).unwrap();
        tokio::time::timeout(
            Duration::from_secs(2),
            streamer.push_block_now(block("final")),
        )
        .await
        .unwrap()
        .unwrap();

        let requests = requests.lock().await;
        let texts = requests.iter().map(preview_text).collect::<Vec<_>>();
        assert_eq!(texts.last().map(String::as_str), Some("final"));
        assert!(
            texts.len() <= 2,
            "latest-frame-wins must not replay every intermediate frame: {texts:?}"
        );
    }
}

#[cfg(test)]
mod flush_wakeup_tests {
    use super::{
        handoff_tests::{block, spawn_server},
        *,
    };

    #[tokio::test]
    async fn flush_returns_immediately_once_the_last_frame_is_sent() {
        let (base_url, _requests) = spawn_server().await;
        let mut streamer = ToolPreviewStreamer::new(
            &base_url,
            SecretString::from("test-token"),
            "doc",
            "preview-wakeup",
            "main",
            "block-1",
            50,
        );
        streamer.push_block(block("first")).unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
        tokio::time::timeout(Duration::from_millis(100), streamer.transport.flush())
            .await
            .unwrap()
            .unwrap();
    }
}
