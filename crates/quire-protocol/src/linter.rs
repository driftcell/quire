//! Static, zero-browser document validation with self-healing diagnostics.

use std::collections::{BTreeSet, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    block::{
        BlockKind, CustomHtmlBlock, EchartsBlock, TableBlock, is_semantic_block_id,
        looks_like_uuid_v4,
    },
    citation::{CitationLocation, scan_document_citations},
    echarts::{
        axis_count, configured_axis_count, entries, find_js_function_string, has_data_source,
        inner_title_duplicates, is_center_inner_title, is_positioned_or_light_inner_title,
        is_valid_series_type, merged_option, series as chart_series,
    },
    ids::BlockId,
    model::{ReportDocument, is_safe_css_token_value, is_valid_css_token_name},
    table::{TableError, unknown_row_keys, unknown_width_overrides, validate_table},
    text::css_brace_issue,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    /// Actionable next step, including valid IDs/enums when possible.
    pub hint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<BlockId>,
    /// JSON-ish path such as `blocks.financials.columns`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub valid_values: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LintSummary {
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LintReport {
    pub diagnostics: Vec<Diagnostic>,
    pub summary: LintSummary,
}

impl LintReport {
    pub fn has_errors(&self) -> bool {
        self.summary.errors > 0
    }

    pub fn is_clean(&self) -> bool {
        self.diagnostics.is_empty()
    }

    pub fn error_codes(&self) -> Vec<&str> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Error)
            .map(|diagnostic| diagnostic.code.as_str())
            .collect()
    }
}

/// Run all static protocol checks.
pub fn lint_document(document: &ReportDocument) -> LintReport {
    let mut linter = Linter {
        document,
        diagnostics: Vec::new(),
    };
    linter.check_document_identity();
    linter.check_blocks();
    linter.check_citations();
    linter.check_theme();
    linter.check_toc();
    linter.check_cover();
    linter.check_sources();
    linter.check_empty_document();
    linter.finish()
}

/// Validate one ECharts block in isolation before it is written to a report.
///
/// Preview tools use this to surface `ECHARTS_DATA_MISSING`, empty fallback summaries, invalid
/// series types and other chart diagnostics without mutating a document.
pub fn lint_echarts_block(block_id: BlockId, chart: &EchartsBlock) -> LintReport {
    let Ok(doc_id) = crate::ids::DocId::parse("chart-preview") else {
        return LintReport::default();
    };
    let mut document = ReportDocument::new(doc_id);
    document.blocks.push(crate::block::Block::new(
        block_id,
        BlockKind::Echarts(chart.clone()),
    ));
    lint_document(&document)
}

struct Linter<'a> {
    document: &'a ReportDocument,
    diagnostics: Vec<Diagnostic>,
}

impl Linter<'_> {
    fn error(
        &mut self,
        code: &str,
        message: impl Into<String>,
        hint: impl Into<String>,
        block_id: Option<BlockId>,
        path: Option<String>,
    ) {
        self.diagnostics.push(Diagnostic {
            code: code.to_string(),
            severity: Severity::Error,
            message: message.into(),
            hint: hint.into(),
            block_id,
            path,
            valid_values: Vec::new(),
        });
    }

    fn warning(
        &mut self,
        code: &str,
        message: impl Into<String>,
        hint: impl Into<String>,
        block_id: Option<BlockId>,
        path: Option<String>,
    ) {
        self.diagnostics.push(Diagnostic {
            code: code.to_string(),
            severity: Severity::Warning,
            message: message.into(),
            hint: hint.into(),
            block_id,
            path,
            valid_values: Vec::new(),
        });
    }

    fn finish(mut self) -> LintReport {
        self.diagnostics.sort_by(|left, right| {
            severity_rank(left.severity)
                .cmp(&severity_rank(right.severity))
                .then_with(|| left.code.cmp(&right.code))
                .then_with(|| left.block_id.cmp(&right.block_id))
                .then_with(|| left.message.cmp(&right.message))
        });
        let mut summary = LintSummary::default();
        for diagnostic in &self.diagnostics {
            match diagnostic.severity {
                Severity::Error => summary.errors += 1,
                Severity::Warning => summary.warnings += 1,
                Severity::Info => summary.infos += 1,
            }
        }
        LintReport {
            diagnostics: self.diagnostics,
            summary,
        }
    }

    fn check_document_identity(&mut self) {
        if self.document.doc_id.trim().is_empty() {
            self.error(
                "DOC_ID_EMPTY",
                "document `doc_id` is empty",
                "create or load the report with a non-empty semantic doc_id",
                None,
                Some("doc_id".into()),
            );
        }
    }

    fn check_empty_document(&mut self) {
        if self.document.blocks.is_empty() {
            self.warning(
                "EMPTY_DOCUMENT",
                "document has no blocks",
                "use apply_operations with Append or InsertAfter to add content",
                None,
                Some("blocks".into()),
            );
        }
    }

    fn check_blocks(&mut self) {
        let mut seen = HashSet::new();
        let mut previous_heading_level: Option<u8> = None;
        for (index, block) in self.document.blocks.iter().enumerate() {
            if !seen.insert(block.id.as_str()) {
                self.error(
                    "DUPLICATE_BLOCK_ID",
                    format!("duplicate block id `{}`", block.id),
                    format!(
                        "rename one of the duplicate blocks. Existing block_ids: {}",
                        block_ids(self.document)
                    ),
                    Some(block.id.clone()),
                    Some(format!("blocks[{index}].id")),
                );
            }
            if !is_semantic_block_id(&block.id) {
                self.error(
                    "INVALID_BLOCK_ID",
                    format!("block id `{}` is not a short semantic id", block.id),
                    "use 1-64 ASCII letters/digits and '-' '_' '.', starting alphanumeric; never UUIDv4",
                    Some(block.id.clone()),
                    Some(format!("blocks[{index}].id")),
                );
            } else if looks_like_uuid_v4(&block.id) {
                self.error(
                    "BLOCK_ID_UUID_V4",
                    format!("block id `{}` is a UUIDv4", block.id),
                    "replace it with a short semantic id such as `sec_intro` or `b_01`",
                    Some(block.id.clone()),
                    Some(format!("blocks[{index}].id")),
                );
            }

            match &block.body {
                BlockKind::Heading { level, text } => {
                    if !(1..=6).contains(level) {
                        self.error(
                            "HEADING_LEVEL_RANGE",
                            format!("heading `{}` has level {level}", block.id),
                            "heading level must be 1..=6",
                            Some(block.id.clone()),
                            Some(format!("blocks[{index}].level")),
                        );
                    }
                    if text.trim().is_empty() {
                        self.warning(
                            "EMPTY_HEADING",
                            format!("heading `{}` has no text", block.id),
                            "add section text or remove the heading",
                            Some(block.id.clone()),
                            Some(format!("blocks[{index}].text")),
                        );
                    }
                    if let Some(previous) = previous_heading_level
                        && *level > previous + 1
                    {
                        self.warning(
                            "HEADING_LEVEL_JUMP",
                            format!("heading `{}` jumps from h{previous} to h{level}", block.id),
                            "insert an intermediate heading or reduce the level by one",
                            Some(block.id.clone()),
                            Some(format!("blocks[{index}].level")),
                        );
                    }
                    previous_heading_level = Some(*level);
                }
                BlockKind::Paragraph { text } | BlockKind::Quote { text } => {
                    if text.trim().is_empty() {
                        self.warning(
                            "EMPTY_TEXT_BLOCK",
                            format!("{} `{}` has no text", block.block_type(), block.id),
                            "supply text or delete the block",
                            Some(block.id.clone()),
                            Some(format!("blocks[{index}].text")),
                        );
                    }
                }
                BlockKind::Table(table) => {
                    self.check_visual_title(block, table.title.as_deref(), index);
                    self.check_table_values(block, table, index);
                }
                BlockKind::Echarts(chart) => {
                    self.check_visual_title(block, chart.title.as_deref(), index);
                    self.check_inner_chart_title(block, chart, index);
                    self.check_chart(block, chart, index);
                }
                BlockKind::CustomHtml(html) => {
                    self.check_visual_title(block, html.title.as_deref(), index);
                    self.check_duplicate_html_title(block, html, index);
                    self.check_custom_html(block, html, index);
                }
            }
        }
    }

    fn check_citations(&mut self) {
        let scan = scan_document_citations(self.document);
        let mut seen = BTreeSet::new();
        let valid_ids = source_ids(self.document);
        for usage in scan.dangling {
            let key = (
                usage.block_id.clone(),
                usage.source_id.clone(),
                usage.location,
            );
            if !seen.insert(key.clone()) {
                continue;
            }
            let location = match usage.location {
                CitationLocation::Inline => "inline citation",
                CitationLocation::Block => "block-level source_ids",
            };
            self.error(
                "DANGLING_SOURCE_ID",
                format!(
                    "{} `{}` in block `{}` is not registered. Valid source_ids: {}",
                    location, usage.source_id, usage.block_id, valid_ids
                ),
                format!(
                    "register it with upsert_sources, or change the marker to one of: {}",
                    valid_ids
                ),
                Some(usage.block_id),
                Some(format!("sources.{}", usage.source_id)),
            );
        }

        let cited: BTreeSet<crate::ids::SourceId> = scan
            .uses
            .iter()
            .map(|usage| usage.source_id.clone())
            .collect();
        for source_id in self.document.sources.keys() {
            if !cited.contains(source_id) {
                self.warning(
                    "UNUSED_SOURCE",
                    format!("source `{source_id}` is registered but never cited"),
                    format!(
                        "add [^{source_id}] to prose or include `{source_id}` in a block source_ids array; valid source_ids: {}",
                        valid_ids
                    ),
                    None,
                    Some(format!("sources.{source_id}")),
                );
            }
        }
    }

    fn check_table_values(
        &mut self,
        block: &crate::block::Block,
        table: &TableBlock,
        index: usize,
    ) {
        match validate_table(table) {
            Ok(shape) => {
                let unknown = unknown_row_keys(table, &shape);
                if !unknown.is_empty() {
                    let valid = shape
                        .leaves
                        .iter()
                        .map(|leaf| leaf.key.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    self.warning(
                        "TABLE_UNKNOWN_ROW_KEY",
                        format!(
                            "table `{}` has row keys not present in leaf columns: {}",
                            block.id,
                            unknown.join(", ")
                        ),
                        format!("valid leaf keys: {valid}"),
                        Some(block.id.clone()),
                        Some(format!("blocks[{index}].rows")),
                    );
                }
                let unknown_widths = unknown_width_overrides(&block.style, &shape);
                if !unknown_widths.is_empty() {
                    self.warning(
                        "TABLE_UNKNOWN_WIDTH_OVERRIDE",
                        format!(
                            "table `{}` has column_widths for unknown leaves: {}",
                            block.id,
                            unknown_widths.join(", ")
                        ),
                        format!(
                            "valid leaf keys: {}",
                            shape
                                .leaves
                                .iter()
                                .map(|leaf| leaf.key.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        Some(block.id.clone()),
                        Some(format!("blocks[{index}].style.column_widths")),
                    );
                }
            }
            Err(error) => {
                let hint = match &error {
                    TableError::MissingLeafKey { .. } | TableError::EmptyLeafKey { .. } => {
                        "add a unique short `key` to every leaf column; groups may omit keys"
                            .to_string()
                    }
                    TableError::DuplicateLeafKey { .. } => {
                        "rename one duplicate leaf column key".to_string()
                    }
                    TableError::EmptyColumns => {
                        "declare at least one leaf ColumnNode with a key".to_string()
                    }
                    TableError::EmptyGroup { .. } => {
                        "remove the empty group or add child columns".to_string()
                    }
                };
                self.error(
                    "TABLE_SCHEMA_INVALID",
                    format!("table `{}`: {error}", block.id),
                    hint,
                    Some(block.id.clone()),
                    Some(format!("blocks[{index}].columns")),
                );
            }
        }
    }

    fn check_visual_title(
        &mut self,
        block: &crate::block::Block,
        title: Option<&str>,
        index: usize,
    ) {
        let Some(title) = title.map(str::trim).filter(|title| !title.is_empty()) else {
            return;
        };
        if has_manual_number_prefix(title) {
            self.warning(
                "MANUAL_PREFIX_IN_TITLE",
                format!(
                    "block `{}` title starts with hardcoded numbering: `{title}`",
                    block.id
                ),
                "Put the bare title in outer `title` ONLY; numbering is generated automatically (表 N / 图 N).",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].title")),
            );
        }
    }

    fn check_inner_chart_title(
        &mut self,
        block: &crate::block::Block,
        chart: &EchartsBlock,
        index: usize,
    ) {
        let Some(outer_title) = chart
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
        else {
            return;
        };
        let option = merged_option(chart);
        let Some(title) = option.get("title") else {
            return;
        };
        let is_array = title.is_array();
        let titles = title
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_else(|| std::slice::from_ref(title));
        let duplicate = titles
            .iter()
            .any(|entry| inner_title_duplicates(entry, outer_title));
        let allowed = is_array
            || titles.iter().all(|entry| {
                is_center_inner_title(entry) || is_positioned_or_light_inner_title(entry)
            });
        if duplicate || !allowed {
            let detail = if duplicate {
                "repeats its outer title inside `option.title`"
            } else {
                "has a top-aligned inner ECharts title that is not a positioned subplot label"
            };
            self.warning(
                "DUPLICATE_INNER_TITLE",
                format!("chart `{}` {detail}", block.id),
                "Keep the main title in outer `title` only. For multi-panel charts, use an `option.title` array or light positioned subtitles (left/right/bottom or fontSize <= 13) that do not repeat the outer title.",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].option.title")),
            );
        }
    }

    fn check_duplicate_html_title(
        &mut self,
        block: &crate::block::Block,
        html: &CustomHtmlBlock,
        index: usize,
    ) {
        let Some(title) = html
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
        else {
            return;
        };
        if html_has_duplicate_title(&html.html, title) {
            self.warning(
                "DUPLICATE_INNER_TITLE",
                format!(
                    "custom_html block `{}` repeats its outer title inside a leading heading/.title element",
                    block.id
                ),
                "Put titles in outer `title` ONLY; remove the duplicate heading/title element from the HTML.",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].html")),
            );
        }
    }

    fn check_chart(&mut self, block: &crate::block::Block, chart: &EchartsBlock, index: usize) {
        if chart.fallback_summary.trim().is_empty() {
            self.error(
                "ECHARTS_FALLBACK_EMPTY",
                format!("chart `{}` has an empty fallback_summary", block.id),
                "provide a one-sentence numeric summary for non-JS agents and accessibility",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].fallback_summary")),
            );
        }
        if !(120..=2000).contains(&chart.height) {
            self.warning(
                "ECHARTS_HEIGHT_RANGE",
                format!(
                    "chart `{}` height {} is outside the recommended 120..=2000",
                    block.id, chart.height
                ),
                "use a height between 120 and 2000 pixels",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].height")),
            );
        }
        if chart.option.is_null() {
            self.error(
                "ECHARTS_OPTION_MISSING",
                format!("chart `{}` has no native ECharts `option`", block.id),
                "Provide the complete ECharts 6.0 option object; presets and mode switches are not supported.",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].option")),
            );
        }
        for (field, value) in [
            ("option", Some(&chart.option)),
            ("option_patch", chart.option_patch.as_ref()),
        ] {
            if let Some(value) = value
                && !value.is_null()
                && !value.is_object()
            {
                self.error(
                    "ECHARTS_OPTION_NOT_OBJECT",
                    format!("chart `{}` `{field}` must be a JSON object", block.id),
                    "send an ECharts option object, for example {\"series\":[{\"data\":[1,2]}]}",
                    Some(block.id.clone()),
                    Some(format!("blocks[{index}].{field}")),
                );
            }
        }
        for (field, value) in [
            ("option", Some(&chart.option)),
            ("option_patch", chart.option_patch.as_ref()),
        ] {
            let Some(value) = value else {
                continue;
            };
            if let Some(path) = find_js_function_string(value, field) {
                self.error(
                    "ECHARTS_JS_FUNCTION_STRING",
                    format!(
                        "chart `{}` contains a JavaScript callback string at `{path}`",
                        block.id
                    ),
                    "Remove JS callbacks. Use ECharts string templates instead, for example \"{b}: {c} ({d}%)\"; callbacks must live in frontend renderer code, not canonical JSON.",
                    Some(block.id.clone()),
                    Some(format!("blocks[{index}].{path}")),
                );
            }
        }
        let option = merged_option(chart);
        let has_matrix = option.get("matrix").is_some_and(|value| !value.is_null());
        for (axis_key, axis_path) in [("xAxis", "xAxis"), ("yAxis", "yAxis")] {
            let grid_count = i64::try_from(axis_count(&option, "grid")).unwrap_or(i64::MAX);
            for (axis_index, axis) in entries(option.get(axis_key)).iter().enumerate() {
                let Some(grid_index) = axis.get("gridIndex").and_then(serde_json::Value::as_i64)
                else {
                    continue;
                };
                if grid_index < 0 || grid_index >= grid_count {
                    self.error(
                        "ECHARTS_INDEX_OUT_OF_RANGE",
                        format!(
                            "chart `{}` {axis_key}[{axis_index}].gridIndex is {grid_index}, but only {grid_count} `grid` entr{} exist",
                            block.id,
                            if grid_count == 1 { "y" } else { "ies" }
                        ),
                        format!(
                            "Use a gridIndex in 0..={}. Bind each axis array entry to its matching `grid` and keep series xAxisIndex/yAxisIndex in range.",
                            grid_count.saturating_sub(1)
                        ),
                        Some(block.id.clone()),
                        Some(format!("blocks[{index}].{axis_path}[{axis_index}].gridIndex")),
                    );
                }
            }
        }
        for (series_index, series) in chart_series(&option).into_iter().enumerate() {
            let kind = series.get("type").and_then(serde_json::Value::as_str);
            match kind {
                Some(kind) if is_valid_series_type(kind) => {}
                Some(kind) => {
                    self.error(
                        "ECHARTS_SERIES_TYPE_INVALID",
                        format!(
                            "chart `{}` series[{series_index}] has unsupported type `{kind}`",
                            block.id
                        ),
                        "Use an ECharts 6.0 series type such as line, bar, pie, radar, sankey, chord, beeswarm or matrix, or a registered extension such as wordCloud/liquidFill/bar3D.",
                        Some(block.id.clone()),
                        Some(format!("blocks[{index}].series[{series_index}].type")),
                    );
                }
                None => {
                    self.error(
                        "ECHARTS_SERIES_TYPE_INVALID",
                        format!(
                            "chart `{}` series[{series_index}] is missing a string `type`",
                            block.id
                        ),
                        "Every series needs an explicit ECharts 6.0 type, for example `\"type\":\"bar\"`.",
                        Some(block.id.clone()),
                        Some(format!("blocks[{index}].series[{series_index}].type")),
                    );
                }
            }
            if series
                .get("coordinateSystem")
                .and_then(serde_json::Value::as_str)
                == Some("matrix")
                && !has_matrix
            {
                self.error(
                    "ECHARTS_MATRIX_MISSING",
                    format!(
                        "chart `{}` series[{series_index}] uses the matrix coordinate system but has no top-level `matrix`",
                        block.id
                    ),
                    "Add the ECharts 6.0 `matrix` configuration (rows/columns/cells) or remove coordinateSystem=\"matrix\".",
                    Some(block.id.clone()),
                    Some(format!("blocks[{index}].series[{series_index}].coordinateSystem")),
                );
            }
            for (field, axis_key) in [
                ("xAxisIndex", "xAxis"),
                ("yAxisIndex", "yAxis"),
                ("gridIndex", "grid"),
                ("datasetIndex", "dataset"),
            ] {
                let Some(raw_index) = series.get(field).and_then(serde_json::Value::as_i64) else {
                    continue;
                };
                let count = i64::try_from(axis_count(&option, axis_key)).unwrap_or(i64::MAX);
                if raw_index < 0 || raw_index >= count {
                    self.error(
                        "ECHARTS_INDEX_OUT_OF_RANGE",
                        format!(
                            "chart `{}` series[{series_index}].{field} is {raw_index}, but only {count} `{axis_key}` entr{} exist",
                            block.id,
                            if count == 1 { "y" } else { "ies" }
                        ),
                        format!(
                            "Use an index in 0..={}. Add another `{axis_key}` entry only when the chart truly needs it.",
                            count.saturating_sub(1)
                        ),
                        Some(block.id.clone()),
                        Some(format!("blocks[{index}].series[{series_index}].{field}")),
                    );
                }
            }

            let coordinate_system = series
                .get("coordinateSystem")
                .and_then(serde_json::Value::as_str);
            let needs_radar = kind == Some("radar")
                || series.get("radarIndex").is_some()
                || coordinate_system == Some("radar");
            let needs_polar =
                series.get("polarIndex").is_some() || coordinate_system == Some("polar");
            for (component, field, needed) in [
                ("radar", "radarIndex", needs_radar),
                ("polar", "polarIndex", needs_polar),
            ] {
                if !needed {
                    continue;
                }
                let Some(count) = configured_axis_count(&option, component) else {
                    self.error(
                        "ECHARTS_COORDINATE_MISSING",
                        format!(
                            "chart `{}` series[{series_index}] uses `{component}` coordinates but has no top-level `{component}`",
                            block.id
                        ),
                        format!(
                            "Add a `{component}` object/array to the option (for radar, include `indicator`), or remove the `{field}`/coordinateSystem usage."
                        ),
                        Some(block.id.clone()),
                        Some(format!("blocks[{index}].{component}")),
                    );
                    continue;
                };
                let raw_index = series
                    .get(field)
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0);
                let count = i64::try_from(count).unwrap_or(i64::MAX);
                if raw_index < 0 || raw_index >= count {
                    self.error(
                        "ECHARTS_INDEX_OUT_OF_RANGE",
                        format!(
                            "chart `{}` series[{series_index}].{field} is {raw_index}, but only {count} `{component}` entr{} exist",
                            block.id,
                            if count == 1 { "y" } else { "ies" }
                        ),
                        format!(
                            "Use an index in 0..={}, or add another `{component}` configuration.",
                            count.saturating_sub(1)
                        ),
                        Some(block.id.clone()),
                        Some(format!("blocks[{index}].series[{series_index}].{field}")),
                    );
                }
            }
        }
        if !has_data_source(&option) {
            self.error(
                "ECHARTS_DATA_MISSING",
                format!("chart `{}` has no non-empty data source", block.id),
                "Supply series[*].data, series[*].links/edges, or a non-empty dataset.source so reviewers can verify the chart.",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].option")),
            );
        }
    }

    fn check_custom_html(
        &mut self,
        block: &crate::block::Block,
        html: &CustomHtmlBlock,
        index: usize,
    ) {
        if html.fallback_text.trim().is_empty() {
            self.error(
                "CUSTOM_HTML_FALLBACK_EMPTY",
                format!("custom_html block `{}` has empty fallback_text", block.id),
                "provide a non-empty Markdown/plain-text fallback for agents and accessibility",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].fallback_text")),
            );
        }
        if html.html.trim().is_empty() {
            self.warning(
                "CUSTOM_HTML_EMPTY",
                format!("custom_html block `{}` has empty html", block.id),
                "supply raw HTML or remove the block",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].html")),
            );
        }
        if let Some(css) = html.css.as_deref()
            && let Some(issue) = css_brace_issue(css)
        {
            self.error(
                "CUSTOM_HTML_CSS_SYNTAX",
                format!(
                    "custom_html block `{}` CSS error at line {}, column {}: {}",
                    block.id, issue.line, issue.column, issue.message
                ),
                "balance braces and close strings/comments before saving",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].css")),
            );
        }
    }

    fn check_theme(&mut self) {
        if let Some(issue) = css_brace_issue(&self.document.theme.custom_css) {
            self.error(
                "THEME_CSS_SYNTAX",
                format!(
                    "theme.custom_css error at line {}, column {}: {}",
                    issue.line, issue.column, issue.message
                ),
                "fix the reported brace/string/comment and retry update_style_and_css",
                None,
                Some("theme.custom_css".into()),
            );
        }
        for (name, value) in &self.document.theme.tokens {
            if !is_valid_css_token_name(name) {
                self.warning(
                    "THEME_TOKEN_NAME",
                    format!("theme token `{name}` is not a valid CSS custom property"),
                    "token names must start with `--` and contain only ASCII letters, digits, '-' or '_'",
                    None,
                    Some(format!("theme.tokens.{name}")),
                );
            } else if !is_safe_css_token_value(value) {
                self.warning(
                    "THEME_TOKEN_VALUE",
                    format!("theme token `{name}` has an unsafe value"),
                    "avoid `;`, `{`, `}`, `<`, `>` and backslashes in token values",
                    None,
                    Some(format!("theme.tokens.{name}")),
                );
            }
        }
        if self.document.theme.preset.trim().is_empty() {
            self.warning(
                "THEME_PRESET_EMPTY",
                "theme preset is empty",
                "use `default`, `editorial`, `technical`, or a frontend-supported preset name",
                None,
                Some("theme.preset".into()),
            );
        }
    }

    fn check_toc(&mut self) {
        if !(1..=3).contains(&self.document.toc.max_depth) {
            self.error(
                "TOC_DEPTH_RANGE",
                format!(
                    "toc.max_depth is {} but the protocol allows 1..=3",
                    self.document.toc.max_depth
                ),
                "set max_depth to 1, 2 or 3",
                None,
                Some("toc.max_depth".into()),
            );
        }
        if self.document.toc.enabled && self.document.toc.title.trim().is_empty() {
            self.warning(
                "TOC_TITLE_EMPTY",
                "TOC is enabled but its title is empty",
                "set a title such as `Contents` or disable the TOC",
                None,
                Some("toc.title".into()),
            );
        }
    }

    fn check_cover(&mut self) {
        if self.document.cover.enabled
            && self.document.cover.title.trim().is_empty()
            && self.document.title() == self.document.doc_id.as_str()
        {
            self.warning(
                "COVER_TITLE_EMPTY",
                "cover is enabled without a title and no h1 heading exists",
                "set cover.title or add an h1 heading",
                None,
                Some("cover.title".into()),
            );
        }
        if self.document.cover.enabled && self.document.cover.variant.trim().is_empty() {
            self.warning(
                "COVER_VARIANT_EMPTY",
                "cover variant is empty",
                "use classic, modern, minimal, split, custom, or a frontend-supported variant",
                None,
                Some("cover.variant".into()),
            );
        }
    }

    fn check_sources(&mut self) {
        for (source_id, source) in &self.document.sources {
            if source.title.trim().is_empty() {
                self.warning(
                    "SOURCE_TITLE_EMPTY",
                    format!("source `{source_id}` has an empty title"),
                    "set a title so inline citations can expand to a readable marker",
                    None,
                    Some(format!("sources.{source_id}.title")),
                );
            }
            if let Some(url) = source.url.as_deref()
                && !url.trim().is_empty()
                && crate::text::safe_href(url).is_none()
            {
                self.warning(
                    "SOURCE_URL_UNSAFE",
                    format!("source `{source_id}` URL is not a safe http(s)/relative URL"),
                    "use an http(s), mailto:, tel:, or relative URL",
                    None,
                    Some(format!("sources.{source_id}.url")),
                );
            }
        }
    }
}

fn has_manual_number_prefix(title: &str) -> bool {
    let lower = title.trim().to_ascii_lowercase();
    let mut chars = lower.chars().peekable();
    let Some(first) = chars.next() else {
        return false;
    };
    if matches!(first, '图' | '表') {
        let mut saw_digit = false;
        for ch in chars.by_ref() {
            if ch.is_ascii_digit() {
                saw_digit = true;
                break;
            }
            if ch == ' ' || ch == '\u{3000}' {
                continue;
            }
            break;
        }
        return saw_digit;
    }
    let rest = lower.as_str();
    for prefix in ["figure", "fig.", "table", "tab."] {
        if let Some(mut remainder) = rest.strip_prefix(prefix) {
            remainder = remainder.trim_start();
            if remainder
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_digit())
            {
                return true;
            }
        }
    }
    false
}

fn html_has_duplicate_title(html: &str, title: &str) -> bool {
    let mut rest = html.trim_start();
    while let Some(stripped) = rest.strip_prefix("<!--") {
        let Some(end) = stripped.find("-->") else {
            return false;
        };
        rest = stripped[end + 3..].trim_start();
    }
    let Some(rest) = rest.strip_prefix('<') else {
        return false;
    };
    let Some(tag_end) = rest.find('>') else {
        return false;
    };
    let tag_body = &rest[..tag_end];
    let tag_name = tag_body
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_ascii_lowercase();
    let is_heading = matches!(tag_name.as_str(), "h1" | "h2" | "h3" | "h4");
    let is_title_class = tag_body.to_ascii_lowercase().contains("class=")
        && tag_body.to_ascii_lowercase().contains("title");
    if !is_heading && !is_title_class {
        return false;
    }
    let after_open = &rest[tag_end + 1..];
    let closing = format!("</{tag_name}>");
    let inner = after_open
        .to_ascii_lowercase()
        .find(&closing)
        .map(|end| &after_open[..end])
        .unwrap_or(after_open);
    let inner = strip_tags(inner);
    let inner = normalize_title_text(&inner);
    let title = normalize_title_text(title);
    !inner.is_empty() && inner == title
}

fn strip_tags(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

fn normalize_title_text(input: &str) -> String {
    input
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
    }
}

fn block_ids(document: &ReportDocument) -> String {
    document
        .blocks
        .iter()
        .map(|block| block.id.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn source_ids(document: &ReportDocument) -> String {
    if document.sources.is_empty() {
        "(none registered; call upsert_sources first)".to_string()
    } else {
        document
            .sources
            .keys()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;
    use crate::{
        block::{Block, CellValue, ColumnNode, CustomHtmlBlock, EchartsBlock, TableBlock},
        model::{ReportDocument, SourceEntry},
    };
    use std::collections::BTreeMap;

    fn valid_doc() -> ReportDocument {
        let mut doc = ReportDocument::new(doc_id("report"));
        doc.toc.max_depth = 3;
        doc.sources
            .insert(source_id("sec10"), SourceEntry::new("Annual filing"));
        doc.blocks
            .push(Block::heading(block_id("intro"), 1, "Introduction"));
        doc.blocks
            .push(Block::paragraph(block_id("summary"), "Revenue [^sec10]."));
        doc.blocks.push(Block::table(
            block_id("table"),
            TableBlock {
                title: None,
                numbering: None,
                note: None,
                columns: vec![
                    ColumnNode::leaf("region", "Region"),
                    ColumnNode::leaf("value", "Value"),
                ],
                rows: vec![BTreeMap::from([
                    ("region".to_string(), CellValue::text("APAC")),
                    ("value".to_string(), CellValue::Integer(10)),
                ])],
                summary_rows: vec![],
            },
        ));
        doc
    }

    #[test]
    fn valid_document_has_no_errors() {
        let report = lint_document(&valid_doc());
        assert!(!report.has_errors(), "{:?}", report.diagnostics);
    }

    #[test]
    fn dangling_citation_hint_lists_valid_sources() {
        let mut doc = valid_doc();
        doc.blocks
            .push(Block::paragraph(block_id("bad"), "Broken [^missing]."));
        let report = lint_document(&doc);
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "DANGLING_SOURCE_ID")
            .unwrap();
        assert_eq!(diagnostic.severity, Severity::Error);
        assert!(diagnostic.message.contains("sec10"));
        assert!(diagnostic.hint.contains("sec10"));
        assert_eq!(diagnostic.block_id.as_deref(), Some("bad"));
    }

    #[test]
    fn unused_source_and_dangling_block_source_are_reported() {
        let mut doc = valid_doc();
        doc.sources
            .insert(source_id("unused"), SourceEntry::new("Unused"));
        doc.blocks.push(
            Block::paragraph(block_id("bad"), "No marker").with_sources([source_id("ghost")]),
        );
        let report = lint_document(&doc);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "UNUSED_SOURCE"
                    && diagnostic.message.contains("unused"))
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "DANGLING_SOURCE_ID"
                    && diagnostic.message.contains("ghost"))
        );
    }

    #[test]
    fn table_schema_unknown_keys_and_widths_are_actionable() {
        let mut doc = valid_doc();
        doc.blocks.push(Block::table(
            block_id("bad-table"),
            TableBlock {
                title: None,
                numbering: None,
                note: None,
                columns: vec![
                    ColumnNode::leaf("a", "A"),
                    ColumnNode::leaf("a", "Duplicate"),
                ],
                rows: vec![BTreeMap::from([(
                    "unknown".to_string(),
                    CellValue::text("x"),
                )])],
                summary_rows: vec![],
            },
        ));
        let report = lint_document(&doc);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "TABLE_SCHEMA_INVALID"
                    && diagnostic.message.contains("duplicate"))
        );

        let mut doc = valid_doc();
        doc.blocks.pop();
        doc.blocks.push(Block::table(
            block_id("table2"),
            TableBlock {
                title: None,
                numbering: None,
                note: None,
                columns: vec![ColumnNode::leaf("a", "A")],
                rows: vec![BTreeMap::from([("b".to_string(), CellValue::text("x"))])],
                summary_rows: vec![],
            },
        ));
        let report = lint_document(&doc);
        let warning = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "TABLE_UNKNOWN_ROW_KEY")
            .unwrap();
        assert!(warning.hint.contains("valid leaf keys: a"));
    }

    #[test]
    fn css_errors_include_line_and_column() {
        let mut doc = valid_doc();
        doc.theme.custom_css = ".a {\n color: red;\n".into();
        let report = lint_document(&doc);
        let diagnostic = report
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "THEME_CSS_SYNTAX")
            .unwrap();
        assert!(diagnostic.message.contains("line 1"));
        assert!(diagnostic.message.contains("column"));
    }

    #[test]
    fn custom_html_requires_fallback_and_valid_css() {
        let mut doc = valid_doc();
        doc.blocks.push(Block::new(
            block_id("html"),
            BlockKind::CustomHtml(CustomHtmlBlock {
                title: None,
                numbering: None,
                note: None,
                html: "<div>x</div>".into(),
                css: Some(".x { color: red;".into()),
                fallback_text: "".into(),
            }),
        ));
        let report = lint_document(&doc);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "CUSTOM_HTML_FALLBACK_EMPTY")
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "CUSTOM_HTML_CSS_SYNTAX")
        );
    }

    #[test]
    fn echarts_validation_is_actionable() {
        let mut doc = valid_doc();
        doc.blocks.push(Block::new(
            block_id("chart"),
            BlockKind::Echarts(EchartsBlock {
                title: None,
                height: 30,
                option: serde_json::Value::Null,
                option_patch: None,
                fallback_summary: "".into(),
                numbering: None,
                note: None,
            }),
        ));
        let report = lint_document(&doc);
        for code in [
            "ECHARTS_FALLBACK_EMPTY",
            "ECHARTS_HEIGHT_RANGE",
            "ECHARTS_OPTION_MISSING",
        ] {
            assert!(
                report
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.code == code),
                "missing {code}"
            );
        }
    }

    fn chart_block(option: serde_json::Value) -> Block {
        Block::new(
            block_id("chart"),
            BlockKind::Echarts(EchartsBlock {
                title: Some("Revenue".into()),
                height: 300,
                option,
                option_patch: None,
                fallback_summary: "Revenue rose.".into(),
                numbering: None,
                note: None,
            }),
        )
    }

    fn chart_diagnostics(option: serde_json::Value) -> LintReport {
        let mut doc = valid_doc();
        doc.blocks.push(chart_block(option));
        lint_document(&doc)
    }

    fn has_code(report: &LintReport, code: &str) -> bool {
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == code)
    }

    #[test]
    fn echarts_6_valid_option_is_lint_clean() {
        let report = chart_diagnostics(serde_json::json!({
            "matrix": {"x": {"data": ["A", "B"]}, "y": {"data": ["C", "D"]}},
            "xAxis": [{}, {}],
            "yAxis": [{}],
            "grid": [{}],
            "polar": [{}],
            "radar": [{"indicator": [{"name": "Revenue"}, {"name": "Margin"}]}],
            "dataset": {"source": [["Quarter", "Value"], ["Q1", 10]]},
            "series": [
                {"type": "bar", "xAxisIndex": 1, "yAxisIndex": 0, "gridIndex": 0, "data": [10, 20]},
                {"type": "radar", "data": [{"name": "Plan", "value": [10, 20]}]},
                {"type": "sankey", "links": [{"source": "A", "target": "B", "value": 5}]},
                {"type": "chord", "data": [{"name": "A"}], "links": [{"source": "A", "target": "B", "value": 5}]},
                {"type": "beeswarm", "data": [1, 2, 3]},
                {"type": "custom", "coordinateSystem": "matrix", "data": [1, 2]}
            ]
        }));
        assert_eq!(report.summary.errors, 0, "{:?}", report.diagnostics);
    }

    #[test]
    fn echarts_rejects_js_function_strings() {
        let report = chart_diagnostics(serde_json::json!({
            "series": [{
                "type": "pie",
                "data": [{"name": "A", "value": 10}],
                "label": {"formatter": "function(params) { return params.name; }"}
            }]
        }));
        assert!(has_code(&report, "ECHARTS_JS_FUNCTION_STRING"));

        let report = chart_diagnostics(serde_json::json!({
            "tooltip": {"formatter": "(p) => p.name"},
            "series": [{"type": "bar", "data": [10]}]
        }));
        assert!(has_code(&report, "ECHARTS_JS_FUNCTION_STRING"));

        let report = chart_diagnostics(serde_json::json!({
            "tooltip": {"formatter": "function(p) { return p.name; }"},
            "series": [{"type": "bar", "data": [10]}]
        }));
        assert!(has_code(&report, "ECHARTS_JS_FUNCTION_STRING"));

        let report = chart_diagnostics(serde_json::json!({
            "tooltip": {"formatter": "value => value"},
            "series": [{"type": "bar", "data": [10]}]
        }));
        assert!(has_code(&report, "ECHARTS_JS_FUNCTION_STRING"));
    }

    #[test]
    fn echarts_ignores_js_like_business_text_in_data_fields() {
        let report = chart_diagnostics(serde_json::json!({
            "title": {"text": "A => B"},
            "dataset": {"source": [
                ["Path", "Value"],
                ["Step A => Step B", 1]
            ]},
            "xAxis": {"data": ["Step A => Step B"]},
            "series": [
                {
                    "type": "bar",
                    "name": "A => B",
                    "data": [{"name": "A => B", "value": 10}]
                },
                {
                    "type": "graph",
                    "name": "Step A => Step B",
                    "data": [{"name": "A => B"}],
                    "links": [
                        {"source": "A => B", "target": "B => C", "value": 1}
                    ]
                }
            ]
        }));
        assert!(
            !has_code(&report, "ECHARTS_JS_FUNCTION_STRING"),
            "{:?}",
            report.diagnostics
        );
        assert!(!crate::echarts::looks_like_js_function("A => B"));
        assert!(!crate::echarts::looks_like_js_function("Step A => Step B"));
        assert!(crate::echarts::looks_like_js_function("value => value"));
        assert!(crate::echarts::looks_like_js_function("x => x"));
        assert!(crate::echarts::looks_like_js_function("(p) => p.name"));
        assert!(crate::echarts::looks_like_js_function(
            "function(p) { return p.name; }"
        ));
    }

    #[test]
    fn echarts_requires_radar_and_polar_components() {
        let report = chart_diagnostics(serde_json::json!({
            "series": [
                {"type": "radar", "data": [{"name": "Plan", "value": [1, 2]}]},
                {"type": "line", "radarIndex": 0, "data": [1]},
                {"type": "line", "polarIndex": 0, "data": [1]}
            ]
        }));
        assert!(has_code(&report, "ECHARTS_COORDINATE_MISSING"));
        assert!(
            report
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.code == "ECHARTS_COORDINATE_MISSING")
                .count()
                >= 3
        );

        let report = chart_diagnostics(serde_json::json!({
            "radar": [{"indicator": [{"name": "Revenue"}]}],
            "series": [{
                "type": "radar",
                "radarIndex": 2,
                "data": [{"name": "Plan", "value": [1]}]
            }]
        }));
        assert!(has_code(&report, "ECHARTS_INDEX_OUT_OF_RANGE"));
    }

    #[test]
    fn echarts_validates_series_types_matrix_and_indices() {
        let report = chart_diagnostics(serde_json::json!({
            "xAxis": [{}],
            "yAxis": [{}],
            "radar": [{"indicator": [{"name": "A"}]}],
            "series": [
                {"type": "notARealType", "data": [1]},
                {"type": "custom", "coordinateSystem": "matrix", "data": [1]},
                {"type": "bar", "xAxisIndex": 2, "yAxisIndex": 0, "radarIndex": 5, "data": [1]}
            ]
        }));
        assert!(has_code(&report, "ECHARTS_SERIES_TYPE_INVALID"));
        assert!(has_code(&report, "ECHARTS_MATRIX_MISSING"));
        assert!(has_code(&report, "ECHARTS_INDEX_OUT_OF_RANGE"));

        let empty = serde_json::json!({});
        assert_eq!(axis_count(&empty, "xAxis"), 1);
        assert_eq!(axis_count(&empty, "yAxis"), 1);
        assert_eq!(axis_count(&empty, "grid"), 1);
        assert_eq!(axis_count(&empty, "radar"), 0);
        assert_eq!(axis_count(&empty, "polar"), 0);
    }

    #[test]
    fn echarts_requires_a_real_data_source() {
        let report = chart_diagnostics(serde_json::json!({
            "series": [{"type": "bar", "data": []}]
        }));
        assert!(has_code(&report, "ECHARTS_DATA_MISSING"));
    }

    #[test]
    fn echarts_native_option_is_lint_clean_and_lintable_in_isolation() {
        let block: Block = serde_json::from_value(serde_json::json!({
            "id": "chart",
            "type": "echarts",
            "title": "Revenue",
            "option": {"series": [{"type": "bar", "data": [1, 2]}]},
            "fallback_summary": "Revenue rose."
        }))
        .unwrap();
        let mut doc = valid_doc();
        doc.blocks.push(block);
        let report = lint_document(&doc);
        assert!(!has_code(&report, "ECHARTS_OPTION_MISSING"));
        assert_eq!(report.summary.errors, 0, "{:?}", report.diagnostics);

        let BlockKind::Echarts(chart) = &doc.blocks.last().unwrap().body else {
            panic!("chart");
        };
        let isolated = lint_echarts_block(block_id("preview-chart"), chart);
        assert_eq!(isolated.summary.errors, 0, "{:?}", isolated.diagnostics);
    }

    #[test]
    fn echarts_duplicate_title_uses_merged_option() {
        let mut doc = valid_doc();
        doc.blocks.push(Block::new(
            block_id("chart"),
            BlockKind::Echarts(EchartsBlock {
                title: Some("Revenue".into()),
                height: 300,
                option: serde_json::json!({
                    "series": [{"type": "bar", "data": [1]}]
                }),
                option_patch: Some(serde_json::json!({
                    "title": {"text": "Revenue", "top": "top"}
                })),
                fallback_summary: "Revenue rose.".into(),
                numbering: None,
                note: None,
            }),
        ));
        let report = lint_document(&doc);
        assert!(has_code(&report, "DUPLICATE_INNER_TITLE"));
    }

    #[test]
    fn echarts_allows_multi_panel_subtitle_arrays() {
        let report = chart_diagnostics(serde_json::json!({
            "grid": [{}, {}],
            "xAxis": [
                {"gridIndex": 0, "type": "category", "data": ["Q1", "Q2"]},
                {"gridIndex": 1, "type": "category", "data": ["East", "West"]}
            ],
            "yAxis": [{"gridIndex": 0}, {"gridIndex": 1}],
            "title": [
                {"text": "华东区", "left": "4%", "top": 4, "textStyle": {"fontSize": 12}},
                {"text": "华南区", "left": "54%", "top": 4, "textStyle": {"fontSize": 12}}
            ],
            "series": [
                {"type": "bar", "xAxisIndex": 0, "yAxisIndex": 0, "data": [10, 20]},
                {"type": "line", "xAxisIndex": 1, "yAxisIndex": 1, "data": [30, 40]}
            ]
        }));
        assert!(!has_code(&report, "DUPLICATE_INNER_TITLE"));
        assert_eq!(report.summary.errors, 0, "{:?}", report.diagnostics);
    }

    #[test]
    fn echarts_allows_a_single_positioned_light_subtitle() {
        let report = chart_diagnostics(serde_json::json!({
            "title": {
                "text": "华东区",
                "left": "24%",
                "top": 4,
                "textStyle": {"fontSize": 12}
            },
            "series": [{"type": "bar", "data": [10, 20]}]
        }));
        assert!(!has_code(&report, "DUPLICATE_INNER_TITLE"));
    }

    #[test]
    fn echarts_rejects_outer_title_repeated_in_subtitle_array() {
        let report = chart_diagnostics(serde_json::json!({
            "title": [
                {"text": "Revenue", "left": "4%", "top": 4},
                {"text": "华东区", "left": "54%", "top": 4}
            ],
            "series": [{"type": "bar", "data": [10, 20]}]
        }));
        assert!(has_code(&report, "DUPLICATE_INNER_TITLE"));
    }

    #[test]
    fn echarts_validates_axis_grid_and_dataset_bindings() {
        let report = chart_diagnostics(serde_json::json!({
            "grid": [{}],
            "xAxis": [{"gridIndex": 1}],
            "yAxis": [{"gridIndex": 0}],
            "dataset": [{"source": [["Quarter", "Value"], ["Q1", 10]]}],
            "series": [{"type": "bar", "xAxisIndex": 0, "yAxisIndex": 0, "datasetIndex": 1}]
        }));
        assert!(has_code(&report, "ECHARTS_INDEX_OUT_OF_RANGE"));
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "ECHARTS_INDEX_OUT_OF_RANGE"
                    && diagnostic
                        .path
                        .as_deref()
                        .is_some_and(|path| path.contains("xAxis[0].gridIndex"))
            }),
            "{:?}",
            report.diagnostics
        );
        assert!(
            report.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "ECHARTS_INDEX_OUT_OF_RANGE"
                    && diagnostic
                        .path
                        .as_deref()
                        .is_some_and(|path| path.contains("datasetIndex"))
            }),
            "{:?}",
            report.diagnostics
        );
    }

    #[test]
    fn toc_depth_and_uuid_ids_are_errors() {
        let mut doc = valid_doc();
        doc.toc.max_depth = 5;
        doc.blocks.push(Block::heading(
            block_id("123e4567-e89b-42d3-a456-426614174000"),
            2,
            "UUID heading",
        ));
        let report = lint_document(&doc);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "TOC_DEPTH_RANGE")
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "BLOCK_ID_UUID_V4")
        );
    }

    #[test]
    fn warns_on_manual_numbering_and_duplicate_custom_html_title() {
        let mut doc = valid_doc();
        doc.blocks.push(Block::table(
            block_id("manual-table"),
            TableBlock {
                title: Some("表 1: Revenue".into()),
                numbering: None,
                note: None,
                columns: vec![ColumnNode::leaf("a", "A")],
                rows: vec![],
                summary_rows: vec![],
            },
        ));
        doc.blocks.push(Block::new(
            block_id("html"),
            BlockKind::CustomHtml(CustomHtmlBlock {
                title: Some("KPI Grid".into()),
                numbering: None,
                note: None,
                html: "<h2>KPI Grid</h2><div>18%</div>".into(),
                css: None,
                fallback_text: "18%".into(),
            }),
        ));
        let report = lint_document(&doc);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "MANUAL_PREFIX_IN_TITLE")
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "DUPLICATE_INNER_TITLE")
        );
    }
}
