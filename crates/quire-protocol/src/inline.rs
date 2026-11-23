//! A deliberately small inline Markdown parser.
//!
//! Supports `**bold**`, `*italic*`, `` `code` ``, `[label](url)`, escaped characters,
//! hard line breaks and inline citations (`[^source_id]`). The output is intentionally a
//! neutral AST so Markdown projection, citation resolution and the linter share identical parsing.

use crate::ids::SourceId;

const MAX_DEPTH: u8 = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineNode {
    Text(String),
    Strong(Vec<InlineNode>),
    Em(Vec<InlineNode>),
    Code(String),
    Link {
        href: String,
        children: Vec<InlineNode>,
    },
    /// An inline citation. The contained value is the raw source ID.
    Citation(SourceId),
    Break,
}

impl InlineNode {
    fn readable_chars(&self) -> usize {
        match self {
            Self::Text(text) | Self::Code(text) => text.chars().count(),
            Self::Em(children) | Self::Strong(children) | Self::Link { children, .. } => {
                children.iter().map(Self::readable_chars).sum()
            }
            Self::Citation(_) => 0,
            Self::Break => 1,
        }
    }

    pub fn plain_text(&self) -> String {
        let mut output = String::new();
        self.write_plain_text(&mut output);
        output
    }

    fn write_plain_text(&self, output: &mut String) {
        match self {
            Self::Text(text) | Self::Code(text) => output.push_str(text),
            Self::Strong(children) | Self::Em(children) => {
                for child in children {
                    child.write_plain_text(output);
                }
            }
            Self::Link { children, .. } => {
                for child in children {
                    child.write_plain_text(output);
                }
            }
            Self::Citation(source_id) => {
                output.push_str("[^");
                output.push_str(source_id);
                output.push(']');
            }
            Self::Break => output.push('\n'),
        }
    }

    fn collect_citations(&self, output: &mut Vec<SourceId>) {
        match self {
            Self::Citation(source_id) => output.push(source_id.clone()),
            Self::Strong(children) | Self::Em(children) => {
                for child in children {
                    child.collect_citations(output);
                }
            }
            Self::Link { children, .. } => {
                for child in children {
                    child.collect_citations(output);
                }
            }
            Self::Text(_) | Self::Code(_) | Self::Break => {}
        }
    }
}

/// Parse inline Markdown into a neutral inline AST.
pub fn parse_inline(input: &str) -> Vec<InlineNode> {
    parse_inner(input, 0)
}

/// Visible text for a length target: count the link label and code literals, not markup or citations.
pub(crate) fn readable_char_count(input: &str) -> usize {
    parse_inline(input)
        .iter()
        .map(InlineNode::readable_chars)
        .sum()
}

/// Return citation source IDs in first-appearance order, including citations nested in marks.
pub fn inline_citations(input: &str) -> Vec<SourceId> {
    let mut output = Vec::new();
    for node in parse_inline(input) {
        node.collect_citations(&mut output);
    }
    output
}

/// Convert inline AST to plain text, dropping formatting but keeping citation markers.
pub fn inline_plain_text(input: &str) -> String {
    parse_inline(input)
        .iter()
        .map(InlineNode::plain_text)
        .collect()
}

fn parse_inner(input: &str, depth: u8) -> Vec<InlineNode> {
    if depth > MAX_DEPTH {
        return vec![InlineNode::Text(input.to_string())];
    }
    let mut parser = Parser {
        input,
        position: 0,
        depth,
        nodes: Vec::new(),
    };
    parser.parse_sequence();
    parser.nodes
}

struct Parser<'a> {
    input: &'a str,
    position: usize,
    depth: u8,
    nodes: Vec<InlineNode>,
}

impl<'a> Parser<'a> {
    fn remaining(&self) -> &'a str {
        &self.input[self.position..]
    }

    fn push_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(InlineNode::Text(existing)) = self.nodes.last_mut() {
            existing.push_str(text);
        } else {
            self.nodes.push(InlineNode::Text(text.to_string()));
        }
    }

    fn advance(&mut self, bytes: usize) {
        let mut target = (self.position + bytes).min(self.input.len());
        while target < self.input.len() && !self.input.is_char_boundary(target) {
            target += 1;
        }
        self.position = target;
    }

    fn parse_sequence(&mut self) {
        while self.position < self.input.len() {
            let remaining = self.remaining();
            if let Some(stripped) = remaining.strip_prefix('\\') {
                if let Some(ch) = stripped.chars().next() {
                    self.push_text(&ch.to_string());
                    self.advance(1 + ch.len_utf8());
                } else {
                    self.push_text("\\");
                    self.advance(1);
                }
                continue;
            }
            if remaining.starts_with('`') {
                if self.try_code_span() {
                    continue;
                }
                self.push_text("`");
                self.advance(1);
                continue;
            }
            if remaining.starts_with("[^") {
                if self.try_citation() {
                    continue;
                }
                self.push_text("[^");
                self.advance(2);
                continue;
            }
            if remaining.starts_with('[') && self.try_link() {
                continue;
            }
            if remaining.starts_with("**") && self.try_delimited("**", true) {
                continue;
            }
            if remaining.starts_with('*') && self.try_delimited("*", false) {
                continue;
            }
            let Some(ch) = remaining.chars().next() else {
                break;
            };
            if ch == '\n' {
                self.nodes.push(InlineNode::Break);
                self.advance(1);
                continue;
            }
            if ch == '\r' {
                self.advance(if remaining.starts_with("\r\n") { 2 } else { 1 });
                self.nodes.push(InlineNode::Break);
                continue;
            }
            if matches!(ch, '\\' | '`' | '[' | '*' | '\n' | '\r') {
                // A marker that failed to open anything is literal text.
                self.push_text(&ch.to_string());
                self.advance(ch.len_utf8());
                continue;
            }
            let mut buffer = String::new();
            buffer.push(ch);
            self.advance(ch.len_utf8());
            // Consume ordinary characters in a batch until the next marker.
            while let Some(next) = self.remaining().chars().next() {
                if matches!(next, '\\' | '`' | '[' | '*' | '\n' | '\r') {
                    break;
                }
                buffer.push(next);
                self.advance(next.len_utf8());
            }
            self.push_text(&buffer);
        }
    }

    fn try_code_span(&mut self) -> bool {
        let remaining = self.remaining();
        let run = remaining.bytes().take_while(|byte| *byte == b'`').count();
        if run == 0 {
            return false;
        }
        let content_start = self.position + run;
        let search = &self.input[content_start..];
        let mut offset = 0;
        while let Some(found) = search[offset..].find('`') {
            let absolute = offset + found;
            let closing_run = search[absolute..]
                .bytes()
                .take_while(|byte| *byte == b'`')
                .count();
            if closing_run == run {
                let raw = &search[..absolute];
                let content = strip_code_span_padding(raw);
                self.nodes.push(InlineNode::Code(content.to_string()));
                self.position = content_start + absolute + run;
                return true;
            }
            offset = absolute + closing_run;
            if offset >= search.len() {
                break;
            }
        }
        false
    }

    fn try_citation(&mut self) -> bool {
        let remaining = self.remaining();
        let Some(end_offset) = remaining[2..].find(']') else {
            return false;
        };
        let raw_id = &remaining[2..2 + end_offset];
        let Ok(source_id) = SourceId::parse(raw_id) else {
            return false;
        };
        self.nodes.push(InlineNode::Citation(source_id));
        self.advance(2 + end_offset + 1);
        true
    }

    fn try_link(&mut self) -> bool {
        let remaining = self.remaining();
        let Some(label_end) = find_balanced(remaining, 0, '[', ']') else {
            return false;
        };
        let after_label = &remaining[label_end + 1..];
        if !after_label.starts_with('(') {
            return false;
        }
        let Some(dest_end) = find_balanced(after_label, 0, '(', ')') else {
            return false;
        };
        let label = &remaining[1..label_end];
        let destination = parse_link_destination(&after_label[1..dest_end]);
        let Some(destination) = destination else {
            return false;
        };
        if label.is_empty() {
            return false;
        }
        let children = parse_inner(label, self.depth + 1);
        if children.is_empty() {
            return false;
        }
        self.nodes.push(InlineNode::Link {
            href: destination,
            children,
        });
        self.advance(label_end + 1 + dest_end + 1);
        true
    }

    fn try_delimited(&mut self, delimiter: &str, strong: bool) -> bool {
        let start = self.position;
        let content_start = start + delimiter.len();
        let search = &self.input[content_start..];
        let Some(close_offset) = find_unescaped(search, delimiter) else {
            return false;
        };
        if close_offset == 0 {
            return false;
        }
        let content = &search[..close_offset];
        let children = parse_inner(content, self.depth + 1);
        if children.is_empty() {
            return false;
        }
        self.nodes.push(if strong {
            InlineNode::Strong(children)
        } else {
            InlineNode::Em(children)
        });
        self.position = content_start + close_offset + delimiter.len();
        true
    }
}

fn strip_code_span_padding(raw: &str) -> &str {
    if raw.len() >= 2
        && raw.starts_with(' ')
        && raw.ends_with(' ')
        && !raw.chars().all(|ch| ch == ' ')
    {
        raw.get(1..raw.len() - 1).unwrap_or(raw)
    } else {
        raw
    }
}

fn find_unescaped(input: &str, needle: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut index = 0;
    while index + needle.len() <= bytes.len() {
        if bytes.get(index) == Some(&b'\\') {
            let Some(escaped) = input.get(index + 1..).and_then(|tail| tail.chars().next()) else {
                break;
            };
            index += 1 + escaped.len_utf8();
            continue;
        }
        if input
            .get(index..)
            .is_some_and(|tail| tail.starts_with(needle))
        {
            return Some(index);
        }
        let Some(ch) = input.get(index..).and_then(|tail| tail.chars().next()) else {
            break;
        };
        index += ch.len_utf8();
    }
    None
}

/// Find the closing delimiter for a balanced pair, honoring escapes and nested pairs.
fn find_balanced(input: &str, start: usize, open: char, close: char) -> Option<usize> {
    if !input[start..].starts_with(open) {
        return None;
    }
    let mut depth = 0usize;
    let mut index = start;
    while index < input.len() {
        let ch = input[index..].chars().next()?;
        if ch == '\\' {
            index += 1;
            if index < input.len() {
                index += input[index..].chars().next()?.len_utf8();
            }
            continue;
        }
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
        index += ch.len_utf8();
    }
    None
}

fn parse_link_destination(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // Markdown allows the destination to be wrapped in angle brackets.
    if raw.starts_with('<') && raw.ends_with('>') {
        let inner = raw[1..raw.len() - 1].trim();
        if inner.is_empty() {
            return None;
        }
        return Some(inner.to_string());
    }
    // Strip an optional title: `[x](url "title")`.
    let mut destination = raw;
    if let Some(space) = raw.find(char::is_whitespace) {
        let title = raw[space..].trim();
        if (title.starts_with('"') && title.ends_with('"'))
            || (title.starts_with('\'') && title.ends_with('\''))
            || (title.starts_with('(') && title.ends_with(')'))
        {
            destination = raw[..space].trim();
        }
    }
    if destination.is_empty() {
        None
    } else {
        Some(destination.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::*;

    #[test]
    fn parses_all_supported_inline_features() {
        let nodes = parse_inline(
            "Growth **18%** and *steady* with `code` [source](https://example.com) [^sec10]",
        );
        assert!(
            nodes
                .iter()
                .any(|node| matches!(node, InlineNode::Strong(_)))
        );
        assert!(nodes.iter().any(|node| matches!(node, InlineNode::Em(_))));
        assert!(
            nodes
                .iter()
                .any(|node| matches!(node, InlineNode::Code(text) if text == "code"))
        );
        assert!(nodes.iter().any(|node| matches!(
            node,
            InlineNode::Link { href, .. } if href == "https://example.com"
        )));
        assert!(
            nodes
                .iter()
                .any(|node| matches!(node, InlineNode::Citation(id) if id == "sec10"))
        );
    }

    #[test]
    fn citations_are_found_in_nested_marks() {
        assert_eq!(
            inline_citations("**bold [^a]** and [link [^b]](https://x.test) [^c]"),
            vec![source_id("a"), source_id("b"), source_id("c")]
        );
    }

    #[test]
    fn unmatched_markers_are_literal() {
        assert_eq!(
            inline_plain_text("2 * 3 = 6 and [broken"),
            "2 * 3 = 6 and [broken"
        );
        assert_eq!(inline_plain_text("unclosed **bold"), "unclosed **bold");
        assert_eq!(inline_plain_text("`unclosed"), "`unclosed");
    }

    #[test]
    fn escapes_are_honored() {
        let nodes = parse_inline(r"\*not italic\*");
        assert_eq!(nodes, vec![InlineNode::Text("*not italic*".into())]);
    }

    #[test]
    fn code_span_accepts_multiple_backticks() {
        let nodes = parse_inline("``a ` b``");
        assert_eq!(nodes, vec![InlineNode::Code("a ` b".into())]);
    }

    #[test]
    fn hard_break_is_preserved() {
        assert_eq!(
            parse_inline("line one\nline two"),
            vec![
                InlineNode::Text("line one".into()),
                InlineNode::Break,
                InlineNode::Text("line two".into())
            ]
        );
    }

    #[test]
    fn readable_length_ignores_citation_markers_and_markup() {
        let text = "Revenue grew **18%** [^sec10] with `automation` momentum.";
        // The citation vanishes but the spaces on either side of it are real source text.
        assert_eq!(
            readable_char_count(text),
            "Revenue grew 18%  with automation momentum."
                .chars()
                .count()
        );
        assert!(readable_char_count(text) < inline_plain_text(text).chars().count());
    }

    #[test]
    fn link_titles_are_dropped() {
        let nodes = parse_inline("[x](https://example.com \"title\")");
        assert!(
            matches!(&nodes[0], InlineNode::Link { href, .. } if href == "https://example.com")
        );
    }
}
