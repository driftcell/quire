//! Block AST definitions and shared block metadata.

use std::collections::{BTreeMap, BTreeSet};

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    ids::{BlockId, SourceId},
    text::slugify,
};

/// Common style controls available on every block.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BlockStyle {
    /// Optional renderer hint, for example `callout` or `numbered`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    /// Prevent this block from being split across pages where the renderer supports it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub keep_together: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub page_break_before: bool,
    /// Per-leaf-column CSS widths, keyed by leaf column key. Accepts `%`, `px`, `rem` or numbers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_widths: Option<BTreeMap<String, String>>,
}

const fn is_false(value: &bool) -> bool {
    !*value
}

/// Inferred or explicit data type of a table column.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ColumnDataType {
    /// Free-form text, left aligned.
    #[default]
    Text,
    /// Numeric value, right aligned.
    Number,
    /// Currency amount, right aligned.
    Currency,
    /// Percentage, right aligned.
    Percent,
    /// Date string, center aligned.
    Date,
    /// Short status token rendered as a badge, center aligned.
    Badge,
}

/// Alias retained for concise internal references.
pub type DataType = ColumnDataType;

impl ColumnDataType {
    /// Default body alignment for this data type.
    pub const fn default_align(self) -> Align {
        match self {
            Self::Number | Self::Currency | Self::Percent => Align::Right,
            Self::Date | Self::Badge => Align::Center,
            Self::Text => Align::Left,
        }
    }

    pub const fn is_numeric(self) -> bool {
        matches!(self, Self::Number | Self::Currency | Self::Percent)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    Left,
    Center,
    Right,
}

impl Align {
    pub const fn as_css(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }

    pub const fn as_str(self) -> &'static str {
        self.as_css()
    }
}

/// How a leaf column participates in width distribution.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ColumnSizing {
    /// Size close to its P90 content width.
    #[default]
    Fit,
    /// Take a weighted share of available table width.
    Grow,
}

/// A column node in a possibly multi-level table header.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ColumnNode {
    /// Stable leaf key. Must be unique among all leaf columns. Groups may omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Human-readable header label.
    pub label: String,
    /// Child columns for a grouped/multi-level header. Empty means this node is a leaf.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<ColumnNode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_type: Option<ColumnDataType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sizing: Option<ColumnSizing>,
    /// Relative width weight for `grow` columns (and a multiplier for `fit` columns).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f32>,
    /// Body cell alignment override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    /// Header cell alignment override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header_align: Option<Align>,
}

impl ColumnNode {
    pub fn leaf(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: Some(key.into()),
            label: label.into(),
            children: Vec::new(),
            data_type: None,
            sizing: None,
            weight: None,
            align: None,
            header_align: None,
        }
    }

    pub fn group(
        key: impl Into<String>,
        label: impl Into<String>,
        children: Vec<ColumnNode>,
    ) -> Self {
        Self {
            key: Some(key.into()),
            label: label.into(),
            children,
            data_type: None,
            sizing: None,
            weight: None,
            align: None,
            header_align: None,
        }
    }

    pub fn with_type(mut self, data_type: ColumnDataType) -> Self {
        self.data_type = Some(data_type);
        self
    }

    pub fn with_sizing(mut self, sizing: ColumnSizing) -> Self {
        self.sizing = Some(sizing);
        self
    }

    pub fn with_weight(mut self, weight: f32) -> Self {
        self.weight = Some(weight);
        self
    }

    pub const fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }

    pub fn key_ref(&self) -> Option<&str> {
        self.key.as_deref()
    }

    pub fn effective_data_type(&self) -> ColumnDataType {
        self.data_type.unwrap_or_default()
    }

    pub fn effective_sizing(&self) -> ColumnSizing {
        self.sizing.unwrap_or_default()
    }

    pub fn effective_weight(&self) -> f32 {
        self.weight.unwrap_or(1.0)
    }

    pub fn effective_align(&self) -> Align {
        self.align
            .unwrap_or_else(|| self.effective_data_type().default_align())
    }

    pub fn effective_header_align(&self) -> Align {
        self.header_align
            .or(self.align)
            .unwrap_or_else(|| self.effective_data_type().default_align())
    }

    pub fn leaf_count(&self) -> usize {
        if self.is_leaf() {
            1
        } else {
            self.children.iter().map(ColumnNode::leaf_count).sum()
        }
    }

    pub fn max_depth(&self) -> usize {
        1 + self
            .children
            .iter()
            .map(ColumnNode::max_depth)
            .max()
            .unwrap_or(0)
    }

    /// Visit all leaf nodes in display order.
    pub fn visit_leaves<'a>(&'a self, visitor: &mut impl FnMut(&'a ColumnNode)) {
        if self.is_leaf() {
            visitor(self);
        } else {
            for child in &self.children {
                child.visit_leaves(visitor);
            }
        }
    }
}

/// A table cell value. Untagged so agents can submit plain strings/numbers or rich cells.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum CellValue {
    Text(String),
    Integer(i64),
    Float(f64),
    Bool(bool),
    #[default]
    Null,
    Rich(RichCell),
}

impl CellValue {
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    pub fn display_text(&self) -> String {
        match self {
            Self::Text(value) => value.clone(),
            Self::Integer(value) => value.to_string(),
            Self::Float(value) => format_number(*value),
            Self::Bool(value) => value.to_string(),
            Self::Null => String::new(),
            Self::Rich(cell) => cell.display_text(),
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Integer(value) => Some(*value as f64),
            Self::Float(value) => Some(*value),
            Self::Rich(cell) => cell.number,
            _ => None,
        }
    }

    pub fn rich_class(&self) -> Option<&str> {
        match self {
            Self::Rich(cell) => cell.class_name.as_deref(),
            _ => None,
        }
    }

    pub fn rich_align(&self) -> Option<Align> {
        match self {
            Self::Rich(cell) => cell.align,
            _ => None,
        }
    }

    pub fn rich_tooltip(&self) -> Option<&str> {
        match self {
            Self::Rich(cell) => cell.tooltip.as_deref(),
            _ => None,
        }
    }
}

/// A rich table cell with presentation hints. At least one of `text` or `number` is expected.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RichCell {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
}

impl RichCell {
    pub fn display_text(&self) -> String {
        if let Some(text) = &self.text {
            return text.clone();
        }
        self.number.map(format_number).unwrap_or_default()
    }
}

/// Format an `f64` without a trailing `.0` and without scientific notation for common values.
pub fn format_number(value: f64) -> String {
    if !value.is_finite() {
        return String::new();
    }
    if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
        return format!("{value:.0}");
    }
    let mut rendered = format!("{value:.6}");
    while rendered.ends_with('0') {
        rendered.pop();
    }
    if rendered.ends_with('.') {
        rendered.pop();
    }
    rendered
}

/// Automatic figure/table numbering kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NumberingKind {
    /// Number as a figure (`图 1`).
    Figure,
    /// Number as a table (`表 1`).
    Table,
    /// Do not number.
    #[default]
    None,
}

/// Table block payload. Header layout is computed by the table compiler, not by the agent.
///
/// Accepts the full canonical schema (`columns` + row objects) or a compact shorthand
/// (`headers` + row arrays). Shorthand column keys/types are inferred from cell values.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TableBlock {
    /// Outer table title. Put titles here only, never inside custom HTML.
    #[serde(default, alias = "caption", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// `table` by default; use `none` to suppress the `表 N` label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numbering: Option<NumberingKind>,
    /// Explanatory footnote rendered below the table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Recursive header tree. Agents declare groups/leaves; spans are pre-computed.
    pub columns: Vec<ColumnNode>,
    /// Body rows keyed by leaf column key. Example: `[{"region":"EMEA","q1":800000}]`.
    #[serde(default)]
    pub rows: Vec<BTreeMap<String, CellValue>>,
    /// Footer rows (totals, averages) with the same key shape as `rows`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary_rows: Vec<BTreeMap<String, CellValue>>,
}

/// Schema-only canonical table shape.
#[expect(dead_code, reason = "Schema-only table wire shape")]
#[derive(JsonSchema)]
#[schemars(rename = "TableBlockColumns")]
struct TableBlockColumnsSchema {
    /// Outer table title (legacy alias `caption` is also accepted on input).
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    numbering: Option<NumberingKind>,
    #[serde(default)]
    note: Option<String>,
    /// Recursive column tree. Use this shape OR `headers` shorthand, not both.
    columns: Vec<ColumnNode>,
    /// Body rows keyed by leaf column key.
    #[serde(default)]
    rows: Vec<BTreeMap<String, CellValue>>,
    /// Footer rows with the same key shape as `rows`.
    #[serde(default)]
    summary_rows: Vec<BTreeMap<String, CellValue>>,
}

/// Schema-only compact table shape.
#[expect(dead_code, reason = "Schema-only table wire shape")]
#[derive(JsonSchema)]
#[schemars(rename = "TableBlockHeaders")]
struct TableBlockHeadersSchema {
    /// Outer table title (legacy alias `caption` is also accepted on input).
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    numbering: Option<NumberingKind>,
    #[serde(default)]
    note: Option<String>,
    /// Header labels; column keys and data types are inferred.
    headers: Vec<String>,
    /// Two-dimensional body rows aligned with `headers`.
    #[serde(default)]
    rows: Vec<Vec<CellValue>>,
    /// Two-dimensional footer rows aligned with `headers`.
    #[serde(default)]
    summary_rows: Vec<Vec<CellValue>>,
}

impl JsonSchema for TableBlock {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "TableBlock".into()
    }

    fn schema_id() -> std::borrow::Cow<'static, str> {
        concat!(module_path!(), "::TableBlock").into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let columns = generator.subschema_for::<TableBlockColumnsSchema>();
        let headers = generator.subschema_for::<TableBlockHeadersSchema>();
        let mut schema = schemars::json_schema!({
            "title": "TableBlock",
            "description": "Canonical table with a recursive `columns` tree and row objects, OR compact shorthand with `headers` and two-dimensional `rows`/`summary_rows`. The two shapes are mutually exclusive.",
            "oneOf": []
        });
        if let Some(object) = schema.as_object_mut() {
            object.insert(
                "oneOf".to_string(),
                serde_json::Value::Array(vec![columns.into(), headers.into()]),
            );
        }
        schema
    }
}

impl TableBlock {
    pub const fn numbering_kind(&self) -> NumberingKind {
        match self.numbering {
            Some(kind) => kind,
            None => NumberingKind::Table,
        }
    }
}

#[derive(Deserialize)]
struct TableRepr {
    #[serde(default, alias = "caption")]
    title: Option<String>,
    #[serde(default)]
    numbering: Option<NumberingKind>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    columns: Option<Vec<ColumnNode>>,
    #[serde(default)]
    headers: Option<Vec<String>>,
    #[serde(default)]
    rows: Vec<RowInput>,
    #[serde(default)]
    summary_rows: Vec<RowInput>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RowInput {
    Map(BTreeMap<String, CellValue>),
    Array(Vec<CellValue>),
}

impl<'de> Deserialize<'de> for TableBlock {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;
        let repr = TableRepr::deserialize(deserializer)?;
        let (columns, rows, summary_rows) = if let Some(columns) = repr.columns {
            if repr.headers.is_some() {
                return Err(D::Error::custom(
                    "table accepts either `columns` or `headers`, not both",
                ));
            }
            let leaf_keys: Vec<String> = {
                let mut keys = Vec::new();
                for column in &columns {
                    column.visit_leaves(&mut |leaf| {
                        if let Some(key) = &leaf.key {
                            keys.push(key.clone());
                        }
                    });
                }
                keys
            };
            let rows = normalize_rows(repr.rows, Some(&leaf_keys)).map_err(D::Error::custom)?;
            let summary_rows =
                normalize_rows(repr.summary_rows, Some(&leaf_keys)).map_err(D::Error::custom)?;
            (columns, rows, summary_rows)
        } else if let Some(headers) = repr.headers {
            let (columns, keys) = shorthand_columns(&headers, &repr.rows, &repr.summary_rows);
            let rows = normalize_rows(repr.rows, Some(&keys)).map_err(D::Error::custom)?;
            let summary_rows =
                normalize_rows(repr.summary_rows, Some(&keys)).map_err(D::Error::custom)?;
            (columns, rows, summary_rows)
        } else {
            return Err(D::Error::custom(
                "table requires `columns` (full schema) or `headers` (compact shorthand)",
            ));
        };
        Ok(Self {
            title: repr.title,
            numbering: repr.numbering,
            note: repr.note,
            columns,
            rows,
            summary_rows,
        })
    }
}

fn normalize_rows(
    rows: Vec<RowInput>,
    leaf_keys: Option<&[String]>,
) -> Result<Vec<BTreeMap<String, CellValue>>, String> {
    let mut output = Vec::with_capacity(rows.len());
    for (row_index, row) in rows.into_iter().enumerate() {
        match row {
            RowInput::Map(map) => output.push(map),
            RowInput::Array(values) => {
                let keys = leaf_keys.ok_or_else(|| {
                    format!("row {row_index} is an array but no leaf column keys are available")
                })?;
                if values.len() > keys.len() {
                    return Err(format!(
                        "row {row_index} has {} cells but only {} columns",
                        values.len(),
                        keys.len()
                    ));
                }
                let mut map = BTreeMap::new();
                for (value, key) in values.into_iter().zip(keys.iter()) {
                    map.insert(key.clone(), value);
                }
                output.push(map);
            }
        }
    }
    Ok(output)
}

fn shorthand_columns(
    headers: &[String],
    rows: &[RowInput],
    summary_rows: &[RowInput],
) -> (Vec<ColumnNode>, Vec<String>) {
    let mut keys = Vec::new();
    let mut used = BTreeSet::new();
    for (index, header) in headers.iter().enumerate() {
        let base = if header.trim().is_empty() {
            format!("col_{}", index + 1)
        } else {
            slugify(header)
        };
        let mut candidate = if base.is_empty() {
            format!("col_{}", index + 1)
        } else {
            base
        };
        let mut suffix = 2;
        while !used.insert(candidate.clone()) {
            candidate = format!("{candidate}_{suffix}");
            suffix += 1;
        }
        keys.push(candidate);
    }
    let mut columns = Vec::with_capacity(headers.len());
    for (index, header) in headers.iter().enumerate() {
        let mut values = Vec::new();
        for row in rows.iter().chain(summary_rows.iter()) {
            if let RowInput::Array(cells) = row
                && let Some(cell) = cells.get(index)
            {
                values.push(cell);
            }
        }
        let data_type = infer_data_type(&values);
        let key = keys
            .get(index)
            .cloned()
            .unwrap_or_else(|| format!("col_{}", index + 1));
        columns.push(ColumnNode::leaf(key, header).with_type(data_type));
    }
    (columns, keys)
}

fn infer_data_type(values: &[&CellValue]) -> ColumnDataType {
    let mut any_percent = false;
    let mut any_currency = false;
    let mut any_date = false;
    let mut any_number = false;
    for value in values {
        if value.as_number().is_some() {
            any_number = true;
            continue;
        }
        let text = value.display_text();
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        if text.ends_with('%') {
            any_percent = true;
        } else if text
            .chars()
            .any(|ch| matches!(ch, '$' | '¥' | '€' | '£' | '₹' | '₩'))
        {
            any_currency = true;
        } else if looks_like_date(text) {
            any_date = true;
        } else if text
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, ',' | '.' | '-' | '+'))
            && text.chars().any(|ch| ch.is_ascii_digit())
        {
            any_number = true;
        }
    }
    if any_percent {
        ColumnDataType::Percent
    } else if any_currency {
        ColumnDataType::Currency
    } else if any_date {
        ColumnDataType::Date
    } else if any_number {
        ColumnDataType::Number
    } else {
        ColumnDataType::Text
    }
}

fn looks_like_date(text: &str) -> bool {
    if text.len() < 8 || text.len() > 32 {
        return false;
    }
    let has_digit = text.chars().any(|ch| ch.is_ascii_digit());
    let has_separator =
        text.contains('-') || text.contains('/') || text.chars().any(char::is_whitespace);
    has_digit
        && has_separator
        && text.chars().all(|ch| {
            ch.is_ascii_alphanumeric()
                || matches!(ch, '-' | '/' | ':' | 'T' | ' ' | ',' | '.' | '+' | 'Z')
        })
}

/// An ECharts 6.0 chart block backed by one native `option` object.
/// `fallback_summary` is mandatory so non-JS renderers stay useful.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EchartsBlock {
    /// Optional chart title. `caption` is accepted as a legacy alias on input.
    #[serde(default, alias = "caption", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Canvas height in pixels, recommended 120..=2000.
    #[serde(default = "default_chart_height")]
    pub height: u32,
    /// Complete native ECharts 6.0 option object. Required: no presets or mode switches.
    pub option: serde_json::Value,
    /// Deep-merged onto the base option. Arrays replace arrays, objects merge recursively.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option_patch: Option<serde_json::Value>,
    /// Mandatory one-sentence numeric summary for reviewers, Markdown projection and non-JS use.
    #[serde(default)]
    pub fallback_summary: String,
    /// `figure` by default; use `none` to suppress the `图 N` label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numbering: Option<NumberingKind>,
    /// Explanatory footnote rendered below the chart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl EchartsBlock {
    pub const fn numbering_kind(&self) -> NumberingKind {
        match self.numbering {
            Some(kind) => kind,
            None => NumberingKind::Figure,
        }
    }
}

const fn default_chart_height() -> u32 {
    360
}

/// Raw HTML block. Custom HTML is trusted at render time, but must supply a fallback summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CustomHtmlBlock {
    /// Outer title. Put titles here only; never duplicate them inside the HTML. `caption` is
    /// accepted as a legacy alias for consistency with tables and charts.
    #[serde(default, alias = "caption", skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// `none` by default for freeform HTML; `figure` opts into `图 N` numbering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub numbering: Option<NumberingKind>,
    /// Explanatory footnote rendered below the block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Raw HTML trusted by the external renderer.
    pub html: String,
    /// Optional CSS scoped under `.report-root` by the frontend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub css: Option<String>,
    /// Non-empty plain-text/Markdown fallback. Required for lint cleanliness and agent review.
    #[serde(default)]
    pub fallback_text: String,
}

impl CustomHtmlBlock {
    pub const fn numbering_kind(&self) -> NumberingKind {
        match self.numbering {
            Some(kind) => kind,
            None => NumberingKind::None,
        }
    }
}

/// The type-specific body of a block. Serialized with a `type` discriminator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockKind {
    /// Section heading. Level 1..=6; headings auto-generate the TOC.
    Heading {
        /// Heading depth 1..=6.
        #[serde(default = "default_heading_level")]
        level: u8,
        /// Heading text. Inline Markdown is supported.
        text: String,
    },
    /// Prose paragraph with inline Markdown and `[^source_id]` citations.
    Paragraph {
        /// Paragraph text. Supports **bold**, *italic*, `code`, links and citations.
        text: String,
    },
    /// Pull quote with the same inline Markdown/citation support as paragraphs.
    Quote {
        /// Quote text.
        text: String,
    },
    /// Recursive multi-level table. Never compute colspan/rowspan manually.
    Table(TableBlock),
    /// ECharts chart with mandatory fallback summary.
    Echarts(EchartsBlock),
    /// Trusted raw HTML with mandatory fallback text.
    CustomHtml(CustomHtmlBlock),
}

const fn default_heading_level() -> u8 {
    2
}

impl BlockKind {
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Heading { .. } => "heading",
            Self::Paragraph { .. } => "paragraph",
            Self::Quote { .. } => "quote",
            Self::Table(_) => "table",
            Self::Echarts(_) => "echarts",
            Self::CustomHtml(_) => "custom_html",
        }
    }

    pub fn visible_text(&self) -> String {
        match self {
            Self::Heading { text, .. } | Self::Paragraph { text } | Self::Quote { text } => {
                text.clone()
            }
            Self::Table(table) => {
                let mut parts = Vec::new();
                if let Some(title) = &table.title {
                    parts.push(title.clone());
                }
                for row in table.rows.iter().chain(table.summary_rows.iter()) {
                    for value in row.values() {
                        let text = value.display_text();
                        if !text.is_empty() {
                            parts.push(text);
                        }
                    }
                }
                parts.join(" ")
            }
            Self::Echarts(chart) => {
                let mut text = chart.title.clone().unwrap_or_default();
                if !text.is_empty() && !chart.fallback_summary.is_empty() {
                    text.push(' ');
                }
                text.push_str(&chart.fallback_summary);
                text
            }
            Self::CustomHtml(html) => {
                if html.fallback_text.is_empty() {
                    html.html.clone()
                } else {
                    html.fallback_text.clone()
                }
            }
        }
    }
}

/// A report block with common metadata shared by all variants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Block {
    /// Short semantic ID, for example `q3-revenue`. Empty/missing IDs are auto-assigned on insert.
    #[serde(
        default = "BlockId::pending",
        deserialize_with = "deserialize_block_id_forgiving"
    )]
    pub id: BlockId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    #[serde(default)]
    pub style: BlockStyle,
    /// Citation source IDs attached to the whole block.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_ids: Vec<SourceId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_note: Option<String>,
    /// Suppressible lint rules ignored for this block only. Core contract and safety rules
    /// remain active regardless of this list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lint_ignore: Vec<String>,
    #[serde(flatten)]
    pub body: BlockKind,
}

fn deserialize_block_id_forgiving<'de, D>(deserializer: D) -> Result<BlockId, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error as _;
    let raw = String::deserialize(deserializer)?;
    if raw.trim().is_empty() {
        Ok(BlockId::pending())
    } else {
        BlockId::parse(raw).map_err(D::Error::custom)
    }
}

impl Block {
    pub fn new(id: BlockId, body: BlockKind) -> Self {
        Self {
            id,
            class_name: None,
            style: BlockStyle::default(),
            source_ids: Vec::new(),
            source_note: None,
            lint_ignore: Vec::new(),
            body,
        }
    }

    pub fn heading(id: BlockId, level: u8, text: impl Into<String>) -> Self {
        Self::new(
            id,
            BlockKind::Heading {
                level,
                text: text.into(),
            },
        )
    }

    pub fn paragraph(id: BlockId, text: impl Into<String>) -> Self {
        Self::new(id, BlockKind::Paragraph { text: text.into() })
    }

    pub fn quote(id: BlockId, text: impl Into<String>) -> Self {
        Self::new(id, BlockKind::Quote { text: text.into() })
    }

    pub fn table(id: BlockId, table: TableBlock) -> Self {
        Self::new(id, BlockKind::Table(table))
    }

    pub fn with_class(mut self, class_name: impl Into<String>) -> Self {
        self.class_name = Some(class_name.into());
        self
    }

    pub fn with_style(mut self, style: BlockStyle) -> Self {
        self.style = style;
        self
    }

    pub fn with_sources(mut self, source_ids: impl IntoIterator<Item = SourceId>) -> Self {
        self.source_ids = source_ids.into_iter().collect();
        self
    }

    pub fn with_source_note(mut self, note: impl Into<String>) -> Self {
        self.source_note = Some(note.into());
        self
    }

    pub const fn block_type(&self) -> &'static str {
        self.body.type_name()
    }

    pub fn visible_text(&self) -> String {
        self.body.visible_text()
    }

    /// First 30 characters of visible text, used by the low-token MCP digest.
    pub fn preview_30(&self) -> String {
        crate::text::truncate_chars(self.visible_text().trim(), 30)
    }

    /// Number of Unicode scalar values in visible text.
    pub fn char_count(&self) -> usize {
        match &self.body {
            BlockKind::Paragraph { text }
            | BlockKind::Heading { text, .. }
            | BlockKind::Quote { text } => crate::inline::readable_char_count(text),
            _ => self.visible_text().chars().count(),
        }
    }
}

/// Validate that a block ID is short and semantic.
pub fn is_semantic_block_id(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphanumeric() {
        return false;
    }
    id.len() <= 64 && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Detect canonical UUIDv4 strings, which the protocol explicitly avoids as block IDs.
pub fn looks_like_uuid_v4(id: &str) -> bool {
    let bytes = id.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        match index {
            8 | 13 | 18 | 23 => {
                if *byte != b'-' {
                    return false;
                }
            }
            14 => {
                if *byte != b'4' {
                    return false;
                }
            }
            19 => {
                if !matches!(*byte, b'8' | b'9' | b'a' | b'b' | b'A' | b'B') {
                    return false;
                }
            }
            _ => {
                if !byte.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    #[test]
    fn block_serializes_with_discriminator_and_metadata() {
        let block = Block::paragraph(block_id("exec-summary"), "Revenue grew **18%** [^sec10]")
            .with_sources([source_id("sec10")]);
        let value = serde_json::to_value(&block).unwrap();
        assert_eq!(value["id"], "exec-summary");
        assert_eq!(value["type"], "paragraph");
        assert_eq!(value["text"], "Revenue grew **18%** [^sec10]");
        assert_eq!(value["source_ids"][0], "sec10");
    }

    #[test]
    fn rich_and_primitive_cells_round_trip() {
        let rich: CellValue = serde_json::from_value(serde_json::json!({
            "text": "n/a",
            "class_name": "muted"
        }))
        .unwrap();
        assert_eq!(rich.display_text(), "n/a");
        assert_eq!(rich.rich_class(), Some("muted"));

        let number: CellValue = serde_json::from_value(serde_json::json!(12.5)).unwrap();
        assert_eq!(number.display_text(), "12.5");
    }

    #[test]
    fn number_format_is_stable() {
        assert_eq!(format_number(1.0), "1");
        assert_eq!(format_number(1.25), "1.25");
        assert_eq!(format_number(-0.5), "-0.5");
    }

    #[test]
    fn semantic_ids() {
        assert!(is_semantic_block_id("q3-revenue"));
        assert!(!is_semantic_block_id(""));
        assert!(!is_semantic_block_id("has space"));
        assert!(looks_like_uuid_v4("123e4567-e89b-42d3-a456-426614174000"));
        assert!(!looks_like_uuid_v4("123e4567-e89b-12d3-a456-426614174000"));
    }

    #[test]
    fn table_shorthand_infers_columns_and_types() {
        let value = serde_json::json!({
            "id": "financials",
            "type": "table",
            "title": "Revenue",
            "headers": ["Region", "Revenue", "Growth", "Reported"],
            "rows": [
                ["APAC", "$1,200", "12.5%", "2026-01-15"],
                ["EMEA", "$900", "9.0%", "2026-02-01"]
            ],
            "summary_rows": [["Total", "$2,100", "10.8%", "2026-02-01"]]
        });
        let block: Block = serde_json::from_value(value).expect("shorthand table");
        let BlockKind::Table(table) = &block.body else {
            panic!("expected table");
        };
        assert_eq!(table.title.as_deref(), Some("Revenue"));
        assert_eq!(table.columns.len(), 4);
        assert_eq!(table.columns[0].data_type, Some(ColumnDataType::Text));
        assert_eq!(table.columns[1].data_type, Some(ColumnDataType::Currency));
        assert_eq!(table.columns[2].data_type, Some(ColumnDataType::Percent));
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.summary_rows.len(), 1);
        assert_eq!(
            table.rows[0].get("region").map(CellValue::display_text),
            Some("APAC".to_string())
        );
    }

    #[test]
    fn table_caption_alias_and_numbering_defaults() {
        let value = serde_json::json!({
            "id": "t",
            "type": "table",
            "caption": "Legacy caption",
            "headers": ["A"],
            "rows": [["1"]]
        });
        let block: Block = serde_json::from_value(value).unwrap();
        let BlockKind::Table(table) = block.body else {
            panic!("table");
        };
        assert_eq!(table.title.as_deref(), Some("Legacy caption"));
        assert_eq!(table.numbering_kind(), NumberingKind::Table);
    }

    #[test]
    fn empty_block_id_deserializes_to_pending_placeholder() {
        let block: Block = serde_json::from_value(serde_json::json!({
            "id": "",
            "type": "paragraph",
            "text": "hello"
        }))
        .unwrap();
        assert!(block.id.is_pending());
    }

    #[test]
    fn table_schema_exposes_columns_and_headers_shapes() {
        let schema = serde_json::to_value(schemars::schema_for!(TableBlock))
            .expect("table schema serializes");
        let text = serde_json::to_string(&schema).expect("schema text");
        assert!(text.contains("\"columns\""), "{text}");
        assert!(text.contains("\"headers\""), "{text}");
        assert!(text.contains("\"oneOf\""), "{text}");
        assert!(text.contains("\"summary_rows\""), "{text}");
    }

    #[test]
    fn echarts_requires_native_option_and_accepts_caption_alias() {
        let missing = serde_json::from_value::<Block>(serde_json::json!({
            "id": "chart",
            "type": "echarts",
            "fallback_summary": "Two points"
        }));
        assert!(missing.is_err(), "option must be required");

        let legacy_mode = serde_json::from_value::<Block>(serde_json::json!({
            "id": "chart",
            "type": "echarts",
            "mode": "option",
            "option": {"series": [{"type": "bar", "data": [1]}]},
            "fallback_summary": "One point"
        }));
        assert!(
            legacy_mode.is_err(),
            "removed chart mode/preset fields must not be accepted"
        );

        let legacy: Block = serde_json::from_value(serde_json::json!({
            "id": "chart",
            "type": "echarts",
            "caption": "Legacy caption",
            "option": {"series": [{"type": "bar", "data": [1]}]},
            "fallback_summary": "One point"
        }))
        .unwrap();
        let BlockKind::Echarts(legacy) = legacy.body else {
            panic!("chart");
        };
        assert_eq!(legacy.title.as_deref(), Some("Legacy caption"));
        assert_eq!(legacy.option["series"][0]["data"], serde_json::json!([1]));
    }
}
