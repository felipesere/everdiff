use everdiff_line::Line;
use owo_colors::{OwoColorize, Style};

use crate::snippet::LineWidget;

/// A plain text chunk that fits within the column width, containing no ANSI codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment(pub String);

/// A source line that has been wrapped into one or more segments.
#[derive(Debug, Clone)]
pub struct WrappedLine {
    pub line_nr: Line,
    pub segments: Vec<Segment>,
}

/// A fully formatted row: line number widget + styled content + padding.
/// e.g. "  3 │ content     "
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormattedRow(pub String);

/// All the formatted rows produced from one source line (may be multiple if wrapped).
#[derive(Debug, Clone)]
pub struct SourceLineGroup(pub Vec<FormattedRow>);

/// One complete column (left or right side), composed of source line groups.
#[derive(Debug, Clone)]
pub struct Column(pub Vec<SourceLineGroup>);

/// Split plain text into segments that each fit within `max_width` visible columns.
/// Uses unicode-width for correct width measurement.
pub fn wrap_text(text: &str, max_width: usize) -> Vec<Segment> {
    if max_width == 0 {
        return vec![Segment(String::new())];
    }

    if text.is_empty() {
        return vec![Segment(String::new())];
    }

    let mut segments = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;

    for ch in text.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if current_width + ch_width > max_width && !current.is_empty() {
            segments.push(Segment(current));
            current = String::new();
            current_width = 0;
        }
        current.push(ch);
        current_width += ch_width;
    }

    if !current.is_empty() || segments.is_empty() {
        segments.push(Segment(current));
    }

    segments
}

impl WrappedLine {
    /// Construct a WrappedLine by wrapping the plain text content of a source line.
    pub fn new(line_nr: Line, text: &str, max_width: usize) -> Self {
        let segments = wrap_text(text, max_width);
        WrappedLine { line_nr, segments }
    }

    /// Style the segments and format them into rows with line number widgets and padding.
    /// The first segment gets the real line number; continuation segments get a blank line widget.
    pub fn format(self, style: Style, half_width: usize) -> SourceLineGroup {
        let rows = self
            .segments
            .into_iter()
            .enumerate()
            .map(|(i, Segment(text))| {
                let styled = text.style(style).to_string();
                let extras = styled.len() - ansi_width::ansi_width(&styled);

                let line_widget = if i == 0 {
                    LineWidget::from(self.line_nr)
                } else {
                    LineWidget::Continuation
                };

                FormattedRow(format!(
                    "{line_widget}│ {styled:<width$}",
                    width = half_width + extras
                ))
            })
            .collect();

        SourceLineGroup(rows)
    }
}

/// Variant that uses a raw usize line number (for render_changed_snippet which uses 0-based indices).
#[derive(Debug, Clone)]
pub struct WrappedLineUsize {
    pub line_nr: usize,
    pub segments: Vec<Segment>,
}

impl WrappedLineUsize {
    /// Style the segments and format them into rows, using LineWidget(Some(line_nr)).
    pub fn format_with_usize(self, style: Style, width: usize) -> SourceLineGroup {
        let rows = self
            .segments
            .into_iter()
            .enumerate()
            .map(|(i, Segment(text))| {
                let styled = text.style(style).to_string();
                let extras = styled.len() - ansi_width::ansi_width(&styled);

                let line_widget = if i == 0 {
                    LineWidget::Nr(self.line_nr)
                } else {
                    LineWidget::Continuation
                };

                FormattedRow(format!(
                    "{line_widget}│ {styled:<width$}",
                    width = width + extras
                ))
            })
            .collect();

        SourceLineGroup(rows)
    }
}

impl FormattedRow {
    /// Create a blank padded row (for gaps).
    pub fn blank(half_width: usize) -> Self {
        let line_widget = LineWidget::Filler;
        FormattedRow(format!("{line_widget}│ {blank:<half_width$}", blank = ""))
    }
}

impl SourceLineGroup {
    /// Number of visual rows this group occupies.
    pub fn row_count(&self) -> usize {
        self.0.len()
    }
}

impl Column {
    /// Total number of visual rows across all source line groups.
    pub fn row_count(&self) -> usize {
        self.0.iter().map(|g| g.row_count()).sum()
    }

    /// Zip two columns together for side-by-side display.
    /// Each source line group is paired; when one side has more rows in a group,
    /// the other side is padded with blank rows. Stops at the shorter column
    /// (matching the old zip behavior).
    /// Returns the combined lines as strings.
    pub fn zip_with(self, other: Column, half_width: usize) -> Vec<String> {
        let min_groups = self.0.len().min(other.0.len());
        // wtf is this +6
        let width = half_width + 6;

        let mut result = Vec::new();

        let mut left_iter = self.0.into_iter();
        let mut right_iter = other.0.into_iter();

        for _ in 0..min_groups {
            let left_group = left_iter.next().unwrap();
            let right_group = right_iter.next().unwrap();

            let left_rows = left_group.0;
            let right_rows = right_group.0;
            let max_rows = left_rows.len().max(right_rows.len());

            let blank = FormattedRow::blank(half_width);
            for i in 0..max_rows {
                let left = left_rows.get(i).map(|r| r.0.as_str()).unwrap_or(&blank.0);
                let right = right_rows.get(i).map(|r| r.0.as_str()).unwrap_or(&blank.0);

                // The left/right already contain "NNNN│ content" with padding,
                // we need to wrap them with the outer "│ " prefix
                result.push(format!("│ {left:<width$}│ {right:<width$}"));
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_text_short_line_no_wrapping() {
        let segments = wrap_text("hello", 10);
        assert_eq!(segments, vec![Segment("hello".to_string())]);
    }

    #[test]
    fn wrap_text_exact_width() {
        let segments = wrap_text("12345", 5);
        assert_eq!(segments, vec![Segment("12345".to_string())]);
    }

    #[test]
    fn wrap_text_exceeds_width() {
        let segments = wrap_text("1234567890", 5);
        assert_eq!(
            segments,
            vec![Segment("12345".to_string()), Segment("67890".to_string()),]
        );
    }

    #[test]
    fn wrap_text_empty_string() {
        let segments = wrap_text("", 10);
        assert_eq!(segments, vec![Segment("".to_string())]);
    }

    #[test]
    fn wrap_text_single_char() {
        let segments = wrap_text("x", 10);
        assert_eq!(segments, vec![Segment("x".to_string())]);
    }

    #[test]
    fn wrap_text_multi_segment() {
        let segments = wrap_text("abcdefghijklmno", 5);
        assert_eq!(
            segments,
            vec![
                Segment("abcde".to_string()),
                Segment("fghij".to_string()),
                Segment("klmno".to_string()),
            ]
        );
    }

    #[test]
    fn wrap_text_unicode_wide_chars() {
        // CJK characters are typically 2 columns wide
        let segments = wrap_text("漢字テスト", 6);
        // Each char is 2 wide, so 3 chars fit in width 6
        assert_eq!(
            segments,
            vec![Segment("漢字テ".to_string()), Segment("スト".to_string()),]
        );
    }

    #[test]
    fn wrapped_line_format_line_numbers() {
        let wl = WrappedLine {
            line_nr: Line::unchecked(5),
            segments: vec![
                Segment("first part".to_string()),
                Segment("second part".to_string()),
            ],
        };

        let group = wl.format(Style::new(), 20);
        assert_eq!(group.row_count(), 2);

        // First row should have the line number
        assert!(group.0[0].0.contains("5"));
        // Second row should have continuation marker
        let continuation_prefix = "  ┆ │";
        assert!(group.0[1].0.starts_with(continuation_prefix));
    }

    #[test]
    fn wrapped_line_single_segment_has_line_number() {
        let wl = WrappedLine {
            line_nr: Line::unchecked(3),
            segments: vec![Segment("short".to_string())],
        };

        let group = wl.format(Style::new(), 20);
        assert_eq!(group.row_count(), 1);
        assert!(group.0[0].0.contains("3"));
    }

    #[test]
    fn column_zip_with_symmetric() {
        let half_width = 20;
        let left = Column(vec![
            SourceLineGroup(vec![FormattedRow("   1 │ left line 1      ".to_string())]),
            SourceLineGroup(vec![FormattedRow("   2 │ left line 2      ".to_string())]),
        ]);
        let right = Column(vec![
            SourceLineGroup(vec![FormattedRow("   1 │ right line 1     ".to_string())]),
            SourceLineGroup(vec![FormattedRow("   2 │ right line 2     ".to_string())]),
        ]);

        let result = left.zip_with(right, half_width);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn column_zip_with_asymmetric_wrapping() {
        let half_width = 20;
        // Left side: first source line wraps to 2 rows
        let left = Column(vec![
            SourceLineGroup(vec![
                FormattedRow("   1 │ left part 1      ".to_string()),
                FormattedRow("     │ left part 2      ".to_string()),
            ]),
            SourceLineGroup(vec![FormattedRow("   2 │ left line 2      ".to_string())]),
        ]);
        // Right side: both source lines are single rows
        let right = Column(vec![
            SourceLineGroup(vec![FormattedRow("   1 │ right line 1     ".to_string())]),
            SourceLineGroup(vec![FormattedRow("   2 │ right line 2     ".to_string())]),
        ]);

        let result = left.zip_with(right, half_width);
        // First group: 2 rows (left wrapped), second group: 1 row each
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn column_row_count() {
        let col = Column(vec![
            SourceLineGroup(vec![
                FormattedRow("a".to_string()),
                FormattedRow("b".to_string()),
            ]),
            SourceLineGroup(vec![FormattedRow("c".to_string())]),
        ]);
        assert_eq!(col.row_count(), 3);
    }

    #[test]
    fn line_widget_shows_number_then_continuation_then_filler() {
        // A wrapped line should show the line number on the first row,
        // ┆ on continuation rows, and blanks for filler/gap rows.
        let wl = WrappedLine {
            line_nr: Line::unchecked(7),
            segments: vec![
                Segment("first chunk".to_string()),
                Segment("second chunk".to_string()),
            ],
        };

        let group = wl.format(Style::new(), 20);
        let blank = FormattedRow::blank(20);

        // line number row:  "  7 │ ..."
        assert!(
            group.0[0].0.starts_with("  7 │"),
            "expected line number, got: {:?}",
            group.0[0].0
        );
        // continuation row: "  ┆ │ ..."
        assert!(
            group.0[1].0.starts_with("  ┆ │"),
            "expected continuation, got: {:?}",
            group.0[1].0
        );
        // filler/blank row: "    │ ..."
        assert!(
            blank.0.starts_with("    │"),
            "expected filler, got: {:?}",
            blank.0
        );
    }

    #[test]
    fn formatted_row_blank() {
        let row = FormattedRow::blank(20);
        assert!(row.0.contains("│"));
        // Should be padded to the right width
        assert!(row.0.len() >= 20);
    }
}
