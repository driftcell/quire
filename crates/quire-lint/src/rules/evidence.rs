//! Shared recognition of quantitative evidence in prose.

/// Whether text contains a number attached to a meaningful quantitative unit.
/// Bare years, durations, section numbers and list ordinals intentionally do not qualify.
pub(super) fn has_quantitative_evidence(text: &str) -> bool {
    let has_currency = ['¥', '$', '€', '£', '￥']
        .iter()
        .any(|symbol| text.contains(*symbol));
    if has_currency && text.chars().any(|ch| ch.is_ascii_digit()) {
        return true;
    }

    for (index, ch) in text.char_indices() {
        if !ch.is_ascii_digit() {
            continue;
        }
        let suffix = text
            .get(index + ch.len_utf8()..)
            .unwrap_or_default()
            .trim_start();
        if suffix.chars().next().is_some_and(|unit| {
            matches!(
                unit,
                '%' | '％'
                    | '‰'
                    | '万'
                    | '亿'
                    | '元'
                    | '人'
                    | '项'
                    | '篇'
                    | '所'
                    | '名'
                    | '位'
                    | '倍'
                    | '分'
            )
        }) || [
            "百分点",
            "个百分点",
            "博士点",
            "硕士点",
            "个博士点",
            "个硕士点",
        ]
        .iter()
        .any(|unit| suffix.starts_with(unit))
        {
            return true;
        }
    }
    false
}
