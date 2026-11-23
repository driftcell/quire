use super::*;

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
