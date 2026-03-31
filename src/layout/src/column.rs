use std::{
    cmp::Ordering,
    fmt::{self},
};

use crate::{
    Highlighted, InlineParts,
    content::StyledContent,
    wrap::{split_at_width, wrap_plain},
};

// --- Line widget -----------------------------------------------------------------

/// The 4-character prefix shown before the `│` separator in each display row.
///
/// ```text
/// "  3 │ content"   ← Nr(2)       (0-based index, displayed as idx+1)
/// "  ┆ │ content"   ← Continuation
/// "    │ content"   ← Filler       (plain text, blank rows)
/// ```
pub(crate) enum LineWidget {
    /// A real line number (0-based index; displayed as `idx + 1`).
    Nr(usize),
    /// A wrapped continuation of the previous line.
    Continuation,
    /// No line number (plain text or blank row).
    Filler,
}

impl fmt::Display for LineWidget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nr(idx) => write!(f, "{:>4} ", idx + 1),
            Self::Continuation => write!(f, "   ┆ "),
            Self::Filler => write!(f, "     "),
        }
    }
}

// --- FormattedRow / LineGroup ----------------------------------------------------

/// A single fully-rendered display row: `"WIDGET│ content<padding>"`.
pub struct FormattedRow(pub String);

impl FormattedRow {
    fn blank(content_width: u16) -> Self {
        let w = content_width as usize;
        FormattedRow(format!("{blank:<w$}", blank = "",))
    }
}

/// All display rows produced from one logical [`Line`] (≥1 when the line wraps).
pub struct LineGroup(pub Vec<FormattedRow>);

/// A single logical line of content carrying a line number.
///
/// May wrap into multiple display rows depending on the column width.
/// The `content` produces styled segments; the line widget is applied by
/// [`Column::push`] after wrapping.
pub struct WithLineNumber {
    /// 0-based line index. Displayed as `nr + 1`
    pub nr: usize,
    pub content: Box<dyn StyledContent>,
}

impl WithLineNumber {
    pub fn new(nr: usize, content: impl StyledContent + 'static) -> Self {
        WithLineNumber {
            nr,
            content: Box::new(content),
        }
    }
}

pub struct WithLineNumberFiller;

impl Lineable for WithLineNumberFiller {
    fn do_thing(self, content_width: u16) -> LineGroup {
        let content_width = content_width - 2 - 5 - 2;
        let w = content_width as usize;

        LineGroup(vec![FormattedRow(format!(
            "│{widget}│ {blank:<w$} ",
            widget = LineWidget::Filler,
            blank = "",
        ))])
    }
}

// --- Column ----------------------------------------------------------------------

/// One side of a two-column layout. Knows its own `content_width`.
///
/// Build by calling [`push`](Column::push) and [`blank`](Column::blank).
/// Zip two columns together with [`ColumnPair::zip`].
pub struct Column {
    pub content_width: u16,
    pub(crate) groups: Vec<LineGroup>,
}

impl Column {
    pub fn new(content_width: u16) -> Self {
        Column {
            content_width,
            groups: Vec::new(),
        }
    }

    pub fn push(&mut self, line: impl Lineable) {
        let group = line.do_thing(self.content_width);
        self.groups.push(group);
    }

    pub fn prepend(&mut self, line: impl Lineable) {
        let group = line.do_thing(self.content_width);
        self.groups.insert(0, group);
    }

    /// Append `count` blank rows (no content, no line number).
    pub fn append_blank(&mut self, count: usize) {
        for _ in 0..count {
            self.groups
                .push(LineGroup(vec![FormattedRow::blank(self.content_width)]));
        }
    }

    // TODO: Is this the most efficient way to do this?
    pub fn prepend_blank(&mut self, count: usize) {
        let mut new_line_group = Vec::with_capacity(self.groups.len() + count);
        for _ in 0..count {
            new_line_group.push(LineGroup(vec![FormattedRow::blank(self.content_width)]));
        }
        new_line_group.append(&mut self.groups);
        self.groups = new_line_group;
    }

    /// Total number of display rows across all groups.
    pub fn row_count(&self) -> usize {
        self.groups.iter().map(|g| g.0.len()).sum()
    }
}

pub trait Lineable {
    fn do_thing(self, content_width: u16) -> LineGroup;
}

impl Lineable for String {
    fn do_thing(self, content_width: u16) -> LineGroup {
        let group = wrap_plain(&self, content_width)
            .into_iter()
            .map(FormattedRow)
            .collect();

        LineGroup(group)
    }
}

impl Lineable for &str {
    fn do_thing(self, content_width: u16) -> LineGroup {
        let group = wrap_plain(self, content_width)
            .into_iter()
            .map(FormattedRow)
            .collect();

        LineGroup(group)
    }
}

impl Lineable for Highlighted {
    fn do_thing(self, content_width: u16) -> LineGroup {
        let group = wrap_plain(&self.text, content_width)
            .into_iter()
            .map(|seg| FormattedRow((self.highlight)(&seg)))
            .collect();

        LineGroup(group)
    }
}

impl Lineable for WithLineNumber {
    fn do_thing(self, content_width: u16) -> LineGroup {
        let line = self;
        let nr = line.nr;
        let widget_length = 5;
        let surrounding_empty_cells = 2;
        let chrome = 2 + widget_length + surrounding_empty_cells;

        let actual_width = content_width.saturating_sub(chrome as u16);

        // we need to substract the chrome from this...
        let segments = line.content.styled_segments(actual_width);
        let actual_width = actual_width as usize;

        let rows = segments
            .into_iter()
            .enumerate()
            .map(|(i, styled)| {
                // Measure ANSI overhead so the padding format fills visible columns correctly.
                let extras = styled.len() - ansi_width::ansi_width(&styled);

                let widget = if 0 == i {
                    LineWidget::Nr(nr)
                } else {
                    LineWidget::Continuation
                };
                let used_width = actual_width + extras;
                let l = format!("│{widget}│ {styled:<width$} ", width = used_width,);

                tracing::info!(
                    content_width,
                    actual_width,
                    extras,
                    used_width,
                    l = l.len(),
                    styled.len = styled.len(),
                    "FormattedRow",
                );

                FormattedRow(l)
            })
            .collect();

        LineGroup(rows)
    }
}

impl Lineable for InlineParts {
    fn do_thing(self, width: u16) -> LineGroup {
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
                    let p = std::mem::take(&mut current);
                    segments.push(FormattedRow(pad(&p, width)));
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

fn pad(original: &str, width: u16) -> String {
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

// --- ColumnPair ------------------------------------------------------------------

/// Owns the terminal-width-to-content-width conversion and zips two [`Column`]s.
// NOTE: consider if I need some kind of builder here
#[derive(Debug)]
pub struct ColumnPair {
    pub content_width: u16,
}

impl ColumnPair {
    pub fn new(terminal_width: u16) -> Self {
        // let separator = " │ ";
        // let border = "│";
        let content_width = terminal_width / 2;
        ColumnPair { content_width }
    }

    /// Create a fresh [`Column`] sized for this pair.
    pub fn column(&self) -> Column {
        Column::new(self.content_width)
    }

    /// Zip a left and right [`Column`] into final output lines.
    ///
    /// Groups are paired one-to-one. When one side has more wrapped rows in a
    /// group, the other side is padded with blank rows. Stops at the shorter
    /// column (caller is responsible for equalising heights beforehand).
    pub fn zip(&self, left: Column, right: Column) -> Vec<String> {
        let content_width = self.content_width as usize;

        let min_groups = left.groups.len().min(right.groups.len());
        let mut result = Vec::new();

        let mut left_iter = left.groups.into_iter();
        let mut right_iter = right.groups.into_iter();

        for _ in 0..min_groups {
            let left_rows = left_iter.next().unwrap().0;
            let right_rows = right_iter.next().unwrap().0;
            let max_rows = left_rows.len().max(right_rows.len());

            for i in 0..max_rows {
                let left = left_rows
                    .get(i)
                    .map(|row| row.0.as_str())
                    .unwrap_or_default();
                let right = right_rows
                    .get(i)
                    .map(|row| row.0.as_str())
                    .unwrap_or_default();
                let l_extras = left.chars().count() - ansi_width::ansi_width(left);
                let r_extras = right.chars().count() - ansi_width::ansi_width(right);
                let l_width = content_width + l_extras;
                let r_width = content_width + r_extras;
                result.push(format!("{left:<l_width$}{right:<r_width$}",));
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::content::Highlighted;

    fn with_nr(n: usize, s: &str) -> WithLineNumber {
        WithLineNumber::new(n, s.to_string())
    }

    fn highlighted(s: &str) -> WithLineNumber {
        WithLineNumber::new(
            1,
            Highlighted::new(s, Arc::new(|t: &str| format!("[hl]{t}[/]"))),
        )
    }

    #[test]
    fn column_push_with_nr() {
        let mut col = Column::new(20);
        col.push(with_nr(4, "hello"));
        let row = &col.groups[0].0[0].0;
        // nr=4 (0-based) → displayed as 5
        assert!(row.starts_with("   5 │ hello"), "got: {row:?}");
    }

    #[test]
    fn column_push_wraps_into_continuation_rows() {
        let mut col = Column::new(5);
        col.push(with_nr(0, "hello world"));
        let group = &col.groups[0].0;
        assert_eq!(group.len(), 3); // "hello", " worl", "d"
        assert!(
            group[0].0.starts_with("   1 │"),
            "first row: {:?}",
            group[0].0
        );
        assert!(
            group[1].0.starts_with("   ┆ │"),
            "cont row: {:?}",
            group[1].0
        );
    }

    #[test]
    fn column_blank_adds_filler_rows() {
        let mut col = Column::new(10);
        col.append_blank(3);
        assert_eq!(col.row_count(), 3);
        for g in &col.groups {
            // blank rows have no widget prefix, just padded spaces
            assert_eq!(g.0[0].0.len(), 10, "got: {:?}", g.0[0].0);
            assert!(g.0[0].0.trim().is_empty(), "got: {:?}", g.0[0].0);
        }
    }

    #[test]
    fn column_pair_zip_symmetric() {
        let pair = ColumnPair::new(40);
        let mut left = pair.column();
        let mut right = pair.column();
        left.push(with_nr(1, "left line 1"));
        left.push(with_nr(2, "left line 2"));
        right.push(with_nr(1, "right line 1"));
        right.push(with_nr(2, "right line 2"));

        let lines = pair.zip(left, right);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("│ "));
        assert!(lines[0].contains("│ "));
    }

    #[test]
    fn column_pair_zip_asymmetric_wrapping() {
        let pair = ColumnPair::new(23); // content_width = (23-11)/2 = 6
        let mut left = pair.column();
        let mut right = pair.column();
        // "hello world" at width 6 wraps to 2 rows
        left.push(with_nr(1, "hello world"));
        right.push(with_nr(2, "short"));

        let lines = pair.zip(left, right);
        // left wraps to 2 rows, right has 1 → group produces 2 output lines
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn column_pair_content_width() {
        assert_eq!(ColumnPair::new(120).content_width, 54);
        assert_eq!(ColumnPair::new(80).content_width, 34);
        assert_eq!(ColumnPair::new(16).content_width, 2);
        assert_eq!(ColumnPair::new(10).content_width, 0); // saturating
    }

    #[test]
    fn highlighted_line_segments_are_styled() {
        let mut col = Column::new(20);
        col.push(highlighted("hello"));
        let row = &col.groups[0].0[0].0;
        assert_eq!(row, "   2 │ [hl]hello               [/]")
    }
}
