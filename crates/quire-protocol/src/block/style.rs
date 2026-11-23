use super::*;

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
