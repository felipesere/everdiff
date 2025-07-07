use core::option::Option::None;
use std::{
    cmp::{max, min},
    fmt::{self},
    iter::empty,
    num::NonZeroUsize,
    ops::{Add, Sub},
};

use ansi_width::ansi_width;
use anyhow::Context;
use owo_colors::{OwoColorize, Style};
use saphyr::{Indexable, MarkedYamlOwned, YamlDataOwned};

use crate::{YamlSource, path::Path};

#[derive(Debug, Clone)]
pub struct RenderContext {
    pub max_width: u16,
    pub visual_context: usize,
    pub color: Color,
}

impl RenderContext {
    pub fn new(max_width: u16, color: Color) -> Self {
        RenderContext {
            max_width,
            color,
            visual_context: 5,
        }
    }

    pub fn half_width(&self) -> usize {
        // includes a bit of random padding, do this proper later
        ((self.max_width - 16) / 2) as usize
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Color {
    Enabled,
    // mostly used in tests
    #[allow(dead_code)]
    Disabled,
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub struct Line(NonZeroUsize);

impl fmt::Debug for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("Line({})", &self.0))
    }
}

impl Line {
    pub(crate) fn get(&self) -> usize {
        self.0.get()
    }

    pub fn new(raw: usize) -> Option<Self> {
        Some(Line(NonZeroUsize::try_from(raw).ok()?))
    }

    #[cfg(test)]
    pub fn unchecked(n: usize) -> Self {
        Self(NonZeroUsize::try_from(n).unwrap())
    }

    pub fn one() -> Self {
        Self::new(1).unwrap()
    }
}

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Add<usize> for Line {
    type Output = Line;

    fn add(self, rhs: usize) -> Self::Output {
        Line(self.0.saturating_add(rhs))
    }
}

impl Add<i32> for Line {
    type Output = Line;

    fn add(self, rhs: i32) -> Self::Output {
        if rhs > 0 {
            let rhs = usize::try_from(rhs).expect("a small enough addition to line");
            Line::new(self.get().saturating_add(rhs)).unwrap()
        } else {
            // let rhs = usize::try_from(rhs.abs()).expect("a small enough addition to line");
            // Line::new(self.get().saturating_sub(rhs)).unwrap();
            unimplemented!("Are we really adding a negative number?");
        }
    }
}

impl Sub<usize> for Line {
    type Output = Line;

    fn sub(self, rhs: usize) -> Self::Output {
        let val = self.0.get();
        if val <= rhs {
            Line::one()
        } else {
            let val = val - rhs;
            Line::new(val).unwrap()
        }
    }
}

impl PartialOrd<usize> for Line {
    fn partial_cmp(&self, other: &usize) -> Option<std::cmp::Ordering> {
        self.0.get().partial_cmp(other)
    }
}

impl PartialEq<usize> for Line {
    fn eq(&self, other: &usize) -> bool {
        self.0.get().eq(other)
    }
}

impl From<Line> for LineWidget {
    fn from(value: Line) -> Self {
        // TODO: We still do gross `±1` math in here
        // if the `Line` concept pans out we can clear it
        Self(Some(value.0.get() - 1))
    }
}

struct Snippet<'source> {
    lines: &'source [&'source str],
    from: Line,
    to: Line,
}

// Nicer way to print the snippet to include line-nrs
impl<'source> fmt::Debug for Snippet<'source> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let digits: usize = usize::try_from(self.lines.len().checked_ilog10().unwrap_or(0) + 1)
            .unwrap_or(usize::MAX);
        f.debug_struct("Snippet")
            .field(
                "lines",
                &self
                    .lines
                    .iter()
                    .enumerate()
                    .map(|(nr, line)| format!("[{:>digits$}] {}", nr + 1, line))
                    .collect::<Vec<_>>(),
            )
            .field("from", &self.from)
            .field("to", &self.to)
            .finish()
    }
}

impl Snippet<'_> {
    pub fn try_new<'source>(
        lines: &'source [&'source str],
        from: Line,
        to: Line,
    ) -> Result<Snippet<'source>, anyhow::Error> {
        log::info!("Creating a new snippet");
        log::info!("---from: {from} to {to}");
        log::info!("{:#?}", lines);
        if to <= from {
            anyhow::bail!("'to' ({to}) was less than 'from' ({from})");
        }
        if to > lines.len() {
            anyhow::bail!(
                "'to' ({to}) reaches out of bounds of 'lines' ({})",
                lines.len()
            );
        }
        Ok(Snippet { lines, from, to })
    }

    pub fn iter(&self) -> SnippetLineIter {
        SnippetLineIter {
            snippet: self,
            current: self.from.get(),
        }
    }

    fn split(&self, split_at: Line) -> (Snippet<'_>, Snippet<'_>) {
        let left = Snippet {
            lines: self.lines,
            from: self.from,
            to: split_at,
        };
        let right = Snippet {
            lines: self.lines,
            from: split_at + 1,
            to: self.to,
        };
        (left, right)
    }
}

struct SnippetLineIter<'source> {
    snippet: &'source Snippet<'source>,
    current: usize,
}

impl<'source> Iterator for SnippetLineIter<'source> {
    type Item = (Line, &'source str);

    fn next(&mut self) -> Option<Self::Item> {
        if self.current <= self.snippet.to.get() {
            let content = self.snippet.lines[self.current - 1];
            let line_nr = Line::new(self.current)?;
            self.current += 1;
            Some((line_nr, content))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod snippet_tests {
    use super::{Line, Snippet};

    #[test]
    fn lines_of_simple_snippet() {
        let content = &[
            "a", // 1
            "b", // 2
            "c", // 3
            "d", // 4
            "e", // 5
        ];

        let snippet = Snippet::try_new(content, Line::unchecked(2), Line::unchecked(4)).unwrap();

        let actual_lines: Vec<_> = snippet
            .iter()
            .map(|(nr, content)| (nr, content.to_string()))
            .collect();

        assert_eq!(
            vec![
                (Line::unchecked(2), "b".to_string()),
                (Line::unchecked(3), "c".to_string()),
                (Line::unchecked(4), "d".to_string())
            ],
            actual_lines
        );
    }

    #[test]
    fn splitting_a_snippet() {
        let content = &[
            "a", // 1
            "b", // 2
            "c", // 3
            "d", // 4
            "e", // 5
            "f", // 6
            "g", // 7
            "h", // 8
        ];

        let snippet = Snippet::try_new(content, Line::unchecked(2), Line::unchecked(8)).unwrap();

        let (first, second) = snippet.split(Line::unchecked(6));

        let first_lines: Vec<_> = first
            .iter()
            .map(|(nr, content)| (nr, content.to_string()))
            .collect();

        let second_lines: Vec<_> = second
            .iter()
            .map(|(nr, content)| (nr, content.to_string()))
            .collect();

        assert_eq!(
            vec![
                (Line::unchecked(2), "b".to_string()),
                (Line::unchecked(3), "c".to_string()),
                (Line::unchecked(4), "d".to_string()),
                (Line::unchecked(5), "e".to_string()),
                (Line::unchecked(6), "f".to_string())
            ],
            first_lines
        );

        assert_eq!(
            vec![
                (Line::unchecked(7), "g".to_string()),
                (Line::unchecked(8), "h".to_string()),
            ],
            second_lines
        );
    }
}

struct Rendered {
    content: Vec<String>,
    lines_above: usize,
    lines_below: usize,
}

// We're going to need a "render context" or "render options" at some point
// to control a couple of aspects:
// * rendering the changes "in snippets" or not?
// * how many lines above and below to to show?
// * show colors or not?
// * line numbers that match up with the actual file
//    - this matters in particular for multi-doc docs
pub fn render_removal(
    ctx: &RenderContext,
    path_to_change: Path,
    removal: MarkedYamlOwned,
    left_doc: &YamlSource,
    right_doc: &YamlSource,
) -> String {
    render_change(
        ctx,
        path_to_change,
        removal,
        left_doc,
        right_doc,
        ChangeType::Removal,
    )
}

pub fn render_added(
    ctx: &RenderContext,
    path_to_change: Path,
    addition: MarkedYamlOwned,
    left_doc: &YamlSource,
    right_doc: &YamlSource,
) -> String {
    render_change(
        ctx,
        path_to_change,
        addition,
        left_doc,
        right_doc,
        ChangeType::Addition,
    )
}

enum ChangeType {
    Removal,
    Addition,
}

fn render_change(
    ctx: &RenderContext,
    path_to_change: Path,
    changed_yaml: MarkedYamlOwned,
    left_doc: &YamlSource,
    right_doc: &YamlSource,
    change_type: ChangeType,
) -> String {
    log::debug!("Rendering change for {}", path_to_change.jq_like());
    log::debug!("The changed yaml node looks like: {:#?}", changed_yaml);

    // Select primary and secondary documents based on change type
    // The `primary_doc` has more content and the changed_yaml will be highlighted.
    // The `secondary_doc` has the gap in it
    let (primary_doc, secondary_doc) = match change_type {
        ChangeType::Removal => (left_doc, right_doc),
        ChangeType::Addition => (right_doc, left_doc),
    };

    // Set up styles
    let colors = match ctx.color {
        Color::Enabled => (
            match change_type {
                ChangeType::Removal => owo_colors::Style::new().red(),
                ChangeType::Addition => owo_colors::Style::new().green(),
            },
            owo_colors::Style::new().dimmed(),
        ),
        Color::Disabled => (owo_colors::Style::new(), owo_colors::Style::new()),
    };
    // Meh...
    let unchanged = colors.1;

    let primary = render_primary_side(ctx, primary_doc, &changed_yaml, colors);
    // -----------------------------------------------------------------

    let secondary = render_secondary_side(
        ctx,
        primary_doc,
        secondary_doc,
        path_to_change,
        &change_type,
        &changed_yaml,
        unchanged,
    );

    // Combine the two sides based on change type
    match change_type {
        ChangeType::Removal => primary
            .iter()
            .zip(secondary)
            .map(|(l, r)| format!("│{l} │ {r}"))
            .collect::<Vec<_>>()
            .join("\n"),
        ChangeType::Addition => secondary
            .iter()
            .zip(primary)
            .map(|(l, r)| format!("│{l} │ {r}"))
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn render_primary_side(
    ctx: &RenderContext,
    primary_doc: &YamlSource,
    changed_yaml: &MarkedYamlOwned,
    (highlighting, unchanged): (Style, Style),
) -> Vec<String> {
    // Extract lines from primary document
    let primary_lines = primary_doc.lines();

    let change_start = primary_doc.relative_line(changed_yaml.span.start.line());
    let change_end = primary_doc.relative_line(changed_yaml.span.end.line());

    // Show a few more lines before and after the lines that have changed
    let start = change_start - ctx.visual_context;
    let end = min(change_end + ctx.visual_context, primary_doc.last_line);
    log::info!("Snippet for primary document");
    let primary_snippet =
        Snippet::try_new(&primary_lines, start, end).expect("Primary snippet could not be created");

    // Format the primary side
    let changed_range = change_start..=change_end;
    log::info!("We will highlight {change_start}..={change_end}");
    primary_snippet
        .iter()
        .map(move |(line_nr, line)| {
            let line = if changed_range.contains(&line_nr) {
                line.style(highlighting).to_string()
            } else {
                line.style(unchanged).to_string()
            };

            let extras = line.len() - ansi_width(&line);
            let line_nr = LineWidget::from(line_nr);
            format!(
                "{line_nr}│ {line:<width$}",
                width = ctx.half_width() + extras
            )
        })
        .collect()
}

// This is the side that would have a gap!
fn render_secondary_side(
    ctx: &RenderContext,
    primary_doc: &YamlSource,
    secondary_doc: &YamlSource,
    path_to_change: Path,
    change_type: &ChangeType,
    changed: &MarkedYamlOwned,
    unchanged: Style,
) -> Vec<String> {
    // Build the secondary side
    let secondary_lines = secondary_doc.lines();

    // Find corresponding nodes in secondary document
    // I think this is more complex than it initially seems.
    // The goal is to get the spans of the nodes that need to surround the gap.
    // Therefor I need know what nodes should be there, and then translate
    // that into the other document. I tend to do that via the `path`
    //
    // BUT(!) paths don't necessarily carry over to the other docment.
    // e.g. if the path to the change is `.people.3`
    // the surround nodes could be (.people.2, .people.4)
    // but who knows if the array has sufficient elements?!
    let parent = path_to_change.parent().unwrap();
    let primary_parent_node = node_in(&primary_doc.yaml, &parent).unwrap();

    let (before_path, after_path) = surrounding_paths(primary_parent_node, &path_to_change);

    log::debug!(
        "The before node is {:?}",
        &before_path.as_ref().map(|p| p.jq_like())
    );
    log::debug!(
        "The after node is {:?}",
        &after_path.as_ref().map(|p| p.jq_like())
    );

    let size_of_gap = node_height(changed);
    log::info!("We estimate the size of the gap to be: {size_of_gap}");

    let candidate_node_before_change = before_path.and_then(|p| node_in(&secondary_doc.yaml, &p));
    let candidate_node_after_change = after_path.and_then(|p| node_in(&secondary_doc.yaml, &p));

    let gap_start = candidate_node_before_change
        .map(|before| {
            let n = match primary_parent_node.data {
                YamlDataOwned::Sequence(_) => 1,
                _ => 0,
            };
            log::debug!("weird adjustment factor: {n}");
            primary_doc.relative_line(before.span.end.line() - n)
        })
        .unwrap_or(Line::one());

    log::debug!("The gap starts at: {gap_start}");

    // If we can find a node after the change use its line number, other wise guess based on the
    // start of the gap and the length of the change
    let gap_end = if let Some(after_node) = candidate_node_after_change {
        log::debug!(
            "Using start of after_node to find end of gap: {}",
            after_node.span.start.line()
        );
        primary_doc.relative_line(after_node.span.start.line())
    } else {
        // doing "+1" because keys and values are not on the same line:
        // foo: <--- the key
        //   name: 'abc'
        //   thing: true
        //
        // the node height is 2, but the total thing should be 3
        // maybe this is no longer relevant?
        log::debug!("No after_node present, using height {size_of_gap} of the changed_yaml");

        gap_start + size_of_gap
    };
    log::debug!("The gap ends on {gap_end}");

    let snippet_start = gap_start - ctx.visual_context;
    let snippet_end = min(gap_end + ctx.visual_context, secondary_doc.last_line);

    // Create snippet for secondary document
    let snippet = Snippet::try_new(&secondary_lines, snippet_start, snippet_end)
        .with_context(|| {
            format!(
                "Failed to create a snippet for change {} in {}:{}",
                path_to_change.jq_like(),
                secondary_doc.file,
                secondary_doc.index,
            )
        })
        .unwrap();

    log::debug!("The snippet is at: {:#?}", &snippet);
    log::debug!("The gap starts at: {}", &gap_start);

    let (before_gap, after_gap) = snippet.split(gap_start);

    // Format the secondary side with gap
    let pre_gap = before_gap.iter().map(|(line_nr, line)| {
        let line = line.style(unchanged).to_string();
        let extras = line.len() - ansi_width(&line);

        let line_nr = LineWidget::from(line_nr);
        format!(
            "{line_nr}│ {line:<width$}",
            width = ctx.half_width() + extras
        )
    });

    let change_size = node_height(changed);
    // Are we adding more weird adjustments here?
    // We did similar `+1` math above with node_height
    let change_size = match primary_parent_node.data {
        YamlDataOwned::Mapping(_) => change_size + 1,
        _ => change_size,
    };
    let gap = (0..change_size).map(|_| {
        let l = LineWidget(None);
        match change_type {
            ChangeType::Removal => format!("{l}│"),
            ChangeType::Addition => {
                format!("{l}│ {line:<width$}", line = "", width = ctx.half_width())
            }
        }
    });

    let post_gap = after_gap.iter().map(|(line_nr, line)| {
        let line = line.style(unchanged).to_string();
        let extras = line.len() - ansi_width(&line);

        let line_nr = LineWidget::from(line_nr);
        format!(
            "{line_nr}│ {line:<width$}",
            width = ctx.half_width() + extras
        )
    });

    pre_gap.chain(gap).chain(post_gap).collect()
}

fn node_height(changed_yaml: &MarkedYamlOwned) -> usize {
    let start = changed_yaml.span.start.line();
    let end = changed_yaml.span.end.line();
    max(end - start, 1)
}

#[cfg(test)]
mod test_node_height {
    use indoc::indoc;
    use saphyr::{Indexable, LoadableYamlNode, MarkedYamlOwned};

    use crate::snippet::node_height;

    #[test]
    fn height_of_simple_string() {
        let raw = indoc! {r#"
          element: "Hi there"
        "#};

        let mut yaml = MarkedYamlOwned::load_from_str(raw).unwrap();
        let yaml = yaml.remove(0);
        let element = yaml.get("element").unwrap();

        assert_eq!(1, node_height(element));
    }

    #[test]
    fn height_of_multiline_string() {
        let raw = indoc! {r#"
          element: |
            This is
            great and many
            lines long
        "#};

        let mut yaml = MarkedYamlOwned::load_from_str(raw).unwrap();
        let yaml = yaml.remove(0);
        let element = yaml.get("element").unwrap();

        assert_eq!(3, node_height(element));
    }

    #[test]
    fn height_of_boolean() {
        let raw = indoc! {r#"
          element: true
        "#};

        let mut yaml = MarkedYamlOwned::load_from_str(raw).unwrap();
        let yaml = yaml.remove(0);
        let element = yaml.get("element").unwrap();

        assert_eq!(1, node_height(element));
    }

    #[test]
    fn height_of_integer() {
        let raw = indoc! {r#"
          element: 7
        "#};

        let mut yaml = MarkedYamlOwned::load_from_str(raw).unwrap();
        let yaml = yaml.remove(0);
        let element = yaml.get("element").unwrap();

        assert_eq!(1, node_height(element));
    }

    #[test]
    fn height_of_null() {
        let raw = indoc! {r#"
          element: ~
        "#};

        let mut yaml = MarkedYamlOwned::load_from_str(raw).unwrap();
        let yaml = yaml.remove(0);
        let element = yaml.get("element").unwrap();

        assert_eq!(1, node_height(element));
    }

    #[test]
    fn height_of_sequence() {
        let raw = indoc! {r#"
          element: 
            - first
            - second
            - third: 3
              name: dfjsdklf
        "#};

        let mut yaml = MarkedYamlOwned::load_from_str(raw).unwrap();
        let yaml = yaml.remove(0);
        let element = yaml.get("element").unwrap();

        assert_eq!(4, node_height(element));
    }

    #[test]
    fn height_of_array_element() {
        let raw = indoc! {r#"
          thing:
            - foo: 1
            - foo: 2
              bar: yay!
            - foo: 3
              bar: yay!
              wtf: true
        "#};

        let mut yaml = MarkedYamlOwned::load_from_str(raw).unwrap();
        let yaml = yaml.remove(0);
        let element = yaml.get("thing").and_then(|thing| thing.get(1)).unwrap();

        assert_eq!(2, node_height(element));
    }

    #[test]
    fn height_of_mapping() {
        let raw = indoc! {r#"
          element:
            thing: 3
            goo:
              dfjsdklf: 1
              item: glasses
        "#};

        let mut yaml = MarkedYamlOwned::load_from_str(raw).unwrap();
        let yaml = yaml.remove(0);
        let element = yaml.get("element").unwrap();

        assert_eq!(4, node_height(element));
    }
}

pub fn render_difference(
    ctx: &RenderContext,
    path_to_change: Path,
    left: MarkedYamlOwned,
    left_doc: &YamlSource,
    right: MarkedYamlOwned,
    right_doc: &YamlSource,
) -> String {
    let highlight = if ctx.color == Color::Enabled {
        Style::new().bold()
    } else {
        Style::new()
    };
    let title = format!(
        "Changed: {p}:",
        p = highlight.style(path_to_change.jq_like())
    );

    let max_left = (ctx.max_width - 16) / 2; // includes a bit of random padding, do this proper later
    let smaller_context = RenderContext {
        max_width: max_left,
        color: ctx.color,
        visual_context: 5,
    };
    let left = render_changed_snippet(&smaller_context, left_doc, left);
    let right = render_changed_snippet(&smaller_context, right_doc, right);

    // TODO: this `6` is horrid... I'll have to find a way around this...
    let n = usize::from(max_left + 6);
    let filler = || std::iter::repeat(format!("{:>n$}", ""));

    let above_filler = left.lines_above.abs_diff(right.lines_above);
    let below_filler = left.lines_below.abs_diff(right.lines_below);

    let (left_top_filler, right_top_filler) = if left.lines_above < right.lines_above {
        (
            itertools::Either::Left(filler().take(above_filler)),
            itertools::Either::Right(empty::<String>()),
        )
    } else {
        (
            itertools::Either::Right(empty::<String>()),
            itertools::Either::Left(filler().take(above_filler)),
        )
    };

    let (left_bottom_filler, right_bottom_filler) = if left.lines_below < right.lines_below {
        (
            itertools::Either::Left(filler().take(below_filler)),
            itertools::Either::Right(empty::<String>()),
        )
    } else {
        (
            itertools::Either::Right(empty::<String>()),
            itertools::Either::Left(filler().take(below_filler)),
        )
    };

    let left = left_top_filler
        .into_iter()
        .chain(left.content)
        .chain(left_bottom_filler);

    let right = right_top_filler
        .into_iter()
        .chain(right.content)
        .chain(right_bottom_filler);

    let body = left
        .zip(right)
        .map(|(l, r)| format!("{l} │ {r}"))
        .collect::<Vec<_>>()
        .join("\n");

    format!("{title}\n{body}")
}

fn render_changed_snippet(
    ctx: &RenderContext,
    source: &YamlSource,
    changed_yaml: MarkedYamlOwned,
) -> Rendered {
    // lines to render above and below if available...
    let context = 5;
    let start_line_of_document = source.yaml.span.start.line();

    let lines: Vec<_> = source.content.lines().map(|s| s.to_string()).collect();

    let changed_line = changed_yaml.span.start.line() - start_line_of_document;
    let start = changed_line.saturating_sub(context) + 1;
    let end = min(changed_line + context, lines.len());
    let left_snippet = &lines[start..end];

    let (added, unchaged) = match ctx.color {
        Color::Enabled => (
            owo_colors::Style::new().yellow(),
            owo_colors::Style::new().dimmed(),
        ),
        Color::Disabled => (owo_colors::Style::new(), owo_colors::Style::new()),
    };

    let lines_above = changed_line - start;
    let lines_below = end - changed_line;

    let content = left_snippet
        .iter()
        .zip(start..end)
        .map(|(line, line_nr)| {
            let line = if line_nr == changed_line + 1 {
                line.style(added).to_string()
            } else {
                line.style(unchaged).to_string()
            };

            // Why are we adding "extras"?
            // The line may contain non-printable color codes which count for the padding
            // in format!(...) but don't add to the width on the terminal.
            // To accomodate, we pretend to make the padding wider again
            // because we know some of the width won't be visible.
            let extras = line.len() - ansi_width(&line);
            let width = usize::from(ctx.max_width);

            let line_nr = LineWidget(Some(line_nr - 1));
            format!("{line_nr}│ {line:<width$}", width = width + extras)
        })
        .collect::<Vec<_>>();

    Rendered {
        content,
        lines_above,
        lines_below,
    }
}

pub struct LineWidget(pub Option<usize>);

impl fmt::Display for LineWidget {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            Some(idx) => write!(f, "{:>3} ", idx + 1),
            None => write!(f, "    "),
        }
    }
}

fn node_in<'y>(yaml: &'y MarkedYamlOwned, path: &Path) -> Option<&'y MarkedYamlOwned> {
    let mut n = Some(yaml);
    for p in path.segments() {
        match p {
            crate::path::Segment::Field(f) => {
                let v = n.and_then(|n| n.get(f.as_str()))?;
                n = Some(v);
            }
            crate::path::Segment::Index(nr) => {
                let v = n.and_then(|n| n.get(*nr))?;
                n = Some(v);
            }
        }
    }
    n
}

fn surrounding_paths(parent_node: &MarkedYamlOwned, path: &Path) -> (Option<Path>, Option<Path>) {
    let parent_path = path.parent().unwrap();
    match &parent_node.data {
        YamlDataOwned::Sequence(children) => {
            let idx = path.head().and_then(|s| s.as_index()).unwrap();
            let left = if idx > 0 {
                Some(parent_path.push(idx - 1))
            } else {
                None
            };
            let right = if idx < children.len() {
                Some(parent_path.push(idx + 1))
            } else {
                None
            };
            (left, right)
        }
        YamlDataOwned::Mapping(children) => {
            // Consider extracting this...
            let target_key = path.head().and_then(|s| s.as_field()).unwrap();
            let keys: Vec<_> = children.keys().filter_map(|k| k.data.as_str()).collect();
            if let Some(idx) = keys.iter().position(|k| k == &target_key) {
                let before = if idx > 0 { Some(keys[idx - 1]) } else { None };
                let after = if idx < keys.len() - 1 {
                    Some(keys[idx + 1])
                } else {
                    None
                };
                (
                    before.map(|k| parent_path.push(k)),
                    after.map(|k| parent_path.push(k)),
                )
            } else {
                (None, None)
            }
        }
        _ => unreachable!("parent has to be a container"),
    }
}

#[cfg(test)]
mod test {
    use std::sync::Once;

    use expect_test::expect;
    use indoc::indoc;

    use crate::{
        YamlSource,
        diff::{ArrayOrdering, Context, Difference, diff},
        read_doc, render,
    };

    use super::{RenderContext, render_added, render_difference, render_removal};

    static LOGGING: Once = Once::new();

    fn init_logging() {
        LOGGING.call_once(|| {
            if std::env::var("LOG").is_ok() {
                env_logger::Builder::new()
                    .filter_level(log::LevelFilter::Debug)
                    .init();
            }
        });
    }

    fn ctx() -> RenderContext {
        RenderContext {
            max_width: 80,
            color: super::Color::Disabled,
            visual_context: 5,
        }
    }

    fn yaml_source(yaml: &'static str) -> YamlSource {
        let mut docs = read_doc(yaml, camino::Utf8PathBuf::new()).expect("to have parsed properly");
        docs.remove(0)
    }

    #[test]
    fn print_a_side_by_side_change() {
        let left_doc = yaml_source(indoc! {r#"
            person:
              name: Steve E. Anderson
              age: 12
        "#});

        let right_doc = yaml_source(indoc! {r#"
            person:
              name: Robert Anderson
              age: 12
        "#});

        let mut differences = diff(Context::default(), &left_doc.yaml, &right_doc.yaml);

        let first = differences.remove(0);
        let Difference::Changed { path, left, right } = first else {
            panic!("Should have gotten a Change");
        };
        let content = render_difference(&ctx(), path, left, &left_doc, right, &right_doc);

        expect![[r#"
            Changed: .person.name:
              1 │   name: Steve E. Anderson        │   1 │   name: Robert Anderson         
              2 │   age: 12                        │   2 │   age: 12                       "#]]
        .assert_eq(content.as_str());
    }

    #[test]
    fn display_the_removal_of_a_node() {
        let left_doc = yaml_source(indoc! {r#"
            ---
            person:
              name: Robert Anderson
              address:
                street: foo bar
                nr: 1
                postcode: ABC123
              age: 12
              foo: bar
        "#});

        // the entire `adress` section is gone!
        let right_doc = yaml_source(indoc! {r#"
            ---
            person:
              name: Robert Anderson
              age: 12
              foo: bar
        "#});

        let mut differences = diff(Context::default(), &left_doc.yaml, &right_doc.yaml);

        let first = differences.remove(0);
        let Difference::Removed { path, value } = first else {
            panic!("Should have gotten a Removal");
        };
        let content = render_removal(&ctx(), path, value, &left_doc, &right_doc);

        expect![[r#"
            │  1 │ person:                          │   1 │ person:                         
            │  2 │   name: Robert Anderson          │   2 │   name: Robert Anderson         
            │  3 │   address:                       │     │
            │  4 │     street: foo bar              │     │
            │  5 │     nr: 1                        │     │
            │  6 │     postcode: ABC123             │     │
            │  7 │   age: 12                        │   3 │   age: 12                       
            │  8 │   foo: bar                       │   4 │   foo: bar                      "#]]
        .assert_eq(content.as_str());
    }

    #[test]
    fn display_the_addition_of_a_node() {
        init_logging();
        let left_doc = yaml_source(indoc! {r#"
            ---
            person:
              name: Robert Anderson
              age: 12
              foo: bar
            "#});

        // the entire `adress` section is new!
        let right_doc = yaml_source(indoc! {r#"
            ---
            person:
              name: Robert Anderson
              address:
                street: foo bar
                nr: 1
                postcode: ABC123
              age: 12
              foo: bar
            "#});

        let mut differences = diff(Context::default(), &left_doc.yaml, &right_doc.yaml);

        let first = differences.remove(0);
        let Difference::Added { path, value } = first else {
            panic!("Should have gotten a Removal");
        };
        let content = render_added(&ctx(), path, value, &left_doc, &right_doc);

        expect![[r#"
            │  1 │ person:                          │   1 │ person:                         
            │  2 │   name: Robert Anderson          │   2 │   name: Robert Anderson         
            │    │                                  │   3 │   address:                      
            │    │                                  │   4 │     street: foo bar             
            │    │                                  │   5 │     nr: 1                       
            │    │                                  │   6 │     postcode: ABC123            
            │  3 │   age: 12                        │   7 │   age: 12                       
            │  4 │   foo: bar                       │   8 │   foo: bar                      "#]]
        .assert_eq(content.as_str());
    }

    #[test]
    fn display_addition_of_node_in_array() {
        init_logging();
        let left_doc = yaml_source(indoc! {r#"
            ---
            people:
              - name: Robert Anderson
                age: 30
              - name: Sarah Foo
                age: 31
        "#});

        // the entire `adress` section is new!
        let right_doc = yaml_source(indoc! {r#"
            ---
            people:
              - name: Robert Anderson
                age: 30
              - name: Adam Bar
                age: 32
              - name: Sarah Foo
                age: 31
        "#});

        let mut diff_ctx = Context::default();
        diff_ctx.array_ordering = ArrayOrdering::Dynamic;

        let mut differences = diff(diff_ctx, &left_doc.yaml, &right_doc.yaml);

        let first = differences.remove(0);
        let Difference::Added { path, value } = first else {
            panic!("Should have gotten an Addition");
        };
        let content = render_added(&ctx(), path, value, &left_doc, &right_doc);

        expect![[r#"
            │  1 │ people:                          │   1 │ people:                         
            │  2 │   - name: Robert Anderson        │   2 │   - name: Robert Anderson       
            │  3 │     age: 30                      │   3 │     age: 30                     
            │    │                                  │   4 │   - name: Adam Bar              
            │    │                                  │   5 │     age: 32                     
            │  4 │   - name: Sarah Foo              │   6 │   - name: Sarah Foo             
            │  5 │     age: 31                      │   7 │     age: 31                     "#]]
        .assert_eq(content.as_str());
    }

    #[test]
    fn show_a_change_and_an_additon_at_the_same_time() {
        init_logging();
        let left_doc = yaml_source(indoc! {r#"
            ---
            person:
              name: Steve E. Anderson
              age: 12
        "#});

        // the entire `adress` section is new!
        let right_doc = yaml_source(indoc! {r#"
            ---
            person:
              name: Steven Anderson
              location:
                street: 1 Kentish Street
                postcode: KS87JJ
              age: 34
        "#});

        let differences = diff(Context::default(), &left_doc.yaml, &right_doc.yaml);

        let content = render(
            RenderContext::new(80, super::Color::Disabled),
            &left_doc,
            &right_doc,
            differences,
            true,
        );

        expect![[r#"
            Changed: .person.name:
              1 │ person:                          │   1 │ person:                         
              2 │   name: Steve E. Anderson        │   2 │   name: Steven Anderson         
              3 │   age: 12                        │   3 │   location:                     
                                                   │   4 │     street: 1 Kentish Street    
                                                   │   5 │     postcode: KS87JJ            

            Changed: .person.age:
                                                   │   1 │ person:                         
                                                   │   2 │   name: Steven Anderson         
                                                   │   3 │   location:                     
              1 │ person:                          │   4 │     street: 1 Kentish Street    
              2 │   name: Steve E. Anderson        │   5 │     postcode: KS87JJ            
              3 │   age: 12                        │   6 │   age: 34                       

            Added: .person.location:
            │  1 │ person:                          │   1 │ person:                         
            │  2 │   name: Steve E. Anderson        │   2 │   name: Steven Anderson         
            │    │                                  │   3 │   location:                     
            │    │                                  │   4 │     street: 1 Kentish Street    
            │    │                                  │   5 │     postcode: KS87JJ            
            │  3 │   age: 12                        │   6 │   age: 34                       

        "#]]
        .assert_eq(content.as_str());
    }

    #[test]
    #[ignore = "refacotoring other things atm"]
    fn real_life_example() {
        init_logging();
        let left_doc = yaml_source(indoc! {r#"
            ---
            apiVersion: v1
            kind: Service
            metadata:
              name: flux-engine-steam
              namespace: classification
              labels:
                helm.sh/chart: flux-engine-steam-2.28.12
                app.kubernetes.io/name: flux-engine-steam
                app: flux-engine-steam
                app.kubernetes.io/version: 0.0.27-pre1
                app.kubernetes.io/managed-by: batman
              annotations:
                github.com/repository_url: git@github.com:flux-engine-steam
            spec:
              ports:
                - targetPort: 8501
                  port: 3000
                  name: https
              selector:
                app: flux-engine-steam
        "#});

        let right_doc = yaml_source(indoc! {r#"
            ---
            apiVersion: v1
            kind: Service
            metadata:
              name: flux-engine-steam
              namespace: classification
              labels:
                helm.sh/chart: flux-engine-steam-2.28.12
                app.kubernetes.io/name: flux-engine-steam
                app: flux-engine-steam
                app.kubernetes.io/version: 0.0.27-pre1
                app.kubernetes.io/managed-by: batman
              annotations:
                github.com/repository_url: git@github.com:flux-engine-steam
                this_is: new
            spec:
              ports:
                - targetPort: 8502
                  port: 3000
                  name: https
              selector:
                app: flux-engine-steam
            "#});

        let differences = diff(Context::default(), &left_doc.yaml, &right_doc.yaml);

        let content = render(
            RenderContext::new(150, super::Color::Disabled),
            &left_doc,
            &right_doc,
            differences,
            true,
        );

        expect![[r#"
            Added: .metadata.annotations.this_is:
            │  9 │     app: flux-engine-steam                                          │   9 │     app: flux-engine-steam                                         
            │ 10 │     app.kubernetes.io/version: 0.0.27-pre1                          │  10 │     app.kubernetes.io/version: 0.0.27-pre1                         
            │ 11 │     app.kubernetes.io/managed-by: batman                            │  11 │     app.kubernetes.io/managed-by: batman                           
            │ 12 │   annotations:                                                      │  12 │   annotations:                                                     
            │ 13 │     github.com/repository_url: git@github.com:flux-engine-steam     │  13 │     github.com/repository_url: git@github.com:flux-engine-steam    
            │    │                                                                     │  14 │     this_is: new                                                   
            │ 14 │ spec:                                                               │  15 │ spec:                                                              
            │ 15 │   ports:                                                            │  16 │   ports:                                                           
            │ 16 │     - targetPort: 8501                                              │  17 │     - targetPort: 8502                                             
            │ 17 │       port: 3000                                                    │  18 │       port: 3000                                                   
            │ 18 │       name: https                                                   │  19 │       name: https                                                  

            Changed: .spec.ports[0].targetPort:
             11 │     app.kubernetes.io/managed-by: batman                            │  12 │   annotations:                                                     
             12 │   annotations:                                                      │  13 │     github.com/repository_url: git@github.com:flux-engine-steam    
             13 │     github.com/repository_url: git@github.com:flux-engine-steam     │  14 │     this_is: new                                                   
             14 │ spec:                                                               │  15 │ spec:                                                              
             15 │   ports:                                                            │  16 │   ports:                                                           
             16 │     - targetPort: 8501                                              │  17 │     - targetPort: 8502                                             
             17 │       port: 3000                                                    │  18 │       port: 3000                                                   
             18 │       name: https                                                   │  19 │       name: https                                                  
             19 │   selector:                                                         │  20 │   selector:                                                        

        "#]].assert_eq(content.as_str());
    }
}
