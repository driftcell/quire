use std::collections::BTreeMap;

use indexmap::IndexMap;
use quire_protocol::{BlockId, Diagnostic, LintSummary, SourceEntry, SourceId};

use super::*;

#[test]
fn read_as_markdown_renders_unencoded_markdown_and_hint() {
    let receipt = contracts::ReadMarkdownReceipt {
        source: "doc:demo".into(),
        doc_id: "demo".into(),
        revision: 3,
        markdown_chars: 42,
        markdown: "## 章节标题\n\n正文包含 \"引号\" 与 <citation id=\"src_1\"/>。".into(),
        _next_hint: "Patch only changed blocks.".into(),
    };
    let rendered = render_read_as_markdown(&receipt).expect("render read_as_markdown");
    assert!(rendered.contains("doc_id: demo | rev: 3 | chars: 42"));
    assert!(rendered.contains("\"引号\""));
    assert!(rendered.contains("<citation id=\"src_1\"/>"));
    assert!(rendered.contains("> Next: Patch only changed blocks."));
    assert!(!rendered.contains("\\n"));
}

#[test]
fn lint_document_renders_clean_and_diagnostic_states() {
    let clean = contracts::LintReceipt {
        ok: true,
        rev: 5,
        summary: None,
        suppressed_blocks: BTreeMap::new(),
        diagnostics: Vec::new(),
        _next_hint: None,
    };
    let clean_text = render_lint_document(&clean).expect("render clean lint");
    assert_eq!(clean_text.trim(), "OK — rev=5, 0 diagnostics.");

    let dirty = contracts::LintReceipt {
        ok: false,
        rev: 6,
        summary: Some(LintSummary {
            errors: 1,
            warnings: 0,
            infos: 0,
            suppressed: 0,
        }),
        suppressed_blocks: BTreeMap::new(),
        diagnostics: vec![Diagnostic {
            code: "QR201".into(),
            rule: "missing-source".into(),
            severity: Severity::Error,
            message: "Unregistered source".into(),
            hint: "Call upsert_sources first.".into(),
            suppressible: false,
            block_id: Some(BlockId::parse("sec_1").expect("valid block id")),
            path: None,
            valid_values: vec!["src_a".into()],
        }],
        _next_hint: Some("Fix errors first.".into()),
    };
    let dirty_text = render_lint_document(&dirty).expect("render dirty lint");
    assert!(dirty_text.contains("[error] QR201 (missing-source) @sec_1"));
    assert!(dirty_text.contains("Call upsert_sources first."));
    assert!(dirty_text.contains("> Next: Fix errors first."));

    let grouped = contracts::LintReceipt {
        ok: true,
        rev: 7,
        summary: Some(LintSummary {
            errors: 0,
            warnings: 2,
            infos: 0,
            suppressed: 0,
        }),
        suppressed_blocks: BTreeMap::new(),
        diagnostics: vec![
            Diagnostic {
                code: "QR102".into(),
                rule: "unused-source".into(),
                severity: Severity::Warning,
                message: "source `src_a` is registered but never cited".into(),
                hint: "add [^<source_id>] to prose".into(),
                suppressible: true,
                block_id: None,
                path: Some("sources.src_a".into()),
                valid_values: Vec::new(),
            },
            Diagnostic {
                code: "QR102".into(),
                rule: "unused-source".into(),
                severity: Severity::Warning,
                message: "source `src_b` is registered but never cited".into(),
                hint: "add [^<source_id>] to prose".into(),
                suppressible: true,
                block_id: None,
                path: Some("sources.src_b".into()),
                valid_values: Vec::new(),
            },
        ],
        _next_hint: None,
    };
    let grouped_text = render_lint_document(&grouped).expect("render grouped lint");
    assert!(grouped_text.contains("[warning] QR102 (unused-source) × 2"));
    assert!(grouped_text.contains("Targets: src_a, src_b"));
    assert!(!grouped_text.contains("registered but never cited"));
    assert_eq!(
        grouped_text.matches("add [^<source_id>] to prose").count(),
        1
    );

    let mut suppressed_blocks = BTreeMap::new();
    suppressed_blocks.insert("c12_p05".into(), vec!["QR423".into()]);
    suppressed_blocks.insert("c21_p01".into(), vec!["QR423".into()]);
    let suppressed_only = contracts::LintReceipt {
        ok: true,
        rev: 70,
        summary: Some(LintSummary {
            errors: 0,
            warnings: 0,
            infos: 0,
            suppressed: 2,
        }),
        suppressed_blocks,
        diagnostics: Vec::new(),
        _next_hint: None,
    };
    let sup_text = render_lint_document(&suppressed_only).expect("render suppressed lint");
    assert!(sup_text.contains("QR423 (higher-ed-policy-term-format) × 2: c12_p05, c21_p01"));
}

#[test]
fn upsert_list_sources_and_apply_operations_render_compactly() {
    let mut canonical_id_map = BTreeMap::new();
    canonical_id_map.insert("a".into(), "a".into());
    canonical_id_map.insert("b_dup".into(), "b".into());
    let upsert = contracts::UpsertSourcesReceipt {
        doc_id: "doc_1".into(),
        revision: 4,
        changed: true,
        canonical_id_map,
        source_count: 2,
        inline_warnings: Vec::new(),
        _next_hint: "Cite sources.".into(),
    };
    let up_text = render_upsert_sources(&upsert).expect("render upsert_sources");
    assert!(up_text.contains("OK — doc_1 rev=4 (changed=true, total=2)"));
    assert!(up_text.contains("Mapped: b_dup->b"));

    let mut sources = IndexMap::new();
    sources.insert(
        SourceId::parse("moe_26").unwrap(),
        SourceEntry::new("教育部工作要点"),
    );
    let list = contracts::ListSourcesReceipt {
        doc_id: "doc_1".into(),
        revision: 4,
        sources,
        count: 1,
        _next_hint: "Verify claims.".into(),
    };
    let list_text = render_list_sources(&list).expect("render list_sources");
    assert!(list_text.contains("- [^moe_26] 教育部工作要点"));

    let apply = contracts::ApplyOperationsReceipt {
        new_revision: 5,
        base_revision: 4,
        rebased: false,
        rebased_over: 0,
        committed: true,
        assigned_ids: Vec::new(),
        inline_warnings: Vec::new(),
        blocking_errors: Vec::new(),
        _next_hint: "Continue.".into(),
    };
    let apply_text = render_apply_operations(&apply).expect("render apply_operations");
    assert!(apply_text.contains("OK — rev 4 -> 5"));

    let del = contracts::DeleteSourcesReceipt {
        doc_id: "doc_1".into(),
        revision: 6,
        changed: true,
        deleted_ids: vec!["old_src".into()],
        source_count: 1,
        _next_hint: "Run lint_document.".into(),
    };
    let del_text = render_delete_sources(&del).expect("render delete_sources");
    assert!(del_text.contains("OK — doc_1 rev=6 (changed=true, deleted=1, remaining=1)"));
    assert!(del_text.contains("Deleted: old_src"));
}
