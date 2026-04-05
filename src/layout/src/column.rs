use std::fmt::{self};

use crate::wrap::wrap_plain;

/// A value that can be rendered into a [`LineGroup`] at a fixed column width.
///
/// Implementors wrap content that is too wide into multiple [`FormattedRow`]s.
/// This is the interface [`Column::push`] accepts.
///
/// # Implementors
///
/// - `String` / `&str` — plain unstyled text; used for headers and labels.
/// - [`Highlighted`](crate::Highlighted) and [`InlineParts`](crate::InlineParts) —
///   ANSI-styled content.
/// - [`PrefixedLine`] — wraps another [`Lineable`] and decorates each row with a
///   line-number prefix.
pub trait Lineable {
    /// Produce all display rows for the given implementaor under a given column width.
    ///
    /// If the content is wider than `content_width` it must wrap, producing
    /// multiple [`FormattedRow`]s inside the returned [`LineGroup`].
    fn as_line_group(&self, content_width: u16) -> LineGroup;
}

/// All display rows produced from one logical line pushed onto a [`Column`].
///
/// A [`LineGroup`] contains exactly one [`FormattedRow`] when the line fits within
/// the column width, or more when it wraps. [`ColumnPair::zip`] pairs groups from
/// the left and right columns one-to-one and pads the shorter side with blank rows.
pub struct LineGroup(pub Vec<FormattedRow>);

/// A single terminal output row: a string already padded to the column's visible
/// width and optionally containing ANSI escape codes.
///
/// The string is ready to be printed as-is; no further padding or trimming is
/// needed. ANSI codes are always self-contained within a single `FormattedRow` —
/// no escape sequence ever straddles a row boundary.
pub struct FormattedRow(pub String);

impl FormattedRow {
    fn blank(content_width: u16) -> Self {
        let w = content_width as usize;
        FormattedRow(format!("{blank:<w$}", blank = ""))
    }
}

/// The 5-character slot between the `│` separators, carrying the line number or a
/// decoration.
///
/// Rendered as part of the line-number chrome added by [`PrefixedLine`]:
///
/// ```text
/// │   3 │ content    ← Nr(2)        (0-based stored; displayed as idx + 1)
/// │   ┆ │ continued  ← Continuation (wrapped overflow of the line above)
/// │     │ filler     ← Filler       (placeholder on the opposite side of a gap)
/// ```
pub(crate) enum LineWidget {
    /// A real line number. Stored 0-based; displayed as `idx + 1`.
    Nr(usize),
    /// A wrapped continuation of the previous line (`┆`).
    Continuation,
    /// No line number — blank placeholder used by [`PrefixedLine::Filler`].
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

/// Visible columns consumed by the line-number prefix on each side:
/// `│`(1) + [`LineWidget`](5) + `│`(1) + space(1) + trailing space(1) = 9.
const CHROME: u16 = 9;

/// Wrap `value` with the `│ widget │ … ` prefix to produce a [`FormattedRow`].
///
/// `visual_width` is the number of *visible* columns available for `value`.
/// ANSI overhead (bytes that don't advance the cursor) is measured and added to
/// the format-string width so the padding fills exactly `visual_width` columns.
fn format_chrome_row(widget: LineWidget, value: &str, visual_width: usize) -> FormattedRow {
    let extras = value.len() - ansi_width::ansi_width(value);
    let required_width = visual_width + extras;
    FormattedRow(format!("│{widget}│ {value:<required_width$} "))
}

/// A [`Lineable`] that decorates content with a line-number prefix (`│ nr │`).
///
/// This is the primary [`Lineable`] type in everdiff's code view. Two variants:
///
/// - `Numbered` — pairs any [`Lineable`] content with a 0-based line index
///   (displayed as `nr + 1`).
/// - `Filler` — a blank placeholder row used to keep the two sides of a
///   [`ColumnPair`] aligned when one document has a block the other lacks.
///
/// # Example
///
/// ```rust,ignore
/// col.push(PrefixedLine::numbered(5, Highlighted::new("key: value", dimmed)));
/// col.push(PrefixedLine::Filler);
/// ```
pub enum PrefixedLine {
    /// A content line with a line number.
    Numbered {
        /// 0-based line index; rendered as `nr + 1`.
        nr: usize,
        /// The styled content to display after the chrome.
        content: Box<dyn Lineable>,
    },
    /// A blank chrome-width placeholder, used to align gaps between documents.
    Filler,
}

impl PrefixedLine {
    /// Construct a [`PrefixedLine::Numbered`] from any [`Lineable`].
    ///
    /// `nr` is a **0-based** line index; it will be displayed as `nr + 1`.
    pub fn numbered(nr: usize, content: impl Lineable + 'static) -> Self {
        PrefixedLine::Numbered {
            nr,
            content: Box::new(content),
        }
    }
}

impl Lineable for PrefixedLine {
    fn as_line_group(&self, content_width: u16) -> LineGroup {
        let actual_width_u16 = content_width.saturating_sub(CHROME);
        let actual_width = actual_width_u16 as usize;

        let rows = match self {
            PrefixedLine::Numbered { nr, content } => content
                .as_line_group(actual_width_u16)
                .0
                .into_iter()
                .enumerate()
                .map(|(i, row)| {
                    let widget = if i == 0 {
                        LineWidget::Nr(*nr)
                    } else {
                        LineWidget::Continuation
                    };
                    format_chrome_row(widget, &row.0, actual_width)
                })
                .collect(),

            PrefixedLine::Filler => vec![format_chrome_row(LineWidget::Filler, "", actual_width)],
        };

        LineGroup(rows)
    }
}

impl Lineable for String {
    fn as_line_group(&self, content_width: u16) -> LineGroup {
        let group = wrap_plain(self, content_width)
            .into_iter()
            .map(FormattedRow)
            .collect();

        LineGroup(group)
    }
}

impl Lineable for &str {
    fn as_line_group(&self, content_width: u16) -> LineGroup {
        let group = wrap_plain(self, content_width)
            .into_iter()
            .map(FormattedRow)
            .collect();

        LineGroup(group)
    }
}

/// One side of a two-column diff view.
///
/// Lines are pushed in order via [`push`](Column::push); each becomes a
/// [`LineGroup`] (one or more [`FormattedRow`]s when a line wraps). Build both
/// sides from a [`ColumnPair`] so their widths are guaranteed to match, then pass
/// them to [`ColumnPair::zip`] to produce the final interleaved output.
///
/// Use [`append_blank`](Column::append_blank) or [`prepend_blank`](Column::prepend_blank)
/// to add padding so the two sides have an equal number of groups before zipping.
pub struct Column {
    /// The number of visible terminal columns available for content in this column.
    pub content_width: u16,
    pub(crate) groups: Vec<LineGroup>,
}

impl Column {
    /// Create an empty column with the given visible content width.
    pub fn new(content_width: u16) -> Self {
        Column {
            content_width,
            groups: Vec::new(),
        }
    }

    /// Append a line to the bottom of the column.
    pub fn push(&mut self, line: impl Lineable) {
        let group = line.as_line_group(self.content_width);
        self.groups.push(group);
    }

    /// Insert a line at the top of the column.
    pub fn prepend(&mut self, line: impl Lineable) {
        let group = line.as_line_group(self.content_width);
        self.groups.insert(0, group);
    }

    /// Append `count` blank rows to the bottom (no content, no line-number chrome).
    pub fn append_blank(&mut self, count: usize) {
        for _ in 0..count {
            self.groups
                .push(LineGroup(vec![FormattedRow::blank(self.content_width)]));
        }
    }

    /// Insert `count` blank rows at the top.
    pub fn prepend_blank(&mut self, count: usize) {
        let mut new_line_group = Vec::with_capacity(self.groups.len() + count);
        for _ in 0..count {
            new_line_group.push(LineGroup(vec![FormattedRow::blank(self.content_width)]));
        }
        new_line_group.append(&mut self.groups);
        self.groups = new_line_group;
    }

    /// Total number of display rows across all groups (accounting for wrapped lines).
    pub fn row_count(&self) -> usize {
        self.groups.iter().map(|g| g.0.len()).sum()
    }
}

/// Coordinates two [`Column`]s for a side-by-side diff view.
///
/// `ColumnPair` is the entry point for building two-column output:
///
/// 1. Create a pair from the terminal width: `ColumnPair::new(terminal_width)`.
/// 2. Create both columns from it via [`column`](ColumnPair::column) — this
///    guarantees they share the same `content_width`.
/// 3. Fill each column with [`Lineable`] values.
/// 4. Call [`zip`](ColumnPair::zip) to interleave the rows into a `Vec<String>`.
///
/// The pair splits the terminal width evenly: each column gets
/// `terminal_width / 2` visible columns.
#[derive(Debug)]
pub struct ColumnPair {
    /// Visible terminal columns available to each side.
    pub content_width: u16,
}

impl ColumnPair {
    /// Create a pair sized for the given terminal width.
    ///
    /// Each column receives `terminal_width / 2` visible columns.
    pub fn new(terminal_width: u16) -> Self {
        let content_width = terminal_width / 2;
        ColumnPair { content_width }
    }

    /// Create a fresh [`Column`] sized to this pair's `content_width`.
    ///
    /// Call this twice — once for each side — to get a matched left/right pair.
    pub fn column(&self) -> Column {
        Column::new(self.content_width)
    }

    /// Interleave a left and right [`Column`] into final printable lines.
    ///
    /// Groups are paired one-to-one in order. Within each group, if one side has
    /// more wrapped rows than the other, the shorter side is padded with empty
    /// strings for that group only. The total number of output lines equals the
    /// sum of `max(left_rows, right_rows)` across all groups.
    ///
    /// # Panics
    ///
    /// Panics if the two columns have a different number of groups. Use
    /// [`append_blank`](Column::append_blank) or [`prepend_blank`](Column::prepend_blank)
    /// to equalise them beforehand.
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
