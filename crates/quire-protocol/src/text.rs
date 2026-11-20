//! Small allocation-light text helpers shared by the report engine.

/// Truncate to at most `max_chars` Unicode scalar values, using an ellipsis when truncated.
pub fn truncate_chars(input: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = input.chars().count();
    if count <= max_chars {
        return input.to_string();
    }
    if max_chars == 1 {
        return "…".to_string();
    }
    let mut result: String = input.chars().take(max_chars - 1).collect();
    result.push('…');
    result
}

/// Sanitize a URL for use in an `href`. Only benign schemes and relative URLs are allowed.
pub fn safe_href(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    let first_colon = lower.find(':');
    let is_relative = trimmed.starts_with('#')
        || trimmed.starts_with('/')
        || trimmed.starts_with("./")
        || trimmed.starts_with("../");
    if let Some(colon) = first_colon {
        // A colon after a slash/query/hash is not a scheme separator.
        let slash = lower.find('/').unwrap_or(usize::MAX);
        let query = lower.find('?').unwrap_or(usize::MAX);
        let hash = lower.find('#').unwrap_or(usize::MAX);
        if colon < slash.min(query).min(hash) {
            let scheme = &lower[..colon];
            if !matches!(scheme, "http" | "https" | "mailto" | "tel") {
                return None;
            }
        }
    } else if !is_relative {
        // Plain relative path like `notes.html`: allowed, but reject encoded javascript.
        if lower.starts_with("javascript") || lower.starts_with("data:") {
            return None;
        }
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return None;
    }
    Some(trimmed.to_string())
}

/// Visual width heuristic used for table layout: CJK and full-width glyphs count 2.0, others 1.0.
pub fn visual_width(input: &str) -> f64 {
    input
        .chars()
        .map(|ch| if is_wide_char(ch) { 2.0 } else { 1.0 })
        .sum()
}

/// Widest line of a multi-line string, using [`visual_width`].
pub fn visual_width_multiline(input: &str) -> f64 {
    input
        .split(['\n', '\r'])
        .map(visual_width)
        .fold(0.0, f64::max)
}

/// Rough CJK / emoji detection for monospace-ish layout heuristics.
pub fn is_wide_char(ch: char) -> bool {
    let code = ch as u32;
    matches!(code,
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE10..=0xFE19
        | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x3FFFD
    )
}

/// Normalize whitespace for digests and accessible labels.
pub fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Create a deterministic ASCII slug. Non-ASCII letters are retained only if alphanumeric.
pub fn slugify(input: &str) -> String {
    let mut output = String::new();
    let mut pending_dash = false;
    for ch in input.chars() {
        if ch.is_alphanumeric() {
            if pending_dash && !output.is_empty() {
                output.push('-');
            }
            pending_dash = false;
            for lower in ch.to_lowercase() {
                output.push(lower);
            }
        } else {
            pending_dash = true;
        }
        if output.len() >= 80 {
            break;
        }
    }
    let trimmed = output.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "section".to_string()
    } else {
        trimmed
    }
}

/// A located CSS syntax issue for actionable linter hints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssBraceIssue {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

/// Locate the first unbalanced brace, unterminated string or unterminated comment.
pub fn css_brace_issue(css: &str) -> Option<CssBraceIssue> {
    let bytes = css.as_bytes();
    let mut stack: Vec<usize> = Vec::new();
    let mut index = 0;
    let mut quote_start: Option<(u8, usize)> = None;
    while index < bytes.len() {
        let Some(byte) = bytes.get(index).copied() else {
            break;
        };
        if let Some((active_quote, start)) = quote_start {
            if byte == b'\\' {
                index += 2;
                continue;
            }
            if byte == active_quote {
                quote_start = None;
            }
            if byte == b'\n' {
                return Some(CssBraceIssue {
                    line: line_column(css, start).0,
                    column: line_column(css, start).1,
                    message: "unterminated CSS string literal".to_string(),
                });
            }
            index += 1;
            continue;
        }
        match byte {
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                let start = index;
                index += 2;
                let mut found = false;
                while index + 1 < bytes.len() {
                    if bytes.get(index) == Some(&b'*') && bytes.get(index + 1) == Some(&b'/') {
                        index += 2;
                        found = true;
                        break;
                    }
                    index += 1;
                }
                if !found {
                    let (line, column) = line_column(css, start);
                    return Some(CssBraceIssue {
                        line,
                        column,
                        message: "unterminated CSS comment".to_string(),
                    });
                }
            }
            b'"' | b'\'' => {
                quote_start = Some((byte, index));
                index += 1;
            }
            b'\\' => index += 2,
            b'{' => {
                stack.push(index);
                index += 1;
            }
            b'}' => {
                if stack.pop().is_none() {
                    let (line, column) = line_column(css, index);
                    return Some(CssBraceIssue {
                        line,
                        column,
                        message: "unexpected closing brace `}`".to_string(),
                    });
                }
                index += 1;
            }
            _ => index += 1,
        }
    }
    if let Some((_, start)) = quote_start {
        let (line, column) = line_column(css, start);
        return Some(CssBraceIssue {
            line,
            column,
            message: "unterminated CSS string literal".to_string(),
        });
    }
    if let Some(start) = stack.pop() {
        let (line, column) = line_column(css, start);
        return Some(CssBraceIssue {
            line,
            column,
            message: "unclosed opening brace `{`".to_string(),
        });
    }
    None
}

fn line_column(input: &str, byte_index: usize) -> (usize, usize) {
    let byte_index = byte_index.min(input.len());
    let prefix = input.get(..byte_index).unwrap_or_default();
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map(|(_, tail)| tail.chars().count() + 1)
        .unwrap_or_else(|| prefix.chars().count() + 1);
    (line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_counts_chars_not_bytes() {
        assert_eq!(truncate_chars("日本語のレポート", 4), "日本語…");
        assert_eq!(truncate_chars("abc", 3), "abc");
        assert_eq!(truncate_chars("abcd", 0), "");
    }

    #[test]
    fn unsafe_urls_are_rejected() {
        assert_eq!(
            safe_href("https://example.com/a").as_deref(),
            Some("https://example.com/a")
        );
        assert_eq!(safe_href("/docs/one").as_deref(), Some("/docs/one"));
        assert!(safe_href("javascript:alert(1)").is_none());
        assert!(safe_href("data:text/html,<script>").is_none());
        assert!(safe_href(" java\nscript:alert(1)").is_none());
    }

    #[test]
    fn cjk_width_doubles() {
        assert_eq!(visual_width("AB"), 2.0);
        assert_eq!(visual_width("日本"), 4.0);
        assert_eq!(visual_width("A日"), 3.0);
    }

    #[test]
    fn css_brace_issue_reports_location() {
        let issue = css_brace_issue("a {\n  color: red;\n").unwrap();
        assert_eq!(issue.line, 1);
        assert_eq!(issue.column, 3);
        let issue = css_brace_issue("a { color: red; }\n}\n").unwrap();
        assert_eq!(issue.line, 2);
        assert_eq!(issue.column, 1);
        assert!(css_brace_issue("a { content: \"}\"; }").is_none());
    }
}
