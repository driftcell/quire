use super::*;

/// ECharts 6 default theme palette, used when a chart omits `option.color`.
const ECHARTS6_DEFAULT_PALETTE: [&str; 9] = [
    "#5070dd", "#b6d634", "#505372", "#ff994d", "#0ca8df", "#ffd10a", "#fb628b", "#785db0",
    "#3fbe95",
];

const IGNORED_ENTITY_NAMES: [&str; 18] = [
    "总计",
    "合计",
    "平均值",
    "均值",
    "占比",
    "其他",
    "小计",
    "总数",
    "总和",
    "平均",
    "total",
    "sum",
    "average",
    "avg",
    "mean",
    "share",
    "other",
    "others",
];

#[derive(Debug)]
struct ChartColorOccurrence {
    color: String,
    path: String,
}

enum SeriesColor {
    Missing,
    Invalid,
    Explicit(String, String),
}

pub(super) fn check_cross_chart_colors(context: &LintContext<'_>, engine: &mut Engine) {
    let document = context.document;
    let mut entity_baseline: BTreeMap<String, (String, BlockId)> = BTreeMap::new();
    let mut palette_baseline: Option<(Vec<String>, BlockId)> = None;

    for (block_index, block) in document.blocks.iter().enumerate() {
        if !matches!(&block.body, BlockKind::Echarts(_)) {
            continue;
        }
        let Some(option) = context.effective_option(block_index) else {
            continue;
        };
        let explicit = explicit_palette(option);
        if let Some(palette) = &explicit {
            if let Some((baseline, baseline_block)) = &palette_baseline {
                if palette != baseline {
                    engine.report(
                        RuleId::EchartsPaletteInconsistent,
                        format!(
                            "chart `{}` explicitly configures a different `option.color` sequence than `{baseline_block}`",
                            block.id
                        ),
                        format!(
                            "chart `{}` uses a different `option.color` palette than `{baseline_block}`; unify the report-wide ECharts palette and apply it with `option_patch`",
                            block.id
                        ),
                        Some(block.id.clone()),
                        Some(format!("blocks[{block_index}].option.color")),
                    );
                }
            } else {
                palette_baseline = Some((palette.clone(), block.id.clone()));
            }
        }

        let palette = explicit.unwrap_or_else(default_palette);
        for (entity, occurrence) in chart_entity_colors(option, &palette) {
            if is_ignored_entity(&entity) {
                continue;
            }
            if let Some((baseline_color, baseline_block)) = entity_baseline.get(&entity) {
                if baseline_color != &occurrence.color && baseline_block != &block.id {
                    engine.report(
                        RuleId::EchartsEntityColorInconsistent,
                        format!(
                            "chart `{}` colors entity `{entity}` as `{}`, but `{baseline_block}` first used `{baseline_color}`",
                            block.id, occurrence.color
                        ),
                        format!(
                            "entity `{entity}` uses `{baseline_color}` in `{baseline_block}` but `{}` in `{}`; align `itemStyle.color` or `option.color` via `option_patch`",
                            occurrence.color, block.id
                        ),
                        Some(block.id.clone()),
                        Some(format!("blocks[{block_index}]{}", occurrence.path)),
                    );
                }
            } else {
                entity_baseline.insert(entity, (occurrence.color, block.id.clone()));
            }
        }
    }
}

fn explicit_palette(option: &Value) -> Option<Vec<String>> {
    let colors = option.get("color")?.as_array()?;
    let palette = colors
        .iter()
        .filter_map(normalize_color)
        .collect::<Vec<_>>();
    (!palette.is_empty()).then_some(palette)
}

fn default_palette() -> Vec<String> {
    ECHARTS6_DEFAULT_PALETTE
        .iter()
        .map(|color| (*color).to_string())
        .collect()
}

fn chart_entity_colors(
    option: &Value,
    palette: &[String],
) -> BTreeMap<String, ChartColorOccurrence> {
    let mut entities = BTreeMap::new();
    for (series_index, series) in chart_series(option).into_iter().enumerate() {
        let kind = series
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let item_series = matches!(kind, "pie" | "funnel");
        let series_color = series_color(series, series_index);

        if !item_series && let Some(name) = entity_name(series) {
            match series_color {
                SeriesColor::Explicit(color, path) => {
                    entities.insert(name, ChartColorOccurrence { color, path });
                }
                SeriesColor::Missing => {
                    if let Some(color) = palette_color(palette, series_index) {
                        entities.insert(
                            name,
                            ChartColorOccurrence {
                                color,
                                path: format!(".series[{series_index}].itemStyle.color"),
                            },
                        );
                    }
                }
                SeriesColor::Invalid => {}
            }
        }

        let Some(data) = series.get("data").and_then(Value::as_array) else {
            continue;
        };
        for (item_index, item) in data.iter().enumerate() {
            let Some(name) = entity_name(item) else {
                continue;
            };
            let item_color = item.get("itemStyle").and_then(|style| style.get("color"));
            let normalized_item_color = item_color.and_then(normalize_color);
            let (color, path) = if let Some(color) = normalized_item_color {
                (
                    color,
                    format!(".series[{series_index}].data[{item_index}].itemStyle.color"),
                )
            } else if item_color.is_some() {
                // Gradient objects and other non-string colors are intentionally unsupported.
                continue;
            } else if item_series {
                if let Some(series_item_color) =
                    series.get("itemStyle").and_then(|style| style.get("color"))
                {
                    let Some(color) = normalize_color(series_item_color) else {
                        continue;
                    };
                    (color, format!(".series[{series_index}].itemStyle.color"))
                } else if let Some(color) = palette_color(palette, item_index) {
                    (color, format!(".series[{series_index}].data[{item_index}]"))
                } else {
                    continue;
                }
            } else {
                // Non-pie item colors are only meaningful when explicitly authored.
                continue;
            };
            entities.insert(name, ChartColorOccurrence { color, path });
        }
    }
    entities
}

fn series_color(series: &Value, series_index: usize) -> SeriesColor {
    for (key, path) in [
        ("itemStyle", "itemStyle.color"),
        ("lineStyle", "lineStyle.color"),
    ] {
        let Some(raw) = series.get(key).and_then(|style| style.get("color")) else {
            continue;
        };
        return match normalize_color(raw) {
            Some(color) => SeriesColor::Explicit(color, format!(".series[{series_index}].{path}")),
            None => SeriesColor::Invalid,
        };
    }
    SeriesColor::Missing
}

fn entity_name(value: &Value) -> Option<String> {
    value
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn palette_color(palette: &[String], index: usize) -> Option<String> {
    if palette.is_empty() {
        return None;
    }
    palette.get(index % palette.len()).cloned()
}

fn normalize_color(value: &Value) -> Option<String> {
    value.as_str().and_then(normalize_color_text)
}

fn normalize_color_text(raw: &str) -> Option<String> {
    let text = raw.trim().to_ascii_lowercase();
    if text.is_empty() {
        return None;
    }
    if let Some(hex) = text.strip_prefix('#')
        && hex.chars().all(|ch| ch.is_ascii_hexdigit())
        && matches!(hex.len(), 3 | 4)
    {
        let mut expanded = String::with_capacity(1 + hex.len() * 2);
        expanded.push('#');
        for ch in hex.chars() {
            expanded.push(ch);
            expanded.push(ch);
        }
        return Some(expanded);
    }
    Some(text)
}

fn is_ignored_entity(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lowered = trimmed.to_ascii_lowercase();
    if IGNORED_ENTITY_NAMES.contains(&lowered.as_str()) {
        return true;
    }
    let numeric = trimmed.strip_suffix('年').unwrap_or(trimmed).trim();
    !numeric.is_empty()
        && numeric
            .chars()
            .all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | ',' | '-' | '+' | '%' | '％' | ' '))
}
