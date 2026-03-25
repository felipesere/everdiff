/// Split plain text into segments that each fit within `max_width` visible columns.
/// Unicode-aware: uses `unicode-width` for character measurement.
pub(crate) fn wrap_plain(text: &str, max_width: u16) -> Vec<String> {
    let max_width = max_width as usize;
    debug_assert!(max_width > 0, "wrapping to zero width makes no sense.");
    if text.is_empty() {
        return vec![format!("{:<max_width$}", "")];
    }

    let mut segments = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for ch in text.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_width + ch_width > max_width && !current.is_empty() {
            segments.push(format!("{current:<width$}", width = max_width));
            current = String::new();
            current_width = 0;
        }
        current.push(ch);
        current_width += ch_width;
    }

    if !current.is_empty() || segments.is_empty() {
        segments.push(format!("{current:<max_width$}"));
    }

    segments
}

/// Split `text` at the boundary where `max_width` visible columns are consumed.
/// Returns `(fitting_part, remainder)`. Both are slices into the original.
pub fn split_at_width(text: &str, max_width: u16) -> (&str, &str) {
    let max_width = max_width as usize;
    let mut width = 0usize;
    let mut byte_pos = 0usize;
    for ch in text.chars() {
        let ch_w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + ch_w > max_width {
            break;
        }
        width += ch_w;
        byte_pos += ch.len_utf8();
    }
    (&text[..byte_pos], &text[byte_pos..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_short_line_no_wrap() {
        assert_eq!(wrap_plain("hello", 10), vec!["hello     "]);
    }

    #[test]
    fn plain_exact_width() {
        assert_eq!(wrap_plain("12345", 5), vec!["12345"]);
    }

    #[test]
    fn plain_exceeds_width() {
        assert_eq!(wrap_plain("1234567890", 5), vec!["12345", "67890"]);
    }

    #[test]
    fn plain_empty_string() {
        assert_eq!(wrap_plain("", 10), vec!["          "]);
    }

    #[test]
    fn plain_unicode_wide_chars() {
        // Each CJK char is 2 columns wide; 3 fit in width 6
        assert_eq!(wrap_plain("漢字テスト", 6), vec!["漢字テ   ", "スト    "]);
    }

    #[test]
    fn split_ascii() {
        assert_eq!(split_at_width("hello world", 5), ("hello", " world"));
    }

    #[test]
    fn split_exact() {
        assert_eq!(split_at_width("hello", 5), ("hello", ""));
    }

    #[test]
    fn split_wide_chars() {
        // "漢字" is 4 cols, can't fit in 3 → only "漢" (2 cols) fits
        assert_eq!(split_at_width("漢字", 3), ("漢", "字"));
    }

    #[test]
    fn split_zero_width() {
        assert_eq!(split_at_width("hello", 0), ("", "hello"));
    }
}
