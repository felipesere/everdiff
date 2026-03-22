use std::fmt::{self};

use crate::{Highlighted, content::StyledContent, wrap::wrap_plain};

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
    fn blank(content_width: usize) -> Self {
        FormattedRow(format!("{blank:<content_width$}", blank = "",))
    }
}

/// All display rows produced from one logical [`Line`] (≥1 when the line wraps).
pub struct LineGroup(pub Vec<FormattedRow>);

// --- Line ------------------------------------------------------------------------
#[derive(Clone, Copy)]
pub enum LineNr {
    Nr(usize),
    // Take the space and add borders as if there was a number
    FillerNumber,
    None,
}

/// A single logical line of content, optionally carrying a line number.
///
/// May wrap into multiple display rows depending on the column width.
/// The `content` produces styled segments; the line widget is applied by
/// [`Column::push`] after wrapping.
pub struct Line {
    /// 0-based line index. Displayed as `nr + 1`. `None` → no line number widget.
    pub nr: LineNr,
    pub content: Box<dyn StyledContent>,
}

impl Line {
    pub fn blank() -> Self {
        Line {
            nr: LineNr::None,
            content: Box::new(""),
        }
    }

    pub fn new(content: impl StyledContent + 'static) -> Self {
        Line {
            nr: LineNr::None,
            content: Box::new(content),
        }
    }

    /// Attach a 0-based line index (displayed as `nr + 1`).
    pub fn with_nr(mut self, nr: usize) -> Self {
        self.nr = LineNr::Nr(nr);
        self
    }

    pub fn filler_nr(mut self) -> Self {
        self.nr = LineNr::FillerNumber;
        self
    }
}

// --- Column ----------------------------------------------------------------------

/// One side of a two-column layout. Knows its own `content_width`.
///
/// Build by calling [`push`](Column::push) and [`blank`](Column::blank).
/// Zip two columns together with [`ColumnPair::zip`].
pub struct Column {
    pub content_width: usize,
    pub(crate) groups: Vec<LineGroup>,
}

impl Column {
    pub fn new(content_width: usize) -> Self {
        Column {
            content_width,
            groups: Vec::new(),
        }
    }

    /// Append a logical line. Wraps its content to `content_width` and
    /// formats each segment with the appropriate line widget.
    pub fn push(&mut self, line: Line) {
        let nr = line.nr;
        let segments = line.content.styled_segments(self.content_width);

        let rows = segments
            .into_iter()
            .enumerate()
            .map(|(i, styled)| {
                // Measure ANSI overhead so the padding format fills visible columns correctly.
                let extras = styled.len() - ansi_width::ansi_width(&styled);

                let widget = match (i, nr) {
                    (0, LineNr::Nr(n)) => LineWidget::Nr(n),
                    (0, LineNr::None) => LineWidget::Filler,
                    (_, LineNr::Nr(_)) => LineWidget::Continuation,
                    _ => LineWidget::Filler,
                };

                FormattedRow(format!(
                    "{widget}│ {styled:<width$}",
                    width = self.content_width + extras,
                ))
            })
            .collect();

        self.groups.push(LineGroup(rows));
    }

    pub fn push_without_widget(&mut self, line: Line) {
        let segments = line.content.styled_segments(self.content_width);

        let rows = segments
            .into_iter()
            .map(|styled| {
                // Measure ANSI overhead so the padding format fills visible columns correctly.
                let extras = styled.len() - ansi_width::ansi_width(&styled);

                FormattedRow(format!(
                    "{styled:<width$}",
                    width = self.content_width + extras,
                ))
            })
            .collect();

        self.groups.push(LineGroup(rows));
    }

    // This should be the future. `do_thing`
    pub fn new_push(&mut self, line: impl Lineable) {
        let group = line.do_thing(self.content_width);
        self.groups.push(group);
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
    fn do_thing(self, content_width: usize) -> LineGroup;
}

impl Lineable for String {
    fn do_thing(self, content_width: usize) -> LineGroup {
        let group = wrap_plain(&self, content_width)
            .into_iter()
            .map(FormattedRow)
            .collect();

        LineGroup(group)
    }
}

impl Lineable for &str {
    fn do_thing(self, content_width: usize) -> LineGroup {
        let group = wrap_plain(self, content_width)
            .into_iter()
            .map(FormattedRow)
            .collect();

        LineGroup(group)
    }
}

impl Lineable for Highlighted {
    fn do_thing(self, content_width: usize) -> LineGroup {
        let group = wrap_plain(&self.text, content_width)
            .into_iter()
            .map(|seg| FormattedRow((self.highlight)(&seg)))
            .collect();

        LineGroup(group)
    }
}

// --- ColumnPair ------------------------------------------------------------------

/// Owns the terminal-width-to-content-width conversion and zips two [`Column`]s.
// NOTE: consider if I need some kind of builder here
#[derive(Debug)]
pub struct ColumnPair {
    pub content_width: usize,
    borders: Option<&'static str>,
    separator: Option<&'static str>,
}

impl ColumnPair {
    pub fn new(terminal_width: usize) -> Self {
        let separator = " │ ";
        let border = "│";
        ColumnPair {
            content_width: terminal_width.saturating_sub(separator.len() + 2 * border.len()) / 2,
            separator: Some(separator),
            borders: Some(border),
        }
    }

    pub fn new_plain(terminal_width: usize) -> Self {
        ColumnPair {
            content_width: terminal_width / 2,
            separator: None,
            borders: None,
        }
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
        let content_width = self.content_width;

        let min_groups = left.groups.len().min(right.groups.len());
        let mut result = Vec::new();

        let mut left_iter = left.groups.into_iter();
        let mut right_iter = right.groups.into_iter();

        let border = &self.borders.unwrap_or_default();
        let separator = &self.separator.unwrap_or_default();

        for _ in 0..min_groups {
            let left_rows = left_iter.next().unwrap().0;
            let right_rows = right_iter.next().unwrap().0;
            let max_rows = left_rows.len().max(right_rows.len());

            for i in 0..max_rows {
                let l = left_rows.get(i).map(|r| r.0.as_str()).unwrap_or_default();
                let r = right_rows.get(i).map(|r| r.0.as_str()).unwrap_or_default();
                result.push(format!(
                    "{border}{l:<content_width$}{separator}{r:<content_width$}{border}",
                ));
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

    fn plain(s: &str) -> Line {
        Line::new(s.to_string())
    }

    fn highlighted(s: &str) -> Line {
        Line::new(Highlighted::new(
            s,
            Arc::new(|t: &str| format!("[hl]{t}[/]")),
        ))
    }

    #[test]
    fn column_push_plain_no_wrap() {
        let mut col = Column::new(20);
        col.push(plain("hello"));
        assert_eq!(col.row_count(), 1);
        let row = &col.groups[0].0[0].0;
        assert!(row.starts_with("    │ hello"));
    }

    #[test]
    fn column_push_with_nr() {
        let mut col = Column::new(20);
        col.push(plain("hello").with_nr(4));
        let row = &col.groups[0].0[0].0;
        // nr=4 (0-based) → displayed as 5
        assert!(row.starts_with("  5 │ hello"), "got: {row:?}");
    }

    #[test]
    fn column_push_wraps_into_continuation_rows() {
        let mut col = Column::new(5);
        col.push(plain("hello world").with_nr(0));
        let group = &col.groups[0].0;
        assert_eq!(group.len(), 3); // "hello", " worl", "d"
        assert!(
            group[0].0.starts_with("  1 │"),
            "first row: {:?}",
            group[0].0
        );
        assert!(
            group[1].0.starts_with("  ┆ │"),
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
            assert!(g.0[0].0.starts_with("    │"), "got: {:?}", g.0[0].0);
        }
    }

    #[test]
    fn column_pair_zip_symmetric() {
        let pair = ColumnPair::new(40);
        let mut left = pair.column();
        let mut right = pair.column();
        left.push(plain("left line 1"));
        left.push(plain("left line 2"));
        right.push(plain("right line 1"));
        right.push(plain("right line 2"));

        let lines = pair.zip(left, right);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("│ "));
        assert!(lines[0].contains("│ "));
    }

    #[test]
    fn column_pair_zip_asymmetric_wrapping() {
        let pair = ColumnPair::new(15); // content_width = (15-5)/2 = 5
        let mut left = pair.column();
        let mut right = pair.column();
        // "hello world" at width 8 wraps to 2 rows
        left.push(plain("hello world"));
        right.push(plain("short"));

        let lines = pair.zip(left, right);
        // left wraps to 2 rows, right has 1 → group produces 2 output lines
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn column_pair_content_width() {
        assert_eq!(ColumnPair::new(120).content_width, 52);
        assert_eq!(ColumnPair::new(80).content_width, 32);
        assert_eq!(ColumnPair::new(16).content_width, 0);
        assert_eq!(ColumnPair::new(10).content_width, 0); // saturating
    }

    #[test]
    fn highlighted_line_segments_are_styled() {
        let mut col = Column::new(20);
        col.push(highlighted("hello"));
        let row = &col.groups[0].0[0].0;
        assert!(row.contains("[hl]hello[/]"), "got: {row:?}");
    }
}
