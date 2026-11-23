//! Typed structural views over open-ended ECharts options.
//!
//! Canonical options remain `serde_json::Value`. These views model only the fields used by lint and
//! projections, tolerate malformed leaf values, and preserve object-vs-array path semantics.

use serde::{Deserialize, Deserializer};
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct ComponentList<T> {
    pub items: Vec<T>,
    pub is_array: bool,
}

impl<T> ComponentList<T> {
    pub fn configured_count(&self) -> Option<usize> {
        if self.items.is_empty() {
            None
        } else {
            Some(self.items.len())
        }
    }
}

impl<T> std::ops::Deref for ComponentList<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.items
    }
}

impl<'de, T> Deserialize<'de> for ComponentList<T>
where
    T: Deserialize<'de> + Default,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::Array(items) => Ok(Self {
                items: items
                    .into_iter()
                    .map(|item| T::deserialize(item).unwrap_or_default())
                    .collect(),
                is_array: true,
            }),
            value @ Value::Object(_) => Ok(Self {
                items: vec![T::deserialize(value).unwrap_or_default()],
                is_array: false,
            }),
            _ => Ok(Self::default()),
        }
    }
}

fn lenient_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer).ok().flatten())
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EchartsTitleView {
    #[serde(deserialize_with = "lenient_option")]
    pub text: Option<String>,
    #[serde(deserialize_with = "lenient_option")]
    pub subtext: Option<String>,
    #[serde(deserialize_with = "lenient_option")]
    pub top: Option<String>,
    #[serde(deserialize_with = "lenient_option")]
    pub left: Option<String>,
    #[serde(deserialize_with = "lenient_option")]
    pub right: Option<String>,
    #[serde(deserialize_with = "lenient_option")]
    pub bottom: Option<String>,
    #[serde(deserialize_with = "lenient_option")]
    pub text_style: Option<EchartsTextStyleView>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EchartsTextStyleView {
    #[serde(deserialize_with = "lenient_option")]
    pub font_size: Option<f64>,
}

impl EchartsTitleView {
    pub fn duplicates(&self, outer_title: &str) -> bool {
        let outer = normalized_title(outer_title);
        if outer.is_empty() {
            return false;
        }
        [&self.text, &self.subtext].into_iter().any(|text| {
            text.as_deref()
                .map(normalized_title)
                .is_some_and(|text| !text.is_empty() && text == outer)
        })
    }

    pub fn is_center(&self) -> bool {
        self.top
            .as_deref()
            .map(str::to_ascii_lowercase)
            .is_some_and(|top| matches!(top.as_str(), "center" | "middle"))
    }

    pub fn is_positioned_or_light(&self) -> bool {
        let positioned = [&self.left, &self.right, &self.bottom]
            .into_iter()
            .any(|value| value.as_deref().is_some_and(|value| !value.is_empty()));
        let light = self
            .text_style
            .as_ref()
            .and_then(|style| style.font_size)
            .is_some_and(|size| size <= 13.0);
        positioned || light
    }
}

fn normalized_title(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EchartsLabelLayoutView {
    #[serde(deserialize_with = "lenient_option")]
    pub hide_overlap: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EchartsEndLabelView {
    #[serde(deserialize_with = "lenient_option")]
    pub show: Option<bool>,
    #[serde(deserialize_with = "lenient_option")]
    pub text_border_width: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EchartsAxisLabelView {
    #[serde(deserialize_with = "lenient_option")]
    pub rotate: Option<Value>,
    #[serde(deserialize_with = "lenient_option")]
    pub interval: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EchartsAxisView {
    #[serde(rename = "type", deserialize_with = "lenient_option")]
    pub kind: Option<String>,
    #[serde(deserialize_with = "lenient_option")]
    pub grid_index: Option<i64>,
    #[serde(deserialize_with = "lenient_option")]
    pub data: Option<Vec<Value>>,
    #[serde(deserialize_with = "lenient_option")]
    pub axis_label: Option<EchartsAxisLabelView>,
    #[serde(deserialize_with = "lenient_option")]
    pub min: Option<f64>,
    #[serde(deserialize_with = "lenient_option")]
    pub max: Option<f64>,
    #[serde(deserialize_with = "lenient_option")]
    pub interval: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EchartsDatasetView {
    #[serde(deserialize_with = "lenient_option")]
    pub id: Option<String>,
    #[serde(deserialize_with = "lenient_option")]
    pub source: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EchartsSeriesView {
    #[serde(rename = "type", deserialize_with = "lenient_option")]
    pub kind: Option<String>,
    #[serde(deserialize_with = "lenient_option")]
    pub coordinate_system: Option<String>,
    #[serde(deserialize_with = "lenient_option")]
    pub data: Option<Value>,
    #[serde(deserialize_with = "lenient_option")]
    pub links: Option<Value>,
    #[serde(deserialize_with = "lenient_option")]
    pub edges: Option<Value>,
    #[serde(deserialize_with = "lenient_option")]
    pub x_axis_index: Option<Value>,
    #[serde(deserialize_with = "lenient_option")]
    pub y_axis_index: Option<Value>,
    #[serde(deserialize_with = "lenient_option")]
    pub grid_index: Option<Value>,
    #[serde(deserialize_with = "lenient_option")]
    pub dataset_index: Option<Value>,
    #[serde(deserialize_with = "lenient_option")]
    pub radar_index: Option<Value>,
    #[serde(deserialize_with = "lenient_option")]
    pub polar_index: Option<Value>,
    #[serde(deserialize_with = "lenient_option")]
    pub end_label: Option<EchartsEndLabelView>,
    #[serde(deserialize_with = "lenient_option")]
    pub label_layout: Option<EchartsLabelLayoutView>,
}

#[derive(Debug, Clone, Copy)]
pub enum EchartsComponent {
    XAxis,
    YAxis,
    Grid,
    Dataset,
    Radar,
    Polar,
}

impl EchartsComponent {
    pub fn key(self) -> &'static str {
        match self {
            Self::XAxis => "xAxis",
            Self::YAxis => "yAxis",
            Self::Grid => "grid",
            Self::Dataset => "dataset",
            Self::Radar => "radar",
            Self::Polar => "polar",
        }
    }

    fn has_implicit_default(self) -> bool {
        matches!(self, Self::XAxis | Self::YAxis | Self::Grid)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EchartsOptionView {
    pub title: ComponentList<EchartsTitleView>,
    pub series: ComponentList<EchartsSeriesView>,
    pub x_axis: ComponentList<EchartsAxisView>,
    pub y_axis: ComponentList<EchartsAxisView>,
    pub grid: ComponentList<Value>,
    pub dataset: ComponentList<EchartsDatasetView>,
    pub radar: ComponentList<Value>,
    pub polar: ComponentList<Value>,
    #[serde(deserialize_with = "lenient_option")]
    pub matrix: Option<Value>,
    #[serde(deserialize_with = "lenient_option")]
    pub label_layout: Option<EchartsLabelLayoutView>,
}

impl EchartsOptionView {
    pub fn from_value(value: &Value) -> Self {
        serde_json::from_value(value.clone()).unwrap_or_default()
    }

    pub fn axes(&self, component: EchartsComponent) -> &[EchartsAxisView] {
        match component {
            EchartsComponent::XAxis => &self.x_axis,
            EchartsComponent::YAxis => &self.y_axis,
            _ => &[],
        }
    }

    pub fn configured_count(&self, component: EchartsComponent) -> Option<usize> {
        match component {
            EchartsComponent::XAxis => self.x_axis.configured_count(),
            EchartsComponent::YAxis => self.y_axis.configured_count(),
            EchartsComponent::Grid => self.grid.configured_count(),
            EchartsComponent::Dataset => self.dataset.configured_count(),
            EchartsComponent::Radar => self.radar.configured_count(),
            EchartsComponent::Polar => self.polar.configured_count(),
        }
    }

    pub fn axis_count(&self, component: EchartsComponent) -> usize {
        self.configured_count(component)
            .unwrap_or(usize::from(component.has_implicit_default()))
    }

    pub fn has_matrix(&self) -> bool {
        self.matrix.as_ref().is_some_and(|value| !value.is_null())
    }

    pub fn has_data_source(&self) -> bool {
        if self.series.iter().any(|series| {
            [&series.data, &series.links, &series.edges]
                .into_iter()
                .any(|value| non_empty_container(value.as_ref()))
        }) {
            return true;
        }
        self.dataset
            .iter()
            .any(|dataset| non_empty_container(dataset.source.as_ref()))
    }
}

fn non_empty_container(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
        Some(Value::String(text)) => !text.trim().is_empty(),
        Some(Value::Null) | None => false,
        Some(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_preserves_component_shapes_and_invalid_series_slots() {
        let view = EchartsOptionView::from_value(&serde_json::json!({
            "title": {"text": "Revenue"},
            "series": [{"type": "bar", "data": [1]}, 42],
            "xAxis": [{"type": "category", "data": ["A"]}],
            "dataset": {"source": [{"name": "A", "value": 1}]}
        }));
        assert!(!view.title.is_array);
        assert!(view.series.is_array);
        assert_eq!(view.series.len(), 2);
        assert_eq!(
            view.series
                .first()
                .and_then(|series| series.kind.as_deref()),
            Some("bar")
        );
        assert_eq!(
            view.series.get(1).and_then(|series| series.kind.as_deref()),
            None
        );
        assert_eq!(view.axis_count(EchartsComponent::XAxis), 1);
        assert!(view.has_data_source());
    }
}
