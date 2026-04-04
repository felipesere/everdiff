use std::sync::Arc;

use crate::{
    column::{FormattedRow, LineGroup, Lineable},
    wrap::{split_at_width, wrap_plain},
};

// --- Highlight -------------------------------------------------------------------

/// A thread-safe, cloneable styling function.
///
/// Takes a plain-text slice and returns a string that may contain ANSI escape
/// codes. The `Arc` makes it cheap to share a single highlight function across
/// many lines (e.g. the same "dimmed" style applied to every context line in a
/// diff hunk) without cloning the closure body.
///
/// The layout crate never constructs a `Highlight` itself — callers in
/// `everdiff-snippet` provide them.
pub type Highlight = Arc<dyn Fn(&str) -> String + Send + Sync>;

// --- StyledContent trait ---------------------------------------------------------

/// A content type that wraps long text into fixed-width segments and applies
/// ANSI colour styling, ensuring codes never straddle a line boundary.
///
/// This trait operates at the *string* level: it borrows `&self` and returns
/// `Vec<String>`. It knows nothing about [`FormattedRow`], [`LineGroup`], or
/// line-number chrome — that is [`Lineable`]'s concern.
///
/// [`PrefixedLine::Numbered`](crate::PrefixedLine) is the bridge between the two
/// traits: it holds a `Box<dyn StyledContent>`, calls `styled_segments` to obtain
/// styled strings, and then frames each one with `│ nr │` chrome before wrapping
/// it in a [`FormattedRow`].
///
/// Implementors are responsible for their own wrapping: each returned `String`
/// must have a *visible* width of at most `width` columns. Strings may contain
/// ANSI codes; visible width is measured separately from `str::len` because ANSI
/// escape bytes inflate the byte count without advancing the cursor.
pub trait StyledContent: Send + Sync {
    /// Return one styled string per display row, each fitting within `width`
    /// visible terminal columns.
    ///
    /// Always returns at least one element — a zero-width `width` should produce
    /// a single empty string rather than an empty `Vec`.
    fn styled_segments(&self, width: u16) -> Vec<String>;
}

// --- Plain string impls ----------------------------------------------------------

impl StyledContent for String {
    fn styled_segments(&self, width: u16) -> Vec<String> {
        wrap_plain(self, width)
    }
}

impl StyledContent for &'static str {
    fn styled_segments(&self, width: u16) -> Vec<String> {
        wrap_plain(self, width)
    }
}

// --- Highlighted -----------------------------------------------------------------

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

impl StyledContent for Highlighted {
    fn styled_segments(&self, width: u16) -> Vec<String> {
        wrap_plain(&self.text, width)
            .into_iter()
            .map(|seg| (self.highlight)(&seg))
            .inspect(|l| tracing::info!(l.len = l.len()))
            .collect()
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

impl StyledContent for InlineParts {
    fn styled_segments(&self, width: u16) -> Vec<String> {
        if width == 0 {
            return vec![String::new()];
        }

        let width_usize = width as usize;
        let mut segments: Vec<String> = Vec::new();
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

// --- Lineable impls --------------------------------------------------------------

impl Lineable for Highlighted {
    fn into_line_group(self, content_width: u16) -> LineGroup {
        let group = wrap_plain(&self.text, content_width)
            .into_iter()
            .map(|seg| FormattedRow((self.highlight)(&seg)))
            .collect();

        LineGroup(group)
    }
}

/// Pad `original` to `width` *visible* columns, accounting for ANSI overhead.
///
/// `str::len` counts bytes, but ANSI escape sequences inflate byte length without
/// advancing the cursor. When `visible_width < original.len()` the difference is
/// ANSI overhead; the format-string padding target is widened by that amount so
/// the output fills exactly `width` visible columns.
fn pad(original: &str, width: u16) -> String {
    use std::cmp::Ordering;
    let visible_width = unicode_width::UnicodeWidthStr::width(original);
    match visible_width.cmp(&original.len()) {
        Ordering::Less => {
            let extras = original.len() - visible_width;
            format!("{original:<w$}", w = width as usize + extras)
        }
        Ordering::Equal => original.to_string(),
        Ordering::Greater => {
            unreachable!("the visible width can't be greater than teh normal one?")
        }
    }
}

impl Lineable for InlineParts {
    fn into_line_group(self, width: u16) -> LineGroup {
        if width == 0 {
            return LineGroup(vec![]);
        }

        let segments = self
            .styled_segments(width)
            .into_iter()
            .map(|s| FormattedRow(pad(&s, width)))
            .collect();

        LineGroup(segments)
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
        assert_eq!(segs, vec!["hello", " worl", "d    "]);
    }

    #[test]
    fn highlighted_applies_to_each_segment() {
        let h = Highlighted::new("hello world", Arc::new(|s: &str| format!("[x]{s}[/x]")));
        let segs = h.styled_segments(5);
        assert_eq!(segs, vec!["[x]hello[/x]", "[x] worl[/x]", "[x]d    [/x]"]);
    }

    #[test]
    fn inline_parts_no_wrap_needed() {
        let mut parts = InlineParts::new();
        parts.push("key: ", Arc::new(|s: &str| dim(s)));
        parts.push("val", Arc::new(|s: &str| bold(s)));
        let segs = parts.styled_segments(20);
        assert_eq!(segs, vec!["[dim]key: [/][bold]val[/]"]);
    }

    #[test]
    fn inline_parts_wraps_across_part_boundary() {
        // width=10, parts: "key: "(5) + "old  new"(8) + " # note"(7)
        let mut parts = InlineParts::new();
        parts.push("key: ", Arc::new(|s: &str| dim(s)));
        parts.push("old  new", Arc::new(|s: &str| bold(s)));
        parts.push(" # note", Arc::new(|s: &str| dim(s)));
        let segs = parts.styled_segments(10);
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
