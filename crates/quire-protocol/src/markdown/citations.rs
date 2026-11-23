use super::*;

pub fn citation_style_label(style: CitationStyle) -> &'static str {
    match style {
        CitationStyle::Numeric => "numeric",
        CitationStyle::AuthorYear => "author_year",
    }
}

/// Resolve citation order for callers that need numbering metadata alongside Markdown.
pub fn citation_numbers(document: &ReportDocument) -> std::collections::BTreeMap<SourceId, usize> {
    resolve_citations(document).numbers.into_iter().collect()
}
