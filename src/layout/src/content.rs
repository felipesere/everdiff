use std::sync::Arc;

use crate::{
    column::{FormattedRow, LineGroup, Lineable},
    wrap::{split_at_width, wrap_plain},
};

/// A cloneable styling function.
///
/// Takes a plain-text slice and returns a string that may contain ANSI escape codes.
/// The `Arc` makes it cheap to share a single highlight function across
/// many lines (e.g. the same "dimmed" style applied to every context line in a
/// diff hunk) without cloning the closure body.
///
/// The layout crate never constructs a `Highlight` itself — callers in
/// `everdiff-snippet` provide them.
pub type Highlight = Arc<dyn Fn(&str) -> String + Send + Sync>;

/// Content styled uniformly with a single [`Highlight`] function.
///
/// The plain text is wrapped first; each segment is then passed through
/// `highlight`. Because highlighting is applied per segment, ANSI codes are
/// always self-contained within one [`FormattedRow`] — no reset/reopen across
/// line breaks is needed.
///
/// Use this for lines where the entire content shares one style (e.g. a line
/// that was added, removed, or left unchanged as context).
/// For lines where different *spans* carry different styles, use [`InlineParts`].
pub struct Highlighted {
    /// The plain text to display (no ANSI codes).
    pub text: String,
    /// The styling function applied to each wrapped segment.
    pub highlight: Highlight,
}

impl Highlighted {
    /// Create a [`Highlighted`] from any string-like value and a [`Highlight`] function.
    pub fn new(text: impl Into<String>, highlight: Highlight) -> Self {
        Highlighted {
            text: text.into(),
            highlight,
        }
    }
}

impl Lineable for Highlighted {
    fn as_line_group(&self, content_width: u16) -> LineGroup {
        let group = wrap_plain(&self.text, content_width)
            .into_iter()
            .map(|seg| FormattedRow((self.highlight)(&seg)))
            .collect();

        LineGroup(group)
    }
}

// --- InlineParts -----------------------------------------------------------------

/// Content assembled from spans, each with its own [`Highlight`] function.
///
/// Use this for word-wise diffs where different spans of the same line carry
/// different styles — for example, a key rendered in a dimmed style followed by
/// the changed value in a highlighted style.
///
/// Build incrementally with [`push`](InlineParts::push). Wrapping walks the parts
/// in order and fills fixed-width segment buckets. When a part straddles a segment
/// boundary it is split; the remainder carries forward with the same [`Highlight`].
/// ANSI codes are therefore always self-contained per segment.
pub struct InlineParts {
    pub(crate) parts: Vec<(String, Highlight)>,
}

impl InlineParts {
    /// Create an empty [`InlineParts`].
    pub fn new() -> Self {
        InlineParts { parts: Vec::new() }
    }

    /// Append a text span with its associated [`Highlight`] function.
    pub fn push(&mut self, text: impl Into<String>, highlight: Highlight) -> &Self {
        self.parts.push((text.into(), highlight));
        self
    }
}

impl Default for InlineParts {
    fn default() -> Self {
        Self::new()
    }
}

impl Lineable for InlineParts {
    fn as_line_group(&self, width: u16) -> LineGroup {
        if width == 0 {
            return LineGroup(vec![]);
        }

        let width_usize = width as usize;
        let mut segments: Vec<FormattedRow> = Vec::new();
        let mut current = String::new();
        let mut current_width = 0usize;

        for (text, highlight) in &self.parts {
            let mut remaining = text.as_str();
            while !remaining.is_empty() {
                let remaining_available_space = width_usize.saturating_sub(current_width);
                let (fits, rest) = split_at_width(remaining, remaining_available_space as u16);

                if !fits.is_empty() {
                    current.push_str(&highlight(fits));
                    current_width += unicode_width::UnicodeWidthStr::width(fits);
                }

                remaining = rest;

                // Close the segment when full and there is still more text to place.
                if current_width >= width_usize && !remaining.is_empty() {
                    segments.push(FormattedRow(pad(&current, width)));
                    current = String::new();
                    current_width = 0;
                }
            }
        }

        // Emit whatever remains in the buffer (always at least one segment).
        if !current.is_empty() || segments.is_empty() {
            segments.push(FormattedRow(pad(&current, width)));
        }

        LineGroup(segments)
    }
}

// --- Helpers ---------------------------------------------------------------------

/// Pad `original` to `width` *visible* columns, accounting for ANSI overhead.
///
/// `str::len` counts bytes, but ANSI escape sequences inflate byte length without
/// advancing the cursor. `ansi_width` measures only the visible columns; the
/// difference is used to widen the format-string target so the output fills
/// exactly `width` visible columns.
fn pad(original: &str, width: u16) -> String {
    let visible_width = ansi_width::ansi_width(original);
    let extras = original.len().saturating_sub(visible_width);
    format!("{original:<w$}", w = width as usize + extras)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(group: LineGroup) -> Vec<String> {
        group.0.into_iter().map(|r| r.0).collect()
    }

    fn dim(s: &str) -> String {
        format!("[dim]{s}[/]")
    }
    fn bold(s: &str) -> String {
        format!("[bold]{s}[/]")
    }

    #[test]
    fn highlighted_applies_to_each_segment() {
        let h = Highlighted::new("hello world", Arc::new(|s: &str| format!("[x]{s}[/x]")));
        let segs = rows(h.as_line_group(5));
        assert_eq!(segs, vec!["[x]hello[/x]", "[x] worl[/x]", "[x]d    [/x]"]);
    }

    #[test]
    fn inline_parts_no_wrap_needed() {
        let mut parts = InlineParts::new();
        parts.push("key: ", Arc::new(|s: &str| dim(s)));
        parts.push("val", Arc::new(|s: &str| bold(s)));
        let segs = rows(parts.as_line_group(20));
        // Fake ANSI tags aren't transparent to ansi_width, so padding accounts
        // for byte length; with real ANSI codes the trailing spaces would appear.
        assert_eq!(segs, vec!["[dim]key: [/][bold]val[/]"]);
    }

    #[test]
    fn inline_parts_wraps_across_part_boundary() {
        // width=10, parts: "key: "(5) + "old  new"(8) + " # note"(7)
        let mut parts = InlineParts::new();
        parts.push("key: ", Arc::new(|s: &str| dim(s)));
        parts.push("old  new", Arc::new(|s: &str| bold(s)));
        parts.push(" # note", Arc::new(|s: &str| dim(s)));
        let segs = rows(parts.as_line_group(10));
        assert_eq!(
            segs,
            vec!["[dim]key: [/][bold]old  [/]", "[bold]new[/][dim] # note[/]",]
        );
    }

    #[test]
    fn inline_parts_part_split_mid_word() {
        // width=4, one part "hello" → split into "hell" + "o"
        let mut parts = InlineParts::new();
        parts.push("hello", Arc::new(|s: &str| bold(s)));
        let segs = rows(parts.as_line_group(4));
        assert_eq!(segs, vec!["[bold]hell[/]", "[bold]o[/]"]);
    }

    #[test]
    fn inline_parts_empty_produces_one_blank_segment() {
        let parts = InlineParts::new();
        let segs = rows(parts.as_line_group(10));
        assert_eq!(segs, vec!["          "]);
    }
}
