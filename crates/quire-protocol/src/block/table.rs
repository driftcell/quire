use super::*;

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
