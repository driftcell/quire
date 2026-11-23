use super::{
    data_projection::{
        cartesian_tables, hierarchy_tables, json_scalar_label, radar_tables, relation_tables,
    },
    *,
};

/// Maximum rows emitted for one chart data table.
pub(crate) const MAX_CHART_ROWS: usize = 200;

/// Renderer-agnostic tabular projection of an ECharts option.
///
/// The same tables power Markdown review and the frontend `computed.chart_tables` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChartDataTable {
    pub title: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Extract all structured data tables from a merged ECharts option.
pub fn chart_data_tables(option: &Value) -> Vec<ChartDataTable> {
    let series_entries = series(option);
    let mut tables = dataset_tables(option);
    tables.extend(relation_tables(&series_entries));
    tables.extend(radar_tables(option, &series_entries));
    tables.extend(hierarchy_tables(&series_entries));
    tables.extend(cartesian_tables(option, &series_entries));
    tables
}

fn dataset_tables(option: &Value) -> Vec<ChartDataTable> {
    let view = EchartsOptionView::from_value(option);
    let mut tables = Vec::new();
    for (index, dataset) in view.dataset.iter().enumerate() {
        let Some(source) = dataset.source.as_ref() else {
            continue;
        };
        let rows: Vec<&Value> = match source {
            Value::Array(items) => items.iter().collect(),
            Value::Object(_) => vec![source],
            _ => continue,
        };
        let Some((headers, mut body)) = dataset_shape(&rows) else {
            continue;
        };
        if headers.is_empty() || body.is_empty() {
            continue;
        }
        body.truncate(MAX_CHART_ROWS);
        let label = dataset
            .id
            .clone()
            .unwrap_or_else(|| format!("Dataset {}", index + 1));
        tables.push(ChartDataTable {
            title: format!("{label} (dataset)"),
            headers,
            rows: body,
        });
    }
    tables
}

fn dataset_shape(rows: &[&Value]) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    let object_rows: Vec<&serde_json::Map<String, Value>> =
        rows.iter().filter_map(|row| row.as_object()).collect();
    if object_rows.len() == rows.len() && !object_rows.is_empty() {
        let mut header: Vec<String> = Vec::new();
        for row in &object_rows {
            for key in row.keys() {
                if !header.contains(key) {
                    header.push(key.clone());
                }
            }
        }
        let body: Vec<Vec<String>> = object_rows
            .iter()
            .map(|row| {
                header
                    .iter()
                    .map(|key| row.get(key).map(json_scalar_label).unwrap_or_default())
                    .collect()
            })
            .collect();
        return Some((header, body));
    }
    let array_rows: Vec<&Vec<Value>> = rows.iter().filter_map(|row| row.as_array()).collect();
    if array_rows.len() != rows.len() || array_rows.is_empty() {
        return None;
    }
    let has_header = rows.len() > 1
        && rows.first().is_some_and(|row| {
            row.as_array()
                .is_some_and(|cells| !cells.is_empty() && cells.iter().all(Value::is_string))
        });
    let width = array_rows.iter().map(|row| row.len()).max().unwrap_or(0);
    if width == 0 {
        return None;
    }
    let header: Vec<String> = if has_header {
        rows.first()
            .and_then(|row| row.as_array())
            .map(|cells| cells.iter().map(json_scalar_label).collect())
            .unwrap_or_default()
    } else {
        (1..=width)
            .map(|column| format!("Column {column}"))
            .collect()
    };
    let body: Vec<Vec<String>> = rows
        .iter()
        .skip(usize::from(has_header))
        .filter_map(|row| row.as_array())
        .map(|cells| {
            (0..header.len())
                .map(|index| cells.get(index).map(json_scalar_label).unwrap_or_default())
                .collect()
        })
        .collect();
    Some((header, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::EchartsBlock;

    fn chart(option: Value) -> EchartsBlock {
        EchartsBlock {
            title: None,
            height: 360,
            option,
            option_patch: None,
            fallback_summary: "Summary".into(),
            numbering: None,
            note: None,
        }
    }

    #[test]
    fn effective_option_merges_the_incremental_patch() {
        let base = chart(serde_json::json!({
            "series": [{"type": "bar", "data": [1]}],
            "title": {"text": "Base"}
        }));
        let merged = merge_chart_option_patch(
            Some(&base.option),
            &serde_json::json!({
                "series": [{"type": "bar", "data": [2, 3]}],
                "title": {"subtext": "Patched"}
            }),
        );
        let mut patched = base;
        patched.option_patch = Some(merged);

        let effective = effective_chart_option(&patched);
        assert_eq!(effective["title"]["text"], "Base");
        assert_eq!(effective["title"]["subtext"], "Patched");
        assert_eq!(effective["series"][0]["data"], serde_json::json!([2, 3]));
    }

    #[test]
    fn chart_data_tables_uses_each_panels_axis_categories() {
        let option = serde_json::json!({
            "grid": [{}, {}],
            "xAxis": [
                {"gridIndex": 0, "data": ["Q1", "Q2"]},
                {"gridIndex": 1, "data": ["East", "West"]}
            ],
            "yAxis": [{"gridIndex": 0}, {"gridIndex": 1}],
            "series": [
                {"name": "Revenue", "type": "bar", "xAxisIndex": 0, "yAxisIndex": 0, "data": [10, 20]},
                {"name": "Share", "type": "line", "xAxisIndex": 1, "yAxisIndex": 1, "data": [30, 40]}
            ]
        });
        let tables = chart_data_tables(&option);
        assert_eq!(tables.len(), 2, "{tables:?}");
        let revenue = tables
            .iter()
            .find(|table| table.title.contains("Revenue"))
            .expect("revenue table");
        assert_eq!(
            revenue.rows,
            vec![
                vec!["Q1".to_string(), "10".to_string()],
                vec!["Q2".to_string(), "20".to_string()]
            ]
        );
        let share = tables
            .iter()
            .find(|table| table.title.contains("Share"))
            .expect("share table");
        assert_eq!(
            share.rows,
            vec![
                vec!["East".to_string(), "30".to_string()],
                vec!["West".to_string(), "40".to_string()]
            ]
        );
    }

    #[test]
    fn chart_data_tables_keeps_dual_axis_series_in_one_panel() {
        let option = serde_json::json!({
            "xAxis": {"data": ["Q1", "Q2"]},
            "yAxis": [{}, {}],
            "series": [
                {"name": "Revenue", "type": "bar", "yAxisIndex": 0, "data": [10, 20]},
                {"name": "Margin", "type": "line", "yAxisIndex": 1, "data": [62.5, 64.0]}
            ]
        });
        let tables = chart_data_tables(&option);
        assert_eq!(tables.len(), 1, "{tables:?}");
        assert_eq!(tables[0].headers, vec!["Category", "Revenue", "Margin"]);
        assert_eq!(tables[0].rows[0], vec!["Q1", "10", "62.5"]);
    }

    #[test]
    fn chart_data_tables_includes_every_dataset_and_non_cartesian_series() {
        let option = serde_json::json!({
            "xAxis": {"data": ["Q1", "Q2"]},
            "dataset": [
                {"id": "north", "source": [["Quarter", "Value"], ["Q1", 10], ["Q2", 20]]},
                {"id": "south", "source": [["Quarter", "Value"], ["Q1", 30], ["Q2", 40]]}
            ],
            "series": [
                {"name": "Revenue", "type": "bar", "data": [10, 20]},
                {"name": "Mix", "type": "pie", "data": [{"name": "A", "value": 60}, {"name": "B", "value": 40}]}
            ]
        });
        let tables = chart_data_tables(&option);
        assert!(tables.iter().any(|table| table.title.contains("north")));
        assert!(tables.iter().any(|table| table.title.contains("south")));
        assert!(tables.iter().any(|table| table.title.contains("Revenue")));
        let pie = tables
            .iter()
            .find(|table| table.title.contains("Mix"))
            .expect("pie table");
        assert_eq!(
            pie.rows,
            vec![
                vec!["A".to_string(), "60".to_string()],
                vec!["B".to_string(), "40".to_string()]
            ]
        );
    }
}
