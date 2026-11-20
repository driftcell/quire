//! CSS, custom HTML, theme, cover and TOC checks.

use quire_protocol::{
    BlockKind, ReportDocument, css_brace_issue, is_safe_css_token_value, is_valid_css_token_name,
};

use crate::{engine::Engine, registry::RuleId};

pub(crate) fn check(document: &ReportDocument, engine: &mut Engine<'_>) {
    check_theme(document, engine);
    check_toc(document, engine);
    check_cover(document, engine);
    check_custom_html(document, engine);
}

fn check_theme(document: &ReportDocument, engine: &mut Engine<'_>) {
    if let Some(issue) = css_brace_issue(&document.theme.custom_css) {
        engine.report(
            RuleId::CssSyntaxError,
            format!(
                "theme.custom_css error at line {}, column {}: {}",
                issue.line, issue.column, issue.message
            ),
            "fix the reported brace/string/comment and retry update_style_and_css",
            None,
            Some("theme.custom_css".into()),
        );
    }
    for (name, value) in &document.theme.tokens {
        if !is_valid_css_token_name(name) {
            engine.report(
                RuleId::ThemeTokenName,
                format!("theme token `{name}` is not a valid CSS custom property"),
                "token names must start with `--` and contain only ASCII letters, digits, '-' or '_'",
                None,
                Some(format!("theme.tokens.{name}")),
            );
        } else if !is_safe_css_token_value(value) {
            engine.report(
                RuleId::ThemeTokenValue,
                format!("theme token `{name}` has an unsafe value"),
                "avoid `;`, `{`, `}`, `<`, `>` and backslashes in token values",
                None,
                Some(format!("theme.tokens.{name}")),
            );
        }
    }
    if document.theme.preset.trim().is_empty() {
        engine.report(
            RuleId::ThemePresetEmpty,
            "theme preset is empty",
            "use `default`, `editorial`, `technical`, or a frontend-supported preset name",
            None,
            Some("theme.preset".into()),
        );
    }
}

fn check_toc(document: &ReportDocument, engine: &mut Engine<'_>) {
    if !(1..=3).contains(&document.toc.max_depth) {
        engine.report(
            RuleId::TocDepthRange,
            format!(
                "toc.max_depth is {} but the protocol allows 1..=3",
                document.toc.max_depth
            ),
            "set max_depth to 1, 2 or 3",
            None,
            Some("toc.max_depth".into()),
        );
    }
    if document.toc.enabled && document.toc.title.trim().is_empty() {
        engine.report(
            RuleId::TocTitleEmpty,
            "TOC is enabled but its title is empty",
            "set a title such as `Contents` or disable the TOC",
            None,
            Some("toc.title".into()),
        );
    }
}

fn check_cover(document: &ReportDocument, engine: &mut Engine<'_>) {
    if document.cover.enabled
        && document.cover.title.trim().is_empty()
        && document.title() == document.doc_id.as_str()
    {
        engine.report(
            RuleId::CoverTitleEmpty,
            "cover is enabled without a title and no h1 heading exists",
            "set cover.title or add an h1 heading",
            None,
            Some("cover.title".into()),
        );
    }
    if document.cover.enabled && document.cover.variant.trim().is_empty() {
        engine.report(
            RuleId::CoverVariantEmpty,
            "cover variant is empty",
            "use classic, modern, minimal, split, custom, or a frontend-supported variant",
            None,
            Some("cover.variant".into()),
        );
    }
}

fn check_custom_html(document: &ReportDocument, engine: &mut Engine<'_>) {
    for (index, block) in document.blocks.iter().enumerate() {
        let BlockKind::CustomHtml(html) = &block.body else {
            continue;
        };
        if html.fallback_text.trim().is_empty() {
            engine.report(
                RuleId::CustomHtmlFallbackEmpty,
                format!("custom_html block `{}` has empty fallback_text", block.id),
                "provide a non-empty Markdown/plain-text fallback for agents and accessibility",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].fallback_text")),
            );
        }
        if html.html.trim().is_empty() {
            engine.report(
                RuleId::CustomHtmlEmpty,
                format!("custom_html block `{}` has empty html", block.id),
                "supply raw HTML or remove the block",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].html")),
            );
        }
        if let Some(css) = html.css.as_deref()
            && let Some(issue) = css_brace_issue(css)
        {
            engine.report(
                RuleId::CssSyntaxError,
                format!(
                    "custom_html block `{}` CSS error at line {}, column {}: {}",
                    block.id, issue.line, issue.column, issue.message
                ),
                "balance braces and close strings/comments before saving",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].css")),
            );
        }
        if has_external_resource(&html.html, html.css.as_deref()) {
            engine.report(
                RuleId::CustomHtmlExternalResource,
                format!(
                    "custom_html block `{}` loads an external image/CSS resource",
                    block.id
                ),
                "inline or bundle the resource; offline SSG/PDF exports cannot fetch remote URLs.",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].html")),
            );
        }
        if has_fixed_overflow(&html.html, html.css.as_deref()) {
            engine.report(
                RuleId::CustomHtmlFixedOverflow,
                format!(
                    "custom_html block `{}` uses a fixed width that can overflow A4",
                    block.id
                ),
                "use responsive widths (`width:100%`, `max-width`) below about 780px so the block fits a 640px A4 content area.",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].html")),
            );
        }
        if let Some(title) = html
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
            && html_has_duplicate_title(&html.html, title)
        {
            engine.report(
                RuleId::DuplicateHtmlTitle,
                format!(
                    "custom_html block `{}` repeats its outer title inside a leading heading/.title element",
                    block.id
                ),
                "Put titles in outer `title` ONLY; remove the duplicate heading/title element from the HTML.",
                Some(block.id.clone()),
                Some(format!("blocks[{index}].html")),
            );
        }
    }
}

fn has_external_resource(html: &str, css: Option<&str>) -> bool {
    let combined = format!("{}\n{}", html, css.unwrap_or_default()).to_ascii_lowercase();
    ["src=", "href=", "url("].iter().any(|marker| {
        combined.match_indices(marker).any(|(position, _)| {
            let rest = combined
                .get(position + marker.len()..)
                .unwrap_or_default()
                .trim_start_matches(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\''));
            rest.starts_with("http://") || rest.starts_with("https://") || rest.starts_with("//")
        })
    })
}

fn has_fixed_overflow(html: &str, css: Option<&str>) -> bool {
    let combined = format!("{}\n{}", html, css.unwrap_or_default());
    combined.split([';', '{', '}']).any(|declaration| {
        let Some((property, value)) = declaration.split_once(':') else {
            return false;
        };
        let property = property
            .rsplit('=')
            .next()
            .unwrap_or(property)
            .trim()
            .trim_matches(|ch: char| matches!(ch, '"' | '\''))
            .to_ascii_lowercase();
        if property != "width" && property != "min-width" {
            return false;
        }
        let value = value.trim().trim_start_matches(['"', '\'']).trim();
        let Some(px_index) = value.find("px") else {
            return false;
        };
        let Some(number) = value.get(..px_index) else {
            return false;
        };
        number
            .trim()
            .parse::<f64>()
            .is_ok_and(|width| width >= 780.0)
    })
}

fn html_has_duplicate_title(html: &str, title: &str) -> bool {
    let mut rest = html.trim_start();
    while let Some(stripped) = rest.strip_prefix("<!--") {
        let Some(end) = stripped.find("-->") else {
            return false;
        };
        rest = stripped.get(end + 3..).unwrap_or_default().trim_start();
    }
    let Some(rest) = rest.strip_prefix('<') else {
        return false;
    };
    let Some(tag_end) = rest.find('>') else {
        return false;
    };
    let Some(tag_body) = rest.get(..tag_end) else {
        return false;
    };
    let tag_name = tag_body
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_end_matches('/')
        .to_ascii_lowercase();
    let is_heading = matches!(tag_name.as_str(), "h1" | "h2" | "h3" | "h4");
    let tag_lower = tag_body.to_ascii_lowercase();
    let is_title_class = tag_lower.contains("class=") && tag_lower.contains("title");
    if !is_heading && !is_title_class {
        return false;
    }
    let Some(after_open) = rest.get(tag_end + 1..) else {
        return false;
    };
    let closing = format!("</{tag_name}>");
    let inner = after_open
        .to_ascii_lowercase()
        .find(&closing)
        .and_then(|end| after_open.get(..end))
        .unwrap_or(after_open);
    let inner = strip_tags(inner);
    let inner = normalize_title_text(&inner);
    let title = normalize_title_text(title);
    !inner.is_empty() && inner == title
}

fn strip_tags(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

fn normalize_title_text(input: &str) -> String {
    input
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}
