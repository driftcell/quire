pub(super) fn ascii_digit_prefix_len(text: &str) -> usize {
    let mut end = 0usize;
    for (index, ch) in text.char_indices() {
        if ch.is_ascii_digit() {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    end
}

pub(super) fn chapter_numeral_prefix_len(text: &str) -> usize {
    let mut end = 0usize;
    for (index, ch) in text.char_indices() {
        if ch.is_ascii_digit() || is_chinese_numeral(ch) {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    end
}

pub(super) fn is_chinese_numeral(ch: char) -> bool {
    matches!(
        ch,
        '零' | '一' | '二' | '两' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十' | '百'
    )
}
