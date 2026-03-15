use std::sync::Arc;

use crate::wrap::{split_at_width, wrap_plain};

/// A styling function: takes a plain-text slice, returns a styled string (may contain ANSI codes).
/// Provided by the caller; the layout crate never constructs one itself.
pub type Highlight = Arc<dyn Fn(&str) -> String + Send + Sync>;

/// Produces styled display segments for a given column width.
///
/// Each implementor owns its own wrapping logic. Segments are ready to be placed
/// into a [`FormattedRow`](crate::column::FormattedRow) without further transformation.
///
/// [`crate::column::Column::push`] calls `styled_segments(content_width)` and
/// prefixes each result with a line widget and separator.
pub trait StyledContent: Send + Sync {
    /// Return one styled string per display row, each fitting within `width` visible columns.
    /// Strings may contain ANSI codes; visible width ≤ `width` for all returned strings.
    fn styled_segments(&self, width: usize) -> Vec<String>;
}

// --- Plain string ----------------------------------------------------------------

impl StyledContent for String {
    fn styled_segments(&self, width: usize) -> Vec<String> {
        wrap_plain(self, width)
    }
}

impl StyledContent for &'static str {
    fn styled_segments(&self, width: usize) -> Vec<String> {
        wrap_plain(self, width)
    }
}

// --- Uniform highlight -----------------------------------------------------------

/// A line whose entire content is styled with a single [`Highlight`] function.
///
/// The plain text is wrapped first; each segment is then passed to `highlight`.
/// Because highlighting is applied per segment, ANSI codes are always
/// self-contained within a segment — no reset/reopen across line breaks needed.
pub struct Highlighted {
    pub text: String,
    pub highlight: Highlight,
}

impl Highlighted {
    pub fn new(text: impl Into<String>, highlight: Highlight) -> Self {
        Highlighted {
            text: text.into(),
            highlight,
        }
    }
}

impl StyledContent for Highlighted {
    fn styled_segments(&self, width: usize) -> Vec<String> {
        wrap_plain(&self.text, width)
            .into_iter()
            .map(|seg| (self.highlight)(&seg))
            .collect()
    }
}

// --- Inline (per-part) highlights ------------------------------------------------

/// A line assembled from parts, each with its own [`Highlight`] function.
///
/// Use this for word-wise diffs where different spans of the same line carry
/// different styles. Built incrementally via [`InlineParts::push`].
///
/// Wrapping is done by walking parts in order and filling segment buckets up to
/// `width` visible columns. When a part straddles a segment boundary it is split;
/// the remainder carries forward with the same `Highlight`. This means ANSI codes
/// are always self-contained per segment, requiring no ANSI scanning.
///
/// # Example
///
/// ```
/// # use std::sync::Arc;
/// # use everdiff_layout::content::{InlineParts, StyledContent};
/// let parts = InlineParts::new()
///     .push("key: ",     Arc::new(|s: &str| format!("[dim]{s}[/]")))
///     .push("new_value", Arc::new(|s: &str| format!("[bold]{s}[/]")))
///     .push(" # note",   Arc::new(|s: &str| format!("[dim]{s}[/]")));
///
/// let segs = parts.styled_segments(10);
/// assert_eq!(segs, vec![
///     "[dim]key: [/][bold]new_v[/]",
///     "[bold]alue[/][dim] # not[/]",
///     "[dim]e[/]",
/// ]);
/// ```
pub struct InlineParts {
    parts: Vec<(String, Highlight)>,
}

impl InlineParts {
    pub fn new() -> Self {
        InlineParts { parts: Vec::new() }
    }

    /// Append a text span with its associated highlight function.
    pub fn push(mut self, text: impl Into<String>, highlight: Highlight) -> Self {
        self.parts.push((text.into(), highlight));
        self
    }
}

impl Default for InlineParts {
    fn default() -> Self {
        Self::new()
    }
}

impl StyledContent for InlineParts {
    fn styled_segments(&self, width: usize) -> Vec<String> {
        if width == 0 {
            return vec![String::new()];
        }

        let mut segments: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut current_width = 0usize;

        for (text, highlight) in &self.parts {
            let mut remaining = text.as_str();
            while !remaining.is_empty() {
                let remaining_available_space = width.saturating_sub(current_width);
                let (fits, rest) = split_at_width(remaining, remaining_available_space);

                if !fits.is_empty() {
                    current.push_str(&highlight(fits));
                    current_width += unicode_width::UnicodeWidthStr::width(fits);
                }

                remaining = rest;

                // Close the segment when full and there is still more text to place.
                if current_width >= width && !remaining.is_empty() {
                    segments.push(std::mem::take(&mut current));
                    current_width = 0;
                }
            }
        }

        // Emit whatever remains in the buffer (always at least one segment).
        if !current.is_empty() || segments.is_empty() {
            segments.push(current);
        }

        segments
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dim(s: &str) -> String {
        format!("[dim]{s}[/]")
    }
    fn bold(s: &str) -> String {
        format!("[bold]{s}[/]")
    }

    #[test]
    fn string_plain_wrap() {
        let segs = "hello world".to_string().styled_segments(5);
        assert_eq!(segs, vec!["hello", " worl", "d"]);
    }

    #[test]
    fn highlighted_applies_to_each_segment() {
        let h = Highlighted::new("hello world", Arc::new(|s: &str| format!("[x]{s}[/x]")));
        let segs = h.styled_segments(5);
        assert_eq!(segs, vec!["[x]hello[/x]", "[x] worl[/x]", "[x]d[/x]"]);
    }

    #[test]
    fn inline_parts_no_wrap_needed() {
        let parts = InlineParts::new()
            .push("key: ", Arc::new(|s: &str| dim(s)))
            .push("val", Arc::new(|s: &str| bold(s)));
        let segs = parts.styled_segments(20);
        assert_eq!(segs, vec!["[dim]key: [/][bold]val[/]"]);
    }

    #[test]
    fn inline_parts_wraps_across_part_boundary() {
        // width=10, parts: "key: "(5) + "old  new"(8) + " # note"(7)
        let parts = InlineParts::new()
            .push("key: ", Arc::new(|s: &str| dim(s)))
            .push("old  new", Arc::new(|s: &str| bold(s)))
            .push(" # note", Arc::new(|s: &str| dim(s)));
        let segs = parts.styled_segments(10);
        assert_eq!(
            segs,
            vec!["[dim]key: [/][bold]old  [/]", "[bold]new[/][dim] # note[/]",]
        );
    }

    #[test]
    fn inline_parts_part_split_mid_word() {
        // width=4, one part "hello" → split into "hell" + "o"
        let parts = InlineParts::new().push("hello", Arc::new(|s: &str| bold(s)));
        let segs = parts.styled_segments(4);
        assert_eq!(segs, vec!["[bold]hell[/]", "[bold]o[/]"]);
    }

    #[test]
    fn inline_parts_empty_produces_one_blank_segment() {
        let parts = InlineParts::new();
        let segs = parts.styled_segments(10);
        assert_eq!(segs, vec![""]);
    }
}
