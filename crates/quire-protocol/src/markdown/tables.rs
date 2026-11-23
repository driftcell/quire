use super::*;

pub(crate) fn markdown_chart_option(chart: &EchartsBlock) -> serde_json::Value {
    let mut value = crate::echarts::merged_option(chart);
    let _ = strip_duplicate_inner_title(&mut value, chart.title.as_deref());
    value
}

pub(crate) fn write_markdown_table(output: &mut String, header: &[String], body: &[Vec<String>]) {
    output.push('|');
    for cell in header {
        let _ = write!(output, " {} |", escape_table_cell(cell));
    }
    output.push('\n');
    output.push('|');
    for index in 0..header.len() {
        output.push_str(if index == 0 { " :--- |" } else { " ---: |" });
    }
    output.push('\n');
    for row in body.iter().take(MAX_CHART_ROWS) {
        output.push('|');
        for (index, _) in header.iter().enumerate() {
            let cell = row.get(index).map(String::as_str).unwrap_or_default();
            let _ = write!(output, " {} |", escape_table_cell(cell));
        }
        output.push('\n');
    }
    if body.len() > MAX_CHART_ROWS {
        let mut cells = vec![String::new(); header.len()];
        if let Some(first) = cells.first_mut() {
            *first = "_truncated_".to_string();
        }
        if let Some(second) = cells.get_mut(1) {
            *second = format!("{} of {} rows shown", MAX_CHART_ROWS, body.len());
        }
        output.push('|');
        for cell in &cells {
            let _ = write!(output, " {} |", escape_table_cell(cell));
        }
        output.push('\n');
    }
    output.push('\n');
}

pub(crate) fn render_inline_nodes(nodes: &[InlineNode], document: &ReportDocument) -> String {
    let mut output = String::new();
    for node in nodes {
        match node {
            InlineNode::Text(text) => output.push_str(text),
            InlineNode::Strong(children) => {
                output.push_str("**");
                output.push_str(&render_inline_nodes(children, document));
                output.push_str("**");
            }
            InlineNode::Em(children) => {
                output.push('*');
                output.push_str(&render_inline_nodes(children, document));
                output.push('*');
            }
            InlineNode::Code(text) => {
                let fence = if text.contains('`') { "``" } else { "`" };
                output.push_str(fence);
                output.push_str(text);
                output.push_str(fence);
            }
            InlineNode::Link { href, children } => {
                output.push('[');
                output.push_str(&render_inline_nodes(children, document));
                output.push_str("](");
                output.push_str(href);
                output.push(')');
            }
            InlineNode::Citation(source_id) => {
                let _ = document;
                output.push_str(&format!("[^{source_id}]"));
            }
            InlineNode::Break => output.push_str("  \n"),
        }
    }
    output
}

pub(crate) fn escape_footnote(input: &str) -> String {
    input.replace(']', "\\]").replace('\n', " ")
}

pub(crate) fn escape_table_cell(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace(['\r', '\n'], "<br>")
}

pub(crate) fn block_id_list(document: &ReportDocument) -> String {
    if document.blocks.is_empty() {
        "(none)".to_string()
    } else {
        document
            .blocks
            .iter()
            .map(|block| block.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}
