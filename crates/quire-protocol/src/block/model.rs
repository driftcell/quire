use super::*;

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
