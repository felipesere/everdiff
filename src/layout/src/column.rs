use std::fmt::{self};

use crate::{content::StyledContent, wrap::wrap_plain};

/// All display rows produced from one logical line (≥1 when the line wraps).
pub struct LineGroup(pub Vec<FormattedRow>);

/// A single terminal output row: a padded, optionally ANSI-styled string
/// ready to be printed as-is.
pub struct FormattedRow(pub String);

impl FormattedRow {
    fn blank(content_width: u16) -> Self {
        let w = content_width as usize;
        FormattedRow(format!("{blank:<w$}", blank = ""))
    }
}

// --- Lineable trait --------------------------------------------------------------

/// A value that can render itself into a [`LineGroup`] at a given column width.
pub trait Lineable {
    fn into_line_group(self, content_width: u16) -> LineGroup;
}

// --- LineWidget ------------------------------------------------------------------

/// The 5-character prefix shown between the `│` separators in each display row.
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

// --- Chrome helpers --------------------------------------------------------------

/// Columns consumed by the line-number chrome:
/// `│`(1) + widget(5) + `│`(1) + space(1) + trailing space(1) = 9.
const CHROME: u16 = 9;

fn format_chrome_row(widget: LineWidget, value: &str, visual_width: usize) -> FormattedRow {
    let extras = value.len() - ansi_width::ansi_width(value);
    let used_width = visual_width + extras;
    FormattedRow(format!("│{widget}│ {value:<used_width$} "))
}

// --- PrefixedLine ----------------------------------------------------------------

/// A line pushed into a [`Column`], rendered with line-number chrome (`│ nr │`).
///
/// - `Numbered` — real content with a 0-based line index. Displayed as `nr + 1`.
/// - `Filler` — a blank row that still occupies the chrome columns, used to pad
///   the shorter side of a [`ColumnPair`] when the two sides have different heights.
pub enum PrefixedLine {
    Numbered {
        /// 0-based line index. Displayed as `nr + 1`.
        nr: usize,
        content: Box<dyn StyledContent>,
    },
    Filler,
}

impl PrefixedLine {
    pub fn numbered(nr: usize, content: impl StyledContent + 'static) -> Self {
        PrefixedLine::Numbered {
            nr,
            content: Box::new(content),
        }
    }
}

impl Lineable for PrefixedLine {
    fn into_line_group(self, content_width: u16) -> LineGroup {
        let actual_width_u16 = content_width.saturating_sub(CHROME);
        let actual_width = actual_width_u16 as usize;

        let rows = match self {
            PrefixedLine::Numbered { nr, content } => content
                .styled_segments(actual_width_u16)
                .into_iter()
                .enumerate()
                .map(|(i, styled)| {
                    let widget = if i == 0 {
                        LineWidget::Nr(nr)
                    } else {
                        LineWidget::Continuation
                    };
                    format_chrome_row(widget, &styled, actual_width)
                })
                .collect(),

            PrefixedLine::Filler => vec![format_chrome_row(LineWidget::Filler, "", actual_width)],
        };

        LineGroup(rows)
    }
}

// --- Lineable impls for primitive types ------------------------------------------

impl Lineable for String {
    fn into_line_group(self, content_width: u16) -> LineGroup {
        let group = wrap_plain(&self, content_width)
            .into_iter()
            .map(FormattedRow)
            .collect();

        LineGroup(group)
    }
}

impl Lineable for &str {
    fn into_line_group(self, content_width: u16) -> LineGroup {
        let group = wrap_plain(self, content_width)
            .into_iter()
            .map(FormattedRow)
            .collect();

        LineGroup(group)
    }
}

// --- Column ----------------------------------------------------------------------

/// One side of a two-column layout. Knows its own `content_width`.
///
/// Build by calling [`push`](Column::push) and [`append_blank`](Column::append_blank).
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

    /// Append a line to the bottom of the column.
    pub fn push(&mut self, line: impl Lineable) {
        let group = line.into_line_group(self.content_width);
        self.groups.push(group);
    }

    /// Insert a line at the top of the column.
    pub fn prepend(&mut self, line: impl Lineable) {
        let group = line.into_line_group(self.content_width);
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

// --- ColumnPair ------------------------------------------------------------------

/// Owns the terminal-width-to-content-width conversion and zips two [`Column`]s.
// NOTE: consider if I need some kind of builder here
#[derive(Debug)]
pub struct ColumnPair {
    pub content_width: u16,
}

impl ColumnPair {
    pub fn new(terminal_width: u16) -> Self {
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
                result.push(format!("{left:<l_width$}{right:<r_width$}"));
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

    fn with_nr(n: usize, s: &str) -> PrefixedLine {
        PrefixedLine::numbered(n, s.to_string())
    }

    fn highlighted(s: &str) -> PrefixedLine {
        PrefixedLine::numbered(
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
        assert!(row.starts_with("│   5 │ hello"), "got: {row:?}");
    }

    #[test]
    fn column_push_wraps_into_continuation_rows() {
        let mut col = Column::new(14);
        col.push(with_nr(0, "hello world"));
        let group = &col.groups[0].0;
        assert_eq!(group.len(), 3); // "hello", " worl", "d"
        assert!(
            group[0].0.starts_with("│   1 │ hello "),
            "first row: {:?}",
            group[0].0
        );
        assert!(
            group[1].0.starts_with("│   ┆ │  worl "),
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
        let pair = ColumnPair::new(50);
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
        let pair = ColumnPair::new(30);
        let mut left = pair.column();
        let mut right = pair.column();
        // "hello world" at width 6 wraps to 2 rows
        left.push(with_nr(1, "hello world"));
        right.push(with_nr(2, "short"));

        let lines = pair.zip(left, right);
        dbg!(&lines);
        // left wraps to 2 rows, right has 1 → group produces 2 output lines
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn highlighted_line_segments_are_styled() {
        let mut col = Column::new(20);
        col.push(highlighted("hello"));
        let row = &col.groups[0].0[0].0;
        assert_eq!(row, "│   2 │ [hl]hello      [/] ")
    }
}
