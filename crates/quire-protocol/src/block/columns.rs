use super::*;

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
