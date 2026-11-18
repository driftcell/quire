//! Integration tests against live PostgreSQL and Redis.
//!
//! Run with:
//!   just up
//!   just migrate
//!   cargo test -p quire-store --test store_integration -- --test-threads=1

#![allow(clippy::unwrap_used, clippy::expect_used)]

use quire_protocol::{
    Block, BlockId, BlockPatch, CoverConfig, DocId, PatchOp, ReportDocument, Revision, SessionId,
    SourceEntry, SourceId, TableBlock, TocConfig,
};
use quire_store::{PgReportStore, StoreError};
use sqlx::Row;

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

async fn test_store() -> PgReportStore {
    let _ = dotenvy::dotenv();
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://report:report@127.0.0.1:5432/report_core".into());
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    PgReportStore::connect(&database_url, &redis_url)
        .await
        .expect("live PostgreSQL and Redis are required; run `just up && just migrate`")
}

fn as_doc_id(raw: &str) -> DocId {
    DocId::parse(raw).expect("valid doc id")
}

fn as_block_id(raw: &str) -> BlockId {
    BlockId::parse(raw).expect("valid block id")
}

fn as_source_id(raw: &str) -> SourceId {
    SourceId::parse(raw).expect("valid source id")
}

fn as_revision(value: i64) -> Revision {
    Revision::new(value).expect("valid revision")
}

fn document(doc_id_value: &str, session_id: Option<&str>, blocks: Vec<Block>) -> ReportDocument {
    let mut doc = ReportDocument::new(as_doc_id(doc_id_value));
    doc.session_id = session_id.map(|value| SessionId::parse(value).expect("valid session"));
    doc.cover = CoverConfig {
        enabled: true,
        title: "Integration Report".into(),
        authors: vec!["Ada Lovelace".into()],
        ..Default::default()
    };
    doc.toc = TocConfig::default();
    doc.sources.insert(
        as_source_id("sec10"),
        SourceEntry {
            authors: vec!["Jane Smith".into()],
            date: Some("2025-02-01".into()),
            ..SourceEntry::new("Annual filing 10-K")
        },
    );
    doc.blocks = blocks;
    doc
}

fn paragraph(id: &str, text: &str) -> Block {
    Block::paragraph(as_block_id(id), text)
}

fn patch_text(block_id: &str, text: &str) -> PatchOp {
    PatchOp::PatchBlock {
        block_id: as_block_id(block_id),
        patch: BlockPatch {
            text: Some(text.into()),
            ..Default::default()
        },
    }
}

async fn cleanup(store: &PgReportStore, doc_id: &str) {
    sqlx::query("DELETE FROM report_documents WHERE doc_id = $1")
        .bind(doc_id)
        .execute(store.pool())
        .await
        .expect("cleanup");
}

#[tokio::test]
async fn create_is_idempotent_and_broadcasts_session_event() {
    let store = test_store().await;
    let doc_id = unique("it-create");
    let session_id = unique("sess");
    let idempotency_key = unique("idem");
    let doc = document(
        &doc_id,
        Some(&session_id),
        vec![paragraph("intro", "Hello")],
    );

    let first = store
        .create_document(&doc, "lead", Some(&idempotency_key))
        .await
        .unwrap();
    assert_eq!(first.revision, Revision::ONE);
    assert!(!first.idempotent_replay);
    assert_eq!(first.doc_id, doc_id);

    let mut second_doc = doc.clone();
    second_doc.doc_id = as_doc_id(&unique("it-other"));
    let second = store
        .create_document(&second_doc, "lead", Some(&idempotency_key))
        .await
        .unwrap();
    assert!(second.idempotent_replay);
    assert_eq!(second.doc_id, doc_id);
    assert_eq!(second.revision, Revision::ONE);

    let session_events = store.bus().read_session_events(&session_id).await.unwrap();
    assert!(
        session_events.iter().any(|entry| {
            entry.event.event == "doc_created"
                && entry.event.doc_id == doc_id
                && entry.event.title == "Integration Report"
        }),
        "missing doc_created event: {session_events:?}"
    );

    let stream = store.bus().read_revisions(&doc_id).await.unwrap();
    assert_eq!(stream.len(), 1);
    assert_eq!(stream[0].event.revision.get(), 1);

    cleanup(&store, &doc_id).await;
    cleanup(&store, second_doc.doc_id.as_str()).await;
}

#[tokio::test]
async fn row_level_updates_only_touch_target_blocks_and_preserve_rank_order() {
    let store = test_store().await;
    let doc_id = unique("it-row");
    let doc = document(
        &doc_id,
        None,
        vec![
            paragraph("a", "Alpha"),
            paragraph("b", "Beta"),
            paragraph("c", "Gamma"),
        ],
    );
    store.create_document(&doc, "lead", None).await.unwrap();

    let result = store
        .apply_operations(
            &as_doc_id(&doc_id),
            as_revision(1),
            "writer-b",
            "patch beta",
            &[patch_text("b", "Beta updated")],
        )
        .await
        .unwrap();
    assert_eq!(result.new_revision.get(), 2);

    let rows = sqlx::query(
        r#"SELECT block_id, updated_rev, rank FROM report_blocks
           WHERE doc_id = $1 ORDER BY rank COLLATE "C""#,
    )
    .bind(&doc_id)
    .fetch_all(store.pool())
    .await
    .unwrap();
    let states: Vec<(String, i64, String)> = rows
        .iter()
        .map(|row| {
            (
                row.get::<String, _>("block_id"),
                row.get::<i64, _>("updated_rev"),
                row.get::<String, _>("rank"),
            )
        })
        .collect();
    assert_eq!(
        states
            .iter()
            .map(|(id, _, _)| id.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b", "c"]
    );
    assert_eq!(states[0].1, 1, "untouched block must keep updated_rev");
    assert_eq!(states[1].1, 2);
    assert_eq!(states[2].1, 1, "untouched block must keep updated_rev");
    let content = store
        .get_document(&DocId::parse(&doc_id).expect("doc id"))
        .await
        .unwrap();
    assert_eq!(content.block("b").unwrap().visible_text(), "Beta updated");

    cleanup(&store, &doc_id).await;
}

#[tokio::test]
async fn smart_cas_rebases_disjoint_parallel_writes_and_rejects_conflicts() {
    let store = test_store().await;
    let doc_id = unique("it-cas");
    let doc = document(
        &doc_id,
        None,
        vec![
            paragraph("a", "Alpha"),
            paragraph("b", "Beta"),
            paragraph("c", "Gamma"),
        ],
    );
    store.create_document(&doc, "lead", None).await.unwrap();

    let first_store = store.clone();
    let second_store = store.clone();
    let first_doc = as_doc_id(&doc_id);
    let second_doc = as_doc_id(&doc_id);
    let first = tokio::spawn(async move {
        first_store
            .apply_operations(
                &first_doc,
                as_revision(1),
                "agent-a",
                "edit beta",
                &[patch_text("b", "Beta from A")],
            )
            .await
    });
    let second = tokio::spawn(async move {
        second_store
            .apply_operations(
                &second_doc,
                as_revision(1),
                "agent-b",
                "edit gamma",
                &[patch_text("c", "Gamma from B")],
            )
            .await
    });
    let first = first.await.unwrap().unwrap();
    let second = second.await.unwrap().unwrap();
    let mut revisions = [first.new_revision, second.new_revision];
    revisions.sort_unstable();
    assert_eq!(
        revisions.map(|revision| revision.get()),
        [2, 3],
        "both disjoint writes must succeed"
    );

    let conflict = store
        .apply_operations(
            &as_doc_id(&doc_id),
            as_revision(1),
            "agent-c",
            "conflicting beta edit",
            &[patch_text("b", "Beta from C")],
        )
        .await
        .unwrap_err();
    match conflict {
        StoreError::Conflict(conflict) => {
            assert_eq!(conflict.current_revision.get(), 3);
            assert!(conflict.conflicting_block_ids.contains(&as_block_id("b")));
            assert!(
                conflict
                    .latest_blocks
                    .iter()
                    .any(|block| block.id == "b" && block.visible_text() == "Beta from A")
            );
        }
        other => panic!("expected RevisionConflict, got {other:?}"),
    }

    cleanup(&store, &doc_id).await;
}

#[tokio::test]
async fn rollback_replays_inverse_ops_and_advances_monotonically() {
    let store = test_store().await;
    let doc_id = unique("it-rollback");
    let doc = document(
        &doc_id,
        None,
        vec![paragraph("a", "Original A"), paragraph("b", "Original B")],
    );
    store.create_document(&doc, "lead", None).await.unwrap();
    store
        .apply_operations(
            &as_doc_id(&doc_id),
            as_revision(1),
            "agent-a",
            "edit a",
            &[patch_text("a", "Changed A")],
        )
        .await
        .unwrap();
    store
        .apply_operations(
            &as_doc_id(&doc_id),
            as_revision(2),
            "agent-b",
            "edit b",
            &[patch_text("b", "Changed B")],
        )
        .await
        .unwrap();

    let rollback = store
        .rollback_to_revision(&as_doc_id(&doc_id), as_revision(1), "reviewer")
        .await
        .unwrap();
    assert_eq!(rollback.new_revision, as_revision(4));
    assert_eq!(rollback.target_revision.get(), 1);
    assert_eq!(rollback.replayed_revisions, 2);

    let restored = store
        .get_document(&DocId::parse(&doc_id).expect("doc id"))
        .await
        .unwrap();
    assert_eq!(restored.revision, as_revision(4));
    assert_eq!(restored.block("a").unwrap().visible_text(), "Original A");
    assert_eq!(restored.block("b").unwrap().visible_text(), "Original B");
    let revisions = store.list_revisions(&as_doc_id(&doc_id), 20).await.unwrap();
    assert_eq!(
        revisions
            .iter()
            .map(|revision| revision.revision.get())
            .collect::<Vec<_>>(),
        vec![4, 3, 2, 1]
    );

    cleanup(&store, &doc_id).await;
}

#[tokio::test]
async fn redis_stream_receives_every_revision_in_order() {
    let store = test_store().await;
    let doc_id = unique("it-stream");
    let doc = document(&doc_id, None, vec![paragraph("a", "Alpha")]);
    store.create_document(&doc, "lead", None).await.unwrap();
    for revision_number in 1..=4 {
        let summary = format!("edit {revision_number}");
        let text = format!("Alpha {revision_number}");
        store
            .apply_operations(
                &as_doc_id(&doc_id),
                as_revision(revision_number),
                "agent",
                &summary,
                &[patch_text("a", &text)],
            )
            .await
            .unwrap();
    }
    // Give XADD calls a moment in case the publisher is still completing.
    let mut stream = store.bus().read_revisions(&doc_id).await.unwrap();
    for _ in 0..20 {
        if stream.len() >= 5 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        stream = store.bus().read_revisions(&doc_id).await.unwrap();
    }
    let revisions: Vec<i64> = stream
        .iter()
        .map(|entry| entry.event.revision.get())
        .collect();
    assert_eq!(revisions, vec![1, 2, 3, 4, 5]);
    assert!(
        stream
            .windows(2)
            .all(|pair| pair[0].event.revision < pair[1].event.revision)
    );

    cleanup(&store, &doc_id).await;
}

#[tokio::test]
async fn upsert_sources_deduplicates_url_and_normalized_title() {
    let store = test_store().await;
    let doc_id = unique("it-sources");
    let mut doc = document(&doc_id, None, vec![paragraph("intro", "Hello [^src_one]")]);
    doc.sources.clear();
    store.create_document(&doc, "lead", None).await.unwrap();

    let first = store
        .upsert_sources(
            &as_doc_id(&doc_id),
            "agent",
            vec![quire_store::SourceUpsert {
                source_id: as_source_id("src_one"),
                source: SourceEntry {
                    url: Some("https://example.com/report/".into()),
                    ..SourceEntry::new("Report")
                },
            }],
        )
        .await
        .unwrap();
    assert!(first.changed);
    let second = store
        .upsert_sources(
            &as_doc_id(&doc_id),
            "agent",
            vec![quire_store::SourceUpsert {
                source_id: as_source_id("src_two"),
                source: SourceEntry {
                    url: Some("https://example.com/report".into()),
                    ..SourceEntry::new("Same report")
                },
            }],
        )
        .await
        .unwrap();
    assert_eq!(
        second.canonical_id_map.get(&as_source_id("src_two")),
        Some(&as_source_id("src_one"))
    );
    let loaded = store
        .get_document(&DocId::parse(&doc_id).expect("doc id"))
        .await
        .unwrap();
    assert!(loaded.sources.contains_key("src_one"));
    assert!(!loaded.sources.contains_key("src_two"));

    cleanup(&store, &doc_id).await;
}

#[tokio::test]
async fn inline_sources_are_remapped_and_committed_with_the_batch() {
    let store = test_store().await;
    let doc_id = unique("it-inline-sources");
    let doc = document(&doc_id, None, vec![paragraph("intro", "Intro [^sec10]")]);
    store.create_document(&doc, "lead", None).await.unwrap();

    let cited = paragraph("analysis", "Analysis [^alias_sec10] and [^inline_new].")
        .with_sources([as_source_id("alias_sec10"), as_source_id("inline_new")]);
    let result = store
        .apply_operations_with_sources(
            &as_doc_id(&doc_id),
            as_revision(1),
            "agent",
            "append analysis with inline sources",
            &[PatchOp::Append {
                blocks: vec![cited],
            }],
            vec![
                quire_store::SourceUpsert {
                    source_id: as_source_id("alias_sec10"),
                    source: SourceEntry::new("Annual filing 10-K"),
                },
                quire_store::SourceUpsert {
                    source_id: as_source_id("inline_new"),
                    source: SourceEntry {
                        url: Some("https://example.com/fresh-data".into()),
                        ..SourceEntry::new("Fresh Data")
                    },
                },
            ],
        )
        .await
        .unwrap();
    assert_eq!(result.new_revision.get(), 2);
    assert!(result.inline_warnings.is_empty());

    let loaded = store
        .get_document(&DocId::parse(&doc_id).expect("doc id"))
        .await
        .unwrap();
    assert!(loaded.sources.contains_key("sec10"));
    assert!(loaded.sources.contains_key("inline_new"));
    assert!(!loaded.sources.contains_key("alias_sec10"));
    let analysis = loaded
        .blocks
        .iter()
        .find(|block| block.id.as_str() == "analysis")
        .expect("analysis block");
    let source_ids: Vec<&str> = analysis.source_ids.iter().map(SourceId::as_str).collect();
    assert_eq!(source_ids, vec!["sec10", "inline_new"]);

    cleanup(&store, &doc_id).await;
}

#[tokio::test]
async fn table_patch_and_ordered_load_work_against_real_rows() {
    let store = test_store().await;
    let doc_id = unique("it-table");
    let mut doc = document(&doc_id, None, vec![]);
    doc.blocks.push(Block::table(
        as_block_id("financials"),
        TableBlock {
            title: None,
            numbering: None,
            note: None,
            columns: vec![
                quire_protocol::ColumnNode::leaf("region", "Region"),
                quire_protocol::ColumnNode::leaf("revenue", "Revenue")
                    .with_type(quire_protocol::ColumnDataType::Currency),
            ],
            rows: vec![std::collections::BTreeMap::from([
                (
                    "region".to_string(),
                    quire_protocol::CellValue::text("APAC"),
                ),
                (
                    "revenue".to_string(),
                    quire_protocol::CellValue::Integer(10),
                ),
            ])],
            summary_rows: vec![],
        },
    ));
    store.create_document(&doc, "lead", None).await.unwrap();
    store
        .apply_operations(
            &as_doc_id(&doc_id),
            as_revision(1),
            "agent",
            "update cell",
            &[PatchOp::PatchBlock {
                block_id: as_block_id("financials"),
                patch: BlockPatch {
                    append_rows: vec![std::collections::BTreeMap::from([
                        (
                            "region".to_string(),
                            quire_protocol::CellValue::text("EMEA"),
                        ),
                        (
                            "revenue".to_string(),
                            quire_protocol::CellValue::Integer(20),
                        ),
                    ])],
                    style: Some(quire_protocol::BlockStylePatch {
                        column_widths: Some(std::collections::BTreeMap::from([(
                            "region".to_string(),
                            "30%".to_string(),
                        )])),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            }],
        )
        .await
        .unwrap();
    let loaded = store
        .get_document(&DocId::parse(&doc_id).expect("doc id"))
        .await
        .unwrap();
    let quire_protocol::BlockKind::Table(table) = &loaded.block("financials").unwrap().body else {
        panic!()
    };
    assert_eq!(table.rows.len(), 2);
    assert_eq!(
        loaded
            .block("financials")
            .unwrap()
            .style
            .column_widths
            .as_ref()
            .unwrap()["region"],
        "30%"
    );

    cleanup(&store, &doc_id).await;
}

#[tokio::test]
async fn snapshots_are_written_every_fifty_revisions() {
    let store = test_store().await;
    let doc_id = unique("it-snapshot");
    let doc = document(&doc_id, None, vec![paragraph("a", "Original")]);
    store.create_document(&doc, "lead", None).await.unwrap();
    for revision_number in 1..=49 {
        let text = format!("Version {revision_number}");
        store
            .apply_operations(
                &as_doc_id(&doc_id),
                as_revision(revision_number),
                "agent",
                "snapshot cadence",
                &[patch_text("a", &text)],
            )
            .await
            .unwrap();
    }
    let mut snapshot: Option<(i64, Vec<u8>)> = None;
    for _ in 0..100 {
        if let Some(row) = sqlx::query(
            "SELECT rev_num, snapshot_zstd FROM report_snapshots WHERE doc_id = $1 ORDER BY rev_num DESC LIMIT 1",
        )
        .bind(&doc_id)
        .fetch_optional(store.pool())
        .await
        .unwrap()
        {
            snapshot = Some((row.get::<i64, _>("rev_num"), row.get::<Vec<u8>, _>("snapshot_zstd")));
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let (revision, compressed) = snapshot.expect("revision 50 must trigger a snapshot");
    assert_eq!(revision, 50);
    let json = zstd::decode_all(compressed.as_slice()).unwrap();
    let document: ReportDocument = serde_json::from_slice(&json).unwrap();
    assert_eq!(document.revision, as_revision(50));
    assert_eq!(document.block("a").unwrap().visible_text(), "Version 49");

    cleanup(&store, &doc_id).await;
}

#[tokio::test]
async fn smart_cas_range_replacement_conflicts_with_inserts_inside_the_range() {
    let store = test_store().await;
    let doc_id = unique("it-range-cas");
    let doc = document(
        &doc_id,
        None,
        vec![
            paragraph("a", "Alpha"),
            paragraph("b", "Beta"),
            paragraph("c", "Gamma"),
        ],
    );
    store.create_document(&doc, "lead", None).await.unwrap();
    store
        .apply_operations(
            &as_doc_id(&doc_id),
            as_revision(1),
            "agent-insert",
            "insert inside range",
            &[PatchOp::InsertAfter {
                target_id: as_block_id("b"),
                blocks: vec![paragraph("x", "Inserted by another agent")],
            }],
        )
        .await
        .unwrap();
    let conflict = store
        .apply_operations(
            &as_doc_id(&doc_id),
            as_revision(1),
            "agent-replace",
            "replace a through c",
            &[PatchOp::ReplaceRange {
                start_id: as_block_id("a"),
                end_id: as_block_id("c"),
                blocks: vec![paragraph("replacement", "Replacement")],
            }],
        )
        .await
        .unwrap_err();
    match conflict {
        StoreError::Conflict(conflict) => {
            assert!(
                conflict.conflicting_block_ids.contains(&as_block_id("b"))
                    || conflict.conflicting_block_ids.contains(&as_block_id("x")),
                "conflict must include the intervening block: {conflict:?}"
            );
        }
        other => panic!("expected RevisionConflict, got {other:?}"),
    }
    let current = store
        .get_document(&DocId::parse(&doc_id).expect("doc id"))
        .await
        .unwrap();
    assert!(
        current.block("x").is_some(),
        "intervening insert must survive"
    );

    cleanup(&store, &doc_id).await;
}
