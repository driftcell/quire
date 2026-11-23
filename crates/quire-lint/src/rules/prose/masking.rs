use super::*;

pub(crate) fn has_list_structure(text: &str) -> bool {
    if !text.contains('\n') {
        return false;
    }
    text.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("- ")
            || trimmed.starts_with("* ")
            || trimmed.starts_with("+ ")
            || numbered_list_line(trimmed)
    })
}

fn numbered_list_line(line: &str) -> bool {
    let mut digit_end = 0usize;
    for (index, ch) in line.char_indices() {
        if ch.is_ascii_digit() {
            digit_end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    if digit_end == 0 {
        return false;
    }
    line.get(digit_end..)
        .is_some_and(|rest| rest.starts_with(". "))
}

pub(crate) fn stripped_wall_sentence_count(text: &str) -> usize {
    text.split(['。', '！', '？', '!', '?', '\n'])
        .filter(|sentence| !sentence.trim().is_empty())
        .count()
}

pub(crate) fn strip_wall_markup(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0usize;
    while cursor < text.len() {
        let Some(rest) = text.get(cursor..) else {
            break;
        };
        let Some(ch) = rest.chars().next() else {
            break;
        };
        if ch == '`' {
            cursor += ch.len_utf8();
            continue;
        }
        if rest.starts_with("[^")
            && let Some(end) = bracket_close_end(text, cursor)
        {
            cursor = end;
            continue;
        }
        if ch == '['
            && let Some((label_end, target_end)) = markdown_link_end(text, cursor)
        {
            if let Some(label) = text.get(cursor + 1..label_end) {
                output.push_str(label);
            }
            cursor = target_end;
            continue;
        }
        output.push(ch);
        cursor += ch.len_utf8();
    }
    output
}

fn markdown_link_end(text: &str, start: usize) -> Option<(usize, usize)> {
    let label_end = matching_bracket_end(text, start)?;
    let after = text.get(label_end + 1..)?;
    if !after.starts_with('(') {
        return None;
    }
    let target_end = parenthesis_end(text, label_end + 1)?;
    Some((label_end, target_end))
}

fn matching_bracket_end(text: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut index = start;
    while let Some(tail) = text.get(index..) {
        let ch = tail.chars().next()?;
        if ch == '\\' {
            index += ch.len_utf8();
            if let Some(next) = text.get(index..).and_then(|rest| rest.chars().next()) {
                index += next.len_utf8();
            }
            continue;
        }
        match ch {
            '[' => depth += 1,
            ']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
        index += ch.len_utf8();
    }
    None
}

pub(crate) fn mask_text(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0usize;
    while cursor < text.len() {
        let Some(rest) = text.get(cursor..) else {
            break;
        };
        let Some(ch) = rest.chars().next() else {
            break;
        };
        let span_end = match ch {
            '`' => code_span_end(text, cursor),
            '[' if rest.starts_with("[^") => bracket_close_end(text, cursor),
            '(' if text
                .get(..cursor)
                .is_some_and(|prefix| prefix.ends_with(']')) =>
            {
                parenthesis_end(text, cursor)
            }
            '《' => delimited_end(text, cursor, '》'),
            '“' => delimited_end(text, cursor, '”'),
            '"' => delimited_end(text, cursor, '"'),
            '「' => delimited_end(text, cursor, '」'),
            _ => None,
        };
        if let Some(end) = span_end {
            if let Some(span) = text.get(cursor..end) {
                push_spaces(&mut output, span);
            }
            cursor = end;
        } else {
            output.push(ch);
            cursor += ch.len_utf8();
        }
    }
    output
}

fn push_spaces(output: &mut String, text: &str) {
    for _ in text.chars() {
        output.push(' ');
    }
}

fn code_span_end(text: &str, start: usize) -> Option<usize> {
    let tail = text.get(start..)?;
    let run = tail.bytes().take_while(|byte| *byte == b'`').count();
    let content_start = start + run;
    let mut search = content_start;
    while let Some(rest) = text.get(search..) {
        let found = rest.find('`')?;
        let absolute = search + found;
        let run_tail = text.get(absolute..)?;
        let closing_run = run_tail.bytes().take_while(|byte| *byte == b'`').count();
        if closing_run == run {
            return Some(absolute + closing_run);
        }
        search = absolute + closing_run;
    }
    None
}

fn bracket_close_end(text: &str, start: usize) -> Option<usize> {
    let tail = text.get(start + 2..)?;
    let found = tail.find(']')?;
    Some(start + 2 + found + 1)
}

fn parenthesis_end(text: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut index = start;
    while let Some(tail) = text.get(index..) {
        let ch = tail.chars().next()?;
        if ch == '\\' {
            index += ch.len_utf8();
            if let Some(next) = text.get(index..).and_then(|rest| rest.chars().next()) {
                index += next.len_utf8();
            }
            continue;
        }
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index + ch.len_utf8());
                }
            }
            _ => {}
        }
        index += ch.len_utf8();
    }
    None
}

fn delimited_end(text: &str, start: usize, close: char) -> Option<usize> {
    let open = text.get(start..)?.chars().next()?;
    let content_start = start + open.len_utf8();
    let tail = text.get(content_start..)?;
    let found = tail.find(close)?;
    Some(content_start + found + close.len_utf8())
}

pub(super) fn contains_ascii_phrase(text: &str, phrase: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    let mut search = 0usize;
    while let Some(rest) = lowered.get(search..) {
        let Some(found) = rest.find(phrase) else {
            return false;
        };
        let absolute = search + found;
        let before_ok = lowered
            .get(..absolute)
            .and_then(|prefix| prefix.chars().next_back())
            .is_none_or(|ch| !is_ascii_word_char(ch));
        let after = absolute + phrase.len();
        let after_ok = lowered
            .get(after..)
            .and_then(|suffix| suffix.chars().next())
            .is_none_or(|ch| !is_ascii_word_char(ch));
        if before_ok && after_ok {
            return true;
        }
        search = absolute + phrase.len().max(1);
    }
    false
}

fn is_ascii_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

pub(super) fn contains_unguarded_eco(text: &str) -> bool {
    let mut search = 0usize;
    while let Some(rest) = text.get(search..) {
        let Some(found) = rest.find("生态") else {
            return false;
        };
        let absolute = search + found;
        if !is_eco_compound(text, absolute) {
            return true;
        }
        search = absolute + "生态".len();
    }
    false
}

fn is_eco_compound(text: &str, index: usize) -> bool {
    let before = text.get(..index).unwrap_or("");
    let after = text.get(index + "生态".len()..).unwrap_or("");
    ECO_PREFIXES.iter().any(|prefix| before.ends_with(prefix))
        || ECO_SUFFIXES.iter().any(|suffix| after.starts_with(suffix))
}

pub(crate) fn has_inline_citation(text: &str) -> bool {
    let mut search = 0usize;
    while let Some(rest) = text.get(search..) {
        let Some(found) = rest.find("[^") else {
            return false;
        };
        let absolute = search + found + 2;
        if text.get(absolute..).is_some_and(|tail| tail.contains(']')) {
            return true;
        }
        search = absolute;
    }
    false
}

pub(crate) fn has_book_title(text: &str) -> bool {
    let mut search = 0usize;
    while let Some(rest) = text.get(search..) {
        let Some(found) = rest.find('《') else {
            return false;
        };
        let absolute = search + found + '《'.len_utf8();
        if text.get(absolute..).is_some_and(|tail| tail.contains('》')) {
            return true;
        }
        search = absolute;
    }
    false
}

pub(crate) fn masked_plain_char_count(text: &str) -> usize {
    text.chars()
        .filter(|ch| !ch.is_whitespace() && !is_markdown_symbol(*ch))
        .count()
}

fn is_markdown_symbol(ch: char) -> bool {
    matches!(
        ch,
        '*' | '_'
            | '~'
            | '`'
            | '#'
            | '>'
            | '-'
            | '+'
            | '='
            | '['
            | ']'
            | '('
            | ')'
            | '!'
            | '|'
            | '\\'
    )
}

pub(super) fn contains_punctuated_sequence(text: &str, items: &[&str]) -> bool {
    let mut search = 0usize;
    for item in items {
        let mut matched = false;
        while let Some(rest) = text.get(search..) {
            let Some(found) = rest.find(item) else {
                return false;
            };
            let absolute = search + found;
            let after = absolute + item.len();
            let Some(next) = text.get(after..).and_then(|tail| tail.chars().next()) else {
                return false;
            };
            if matches!(next, '，' | '、' | '：' | ',' | ':') {
                search = after + next.len_utf8();
                matched = true;
                break;
            }
            search = after;
        }
        if !matched {
            return false;
        }
    }
    true
}

pub(super) fn contains_in_order(text: &str, items: &[&str]) -> bool {
    let mut search = 0usize;
    for item in items {
        let Some(rest) = text.get(search..) else {
            return false;
        };
        let Some(found) = rest.find(item) else {
            return false;
        };
        search += found + item.len();
    }
    true
}

pub(super) fn bold_list_count(text: &str) -> usize {
    text.lines()
        .filter(|line| list_bold_prefix_end(line).is_some())
        .count()
}

fn list_bold_prefix_end(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    for prefix in ["- **", "* **"] {
        if trimmed.starts_with(prefix) {
            return Some(prefix.len());
        }
    }
    let mut digit_end = 0usize;
    for (index, ch) in trimmed.char_indices() {
        if ch.is_ascii_digit() {
            digit_end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    if digit_end == 0 {
        return None;
    }
    trimmed
        .get(digit_end..)
        .and_then(|rest| rest.starts_with(". **").then_some(digit_end + ". **".len()))
}

pub(super) fn has_digit_outside_list_markers(text: &str) -> bool {
    for line in text.lines() {
        let trimmed = line.trim_start();
        let rest = match list_bold_prefix_end(line) {
            Some(prefix_end) => trimmed.get(prefix_end..),
            None => Some(trimmed),
        };
        if rest.is_some_and(|value| value.chars().any(|ch| ch.is_ascii_digit())) {
            return true;
        }
    }
    false
}
