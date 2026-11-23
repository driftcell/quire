pub use std::collections::BTreeMap;

pub use quire_lint::{
    LintOptions, lint_document, lint_document_with_options, lint_echarts_block,
    normalize::normalize_block,
};
pub use quire_protocol::{
    Block, BlockId, BlockKind, BlockStyle, CellValue, ColumnDataType, ColumnNode, CustomHtmlBlock,
    DocId, EchartsBlock, LintConfig, LintReport, ReportDocument, Severity, SourceEntry, SourceId,
    TableBlock,
};

pub fn doc() -> ReportDocument {
    ReportDocument::new(
        DocId::parse("lint-doc").unwrap_or_else(|error| panic!("valid lint doc id: {error}")),
    )
}

pub fn bid(raw: &str) -> BlockId {
    BlockId::parse(raw).unwrap_or_else(|error| panic!("valid block id `{raw}`: {error}"))
}

pub fn sid(raw: &str) -> SourceId {
    SourceId::parse(raw).unwrap_or_else(|error| panic!("valid source id `{raw}`: {error}"))
}

pub fn paragraph(id: &str, text: &str) -> Block {
    Block::paragraph(bid(id), text)
}

pub fn heading(id: &str, level: u8, text: &str) -> Block {
    Block::heading(bid(id), level, text)
}

pub fn table(
    id: &str,
    title: Option<&str>,
    columns: Vec<ColumnNode>,
    rows: Vec<BTreeMap<String, CellValue>>,
) -> Block {
    Block::table(
        bid(id),
        TableBlock {
            title: title.map(str::to_string),
            numbering: None,
            note: None,
            columns,
            rows,
            summary_rows: Vec::new(),
        },
    )
}

pub fn with_table_note(mut block: Block, note: &str) -> Block {
    if let BlockKind::Table(table) = &mut block.body {
        table.note = Some(note.into());
    }
    block
}

pub fn with_summary_row(mut block: Block, summary: BTreeMap<String, CellValue>) -> Block {
    if let BlockKind::Table(table) = &mut block.body {
        table.summary_rows.push(summary);
    }
    block
}

pub fn row(pairs: &[(&str, CellValue)]) -> BTreeMap<String, CellValue> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), value.clone()))
        .collect()
}

pub fn valid_document() -> ReportDocument {
    let mut document = doc();
    document
        .sources
        .insert(sid("sec10"), SourceEntry::new("Annual filing"));
    document.blocks.push(heading("intro", 1, "Introduction"));
    document
        .blocks
        .push(paragraph("summary", "Revenue grew 18% [^sec10]."));
    let mut data_table = table(
        "table",
        None,
        vec![
            ColumnNode::leaf("region", "Region"),
            ColumnNode::leaf("value", "Value（项）"),
        ],
        vec![
            row(&[
                ("region", CellValue::text("APAC")),
                ("value", CellValue::Integer(10)),
            ]),
            row(&[
                ("region", CellValue::text("EMEA")),
                ("value", CellValue::Integer(20)),
            ]),
        ],
    );
    data_table.source_note = Some("Annual filing".into());
    data_table.source_ids = vec![sid("sec10")];
    document.blocks.push(data_table);
    document
}

pub fn codes(report: &LintReport) -> Vec<&str> {
    report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect()
}

pub fn has_code(report: &LintReport, code: &str) -> bool {
    codes(report).contains(&code)
}

pub fn finding<'a>(report: &'a LintReport, code: &str) -> &'a quire_protocol::Diagnostic {
    report
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == code)
        .unwrap_or_else(|| panic!("missing {code}: {:?}", report.diagnostics))
}

pub fn chart(option: serde_json::Value) -> Block {
    chart_named("chart", option)
}

pub fn chart_named(id: &str, option: serde_json::Value) -> Block {
    Block::new(
        bid(id),
        BlockKind::Echarts(EchartsBlock {
            title: Some("Revenue".into()),
            height: 320,
            option,
            option_patch: None,
            fallback_summary: "Revenue rose.".into(),
            numbering: None,
            note: None,
        }),
    )
}

pub fn custom_html(html: &str, fallback: &str) -> Block {
    Block::new(
        bid("html"),
        BlockKind::CustomHtml(CustomHtmlBlock {
            title: Some("KPI Grid".into()),
            numbering: None,
            note: None,
            html: html.into(),
            css: None,
            fallback_text: fallback.into(),
        }),
    )
}
