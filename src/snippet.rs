use core::option::Option::None;
use std::{
    cmp::min,
    fmt::{self},
    iter::{empty, repeat_n},
    num::NonZeroUsize,
    ops::{Add, Sub},
};

use ansi_width::ansi_width;
use either::Either;
use owo_colors::{OwoColorize, Style};
use saphyr::{MarkedYamlOwned, YamlDataOwned};

use crate::{YamlSource, diff::Item, node::node_in, path::Path};

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

    pub fn distance(&self, other: &Line) -> usize {
        let a = self.get();
        let b = other.get();

        a.abs_diff(b)
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
        log::debug!("Creating a new snippet");
        log::debug!("---from: {from} to {to}");
        log::debug!("{:#?}", lines);
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

    /// Creates a snippet that will safely clamp the `to` value
    /// to not exceed the number of `lines`
    pub fn new_clamped<'source>(
        lines: &'source [&'source str],
        from: Line,
        to: Line,
    ) -> Snippet<'source> {
        if to <= from {
            panic!("'to' ({to}) was less than 'from' ({from})");
        }
        let to = min(Line::new(lines.len()).unwrap(), to);
        Snippet { lines, from, to }
    }

    pub fn iter<'s>(&'s self) -> SnippetLineIter<'s> {
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
    removal: Item,
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
    addition: Item,
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

#[derive(Copy, Clone)]
enum ChangeType {
    Removal,
    Addition,
}

fn render_change(
    ctx: &RenderContext,
    path_to_change: Path,
    changed_yaml: Item,
    left_doc: &YamlSource,
    right_doc: &YamlSource,
    change_type: ChangeType,
) -> String {
    log::debug!("Rendering change for {}", path_to_change.jq_like());
    log::debug!("The changed yaml node looks like: {:#?}", changed_yaml);

    // Select primary and secondary documents based on change type
    // The `larger_document` has more content and the changed_yaml will be highlighted.
    // The `gapped_document` has the gap in it
    let (larger_document, gapped_document) = match change_type {
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

    let primary = render_primary_side(ctx, larger_document, &changed_yaml, colors);
    let gap_size = changed_yaml.height();
    let secondary = render_secondary_side(
        ctx,
        larger_document,
        gapped_document,
        path_to_change,
        primary.len(),
        gap_size,
        colors.1,
    );

    // wtf is this +6
    let width = ctx.half_width() + 6;

    let fixed_with_line = |(left, right)| format!("│ {left:<width$}│ {right:<width$}");

    log::debug!(
        "Sizes:  primary {}, secondary {}",
        primary.len(),
        secondary.len()
    );

    // Combine the two sides based on change type
    match change_type {
        ChangeType::Removal => primary
            .iter()
            .zip(secondary)
            .map(fixed_with_line)
            .collect::<Vec<_>>()
            .join("\n"),
        ChangeType::Addition => secondary
            .iter()
            .zip(primary)
            .map(fixed_with_line)
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn render_primary_side(
    ctx: &RenderContext,
    primary_doc: &YamlSource,
    item: &Item,
    (highlighting, unchanged): (Style, Style),
) -> Vec<String> {
    // Extract lines from primary document
    let primary_lines = primary_doc.lines();

    let (change_start, change_end) = match item {
        Item::KV { key, value } => (
            primary_doc.relative_line(key.span.start.line()),
            primary_doc.relative_line(value.span.end.line()),
        ),
        Item::ArrayElement { value, .. } => (
            primary_doc.relative_line(value.span.start.line()),
            primary_doc.relative_line(value.span.end.line()),
        ),
    };

    // Show a few more lines before and after the lines that have changed
    let start = change_start - ctx.visual_context;
    let end = min(change_end + ctx.visual_context, primary_doc.last_line);
    log::debug!("Snippet for primary document");
    let primary_snippet =
        Snippet::try_new(&primary_lines, start, end).expect("Primary snippet could not be created");

    // Format the primary side
    let mut changed_range = change_start..change_end;
    if changed_range.is_empty() {
        // We need to at least highlight 1 line!
        changed_range = change_start..(change_end + 1);
    }
    log::debug!("We will highlight {change_start}..={change_end}");
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

fn render_secondary_side(
    ctx: &RenderContext,
    primary_doc: &YamlSource,
    secondary_doc: &YamlSource,
    path_to_changed_node: Path,
    primary_snippet_size: usize,
    gap_size: usize,
    unchanged: Style,
) -> Vec<String> {
    log::debug!("changed_node: {}", path_to_changed_node.jq_like());
    // TODO: this might not be 100% intended as it gives the value, meaning the right hand side...
    // let node_to_align = node_in(&secondary_doc.yaml, &path_to_changed_node)
    //     .expect("node to align was not in secondary_doc");

    let gap_start = gap_start(primary_doc, secondary_doc, path_to_changed_node);
    log::debug!("The gap should be right after: {gap_start}");
    // The gap comes after gap_start, so we need to start at gap_start + 1
    // to align with the primary side which starts at the changed content.
    // This applies to both additions and removals.
    let start = (gap_start + 1) - ctx.visual_context;
    let end: Line = gap_start + ctx.visual_context + 1;

    let lines = secondary_doc.lines();

    let s = Snippet::new_clamped(&lines, start, end);
    log::debug!("Secondary snippet len: {}", s.lines.len());
    log::debug!("{:?}", &s.lines);
    let (before_gap, after_gap) = s.split(gap_start);
    log::debug!("after split:");
    log::debug!("before_gap: {}->{}", before_gap.from, before_gap.to);
    log::debug!("after_gap: {}->{}", after_gap.from, after_gap.to);

    let filler_len = if end.distance(&start) > primary_snippet_size {
        0
    } else {
        (end.distance(&start)).saturating_sub(primary_snippet_size)
    };
    log::debug!("Filler will be {filler_len}");

    let filler = repeat_n("".to_string(), filler_len);

    let pre_gap = before_gap.iter().map(|(line_nr, line)| {
        let line = line.style(unchanged).to_string();
        let extras = line.len() - ansi_width(&line);

        let line_nr = LineWidget::from(line_nr);
        format!(
            "{line_nr}│ {line:<width$}",
            width = ctx.half_width() + extras
        )
    });

    let gap = (0..gap_size).map(|_| {
        let l = LineWidget(None);
        format!("{l}│ {line:<width$}", line = "", width = ctx.half_width())
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

    filler.chain(pre_gap).chain(gap).chain(post_gap).collect()
}

/// Adjusts a path from primary document indexing to secondary document indexing.
/// For sequences (arrays), when an element is added, the indices shift.
/// e.g., if we added at index 0, then primary's index 1 corresponds to secondary's index 0.
fn adjust_path_for_secondary(path: &Path, parent_data: &YamlDataOwned<MarkedYamlOwned>) -> Path {
    match parent_data {
        YamlDataOwned::Sequence(_) => {
            // For sequences, decrement the last index by 1
            let segments = path.segments();
            if let Some((last, rest)) = segments.split_last()
                && let Some(idx) = last.as_index()
                && idx > 0
            {
                let mut new_path = Path::default();
                for seg in rest {
                    new_path = new_path.push(seg.clone());
                }
                new_path = new_path.push(idx - 1);
                return new_path;
            }
            path.clone()
        }
        _ => path.clone(),
    }
}

/// Find corresponding nodes in secondary document
/// I think this is more complex than it initially seems.
/// The goal is to get the spans of the nodes that need to surround the gap.
/// Therefor I need know what nodes should be there, and then translate
/// that into the other document. I tend to do that via the `path`
///
/// BUT(!) paths don't necessarily carry over to the other docment.
/// e.g. if the path to the change is `.people.3`
/// the surround nodes could be (.people.2, .people.4)
/// but who knows if the array has sufficient elements?!
pub fn gap_start(
    primary_doc: &YamlSource,
    secondary_doc: &YamlSource,
    path_to_change: Path,
) -> Line {
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

    // TODO: I think this needs something similar to what I did with Item::KV and Item::ArrayElement
    // where we are able to retrieve the proper bounding box of the node, not just its value.
    let candidate_node_before_change = before_path.and_then(|p| node_in(&secondary_doc.yaml, &p));

    if let Some(before) = candidate_node_before_change {
        // Normal case: there's a node before the change, use its end line.
        // For complex nodes (mappings/sequences), span.end.line() is exclusive
        // (points to line after content), so we subtract 1.
        // For scalars, span.end.line() equals span.start.line() (inclusive).
        let adjustment = match &before.data {
            YamlDataOwned::Sequence(_) | YamlDataOwned::Mapping(_) => 1,
            _ => 0,
        };
        log::debug!("before node adjustment factor: {adjustment}");
        log::debug!("the span ends on {}", before.span.end.line());
        secondary_doc.relative_line(before.span.end.line() - adjustment)
    } else if let Some(after) = after_path {
        // No "before" node (e.g., adding at index 0 of an array).
        // Use the "after" node to find where the gap should go.
        // For sequences, the after_path index needs to be decremented by 1
        // because secondary doesn't have the new element.
        let adjusted_path = adjust_path_for_secondary(&after, &primary_parent_node.data);
        log::debug!(
            "Adjusted after_path for secondary: {:?}",
            adjusted_path.jq_like()
        );

        if let Some(after_node) = node_in(&secondary_doc.yaml, &adjusted_path) {
            // Gap should appear just before this element
            let start_line = after_node.span.start.line();
            log::debug!(
                "After node starts at line {}, gap_start will be {}",
                start_line,
                start_line - 1
            );
            secondary_doc.relative_line(start_line - 1)
        } else {
            // Fallback: use parent node's start
            log::debug!("Could not find after node in secondary, falling back to parent");
            let secondary_parent = node_in(&secondary_doc.yaml, &parent);
            secondary_parent
                .map(|p| secondary_doc.relative_line(p.span.start.line()))
                .unwrap_or(Line::one())
        }
    } else {
        // No before or after path, fall back to line 1
        log::debug!("No before or after path, falling back to Line::one()");
        Line::one()
    }
}

#[cfg(test)]
mod test_node_height {
    use indoc::indoc;
    use saphyr::{LoadableYamlNode, MarkedYamlOwned, SafelyIndex};

    use crate::diff::Item;

    #[test]
    fn height_of_simple_string() {
        let raw = indoc! {r#"
          element: "Hi there"
        "#};

        let mut yaml = MarkedYamlOwned::load_from_str(raw).unwrap();
        let yaml = yaml.remove(0);
        let (key, value) = yaml.data.as_mapping().unwrap().into_iter().next().unwrap();
        let item = Item::KV {
            key: (*key).clone(),
            value: (*value).clone(),
        };

        assert_eq!(1, item.height());
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
        let (key, value) = yaml.data.as_mapping().unwrap().into_iter().next().unwrap();
        let item = Item::KV {
            key: (*key).clone(),
            value: (*value).clone(),
        };

        assert_eq!(4, item.height());
    }

    #[test]
    fn height_of_boolean() {
        let raw = indoc! {r#"
          element: true
        "#};

        let mut yaml = MarkedYamlOwned::load_from_str(raw).unwrap();
        let yaml = yaml.remove(0);
        let (key, value) = yaml.data.as_mapping().unwrap().into_iter().next().unwrap();
        let item = Item::KV {
            key: (*key).clone(),
            value: (*value).clone(),
        };

        assert_eq!(1, item.height());
    }

    #[test]
    fn height_of_integer() {
        let raw = indoc! {r#"
          element: 7
        "#};

        let mut yaml = MarkedYamlOwned::load_from_str(raw).unwrap();
        let yaml = yaml.remove(0);
        let (key, value) = yaml.data.as_mapping().unwrap().into_iter().next().unwrap();
        let item = Item::KV {
            key: (*key).clone(),
            value: (*value).clone(),
        };

        assert_eq!(1, item.height());
    }

    #[test]
    fn height_of_null() {
        let raw = indoc! {r#"
          element: ~
        "#};

        let mut yaml = MarkedYamlOwned::load_from_str(raw).unwrap();
        let yaml = yaml.remove(0);
        let (key, value) = yaml.data.as_mapping().unwrap().into_iter().next().unwrap();
        let item = Item::KV {
            key: (*key).clone(),
            value: (*value).clone(),
        };

        assert_eq!(1, item.height());
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
        let (key, value) = yaml.data.as_mapping().unwrap().into_iter().next().unwrap();
        let item = Item::KV {
            key: (*key).clone(),
            value: (*value).clone(),
        };

        assert_eq!(5, item.height());
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
        let value = yaml.get("thing").and_then(|thing| thing.get(1)).unwrap();
        let item = Item::ArrayElement {
            index: 1,
            value: (*value).clone(),
        };

        assert_eq!(2, item.height());
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
        let (key, value) = yaml.data.as_mapping().unwrap().into_iter().next().unwrap();
        let item = Item::KV {
            key: (*key).clone(),
            value: (*value).clone(),
        };

        assert_eq!(5, item.height());
    }
}

#[cfg(test)]
mod test_gap_start {
    use test_log::test;

    use crate::{path::Path, read_doc, snippet::Line};

    use super::gap_start;

    #[test]
    pub fn clean_split_down_the_middle() {
        let primary = indoc::indoc! {r#"
            ---
            person:
              name: Steve E. Anderson
              location:
                street: 1 Kentish Street
                postcode: KS87JJ
              age: 12
            "#};

        let primary = read_doc(primary, camino::Utf8PathBuf::default())
            .unwrap()
            .remove(0);

        let secondary = indoc::indoc! {r#"
            ---
            person:
              name: Steve E. Anderson
              age: 12
            "#};
        let secondary = read_doc(secondary, camino::Utf8PathBuf::default())
            .unwrap()
            .remove(0);

        let location = Path::parse_str(".person.location");

        let actual_start = gap_start(&primary, &secondary, location);

        // The split we are looking for is
        // [1] person:
        // [2]   name: Steve E. Anderson
        // <--- the gap --->
        // [3]   age: 12
        assert_eq!(actual_start, Line::unchecked(2));
    }

    #[test]
    pub fn example() {
        let primary = indoc::indoc! {r#"
            ---
            apiVersion: v1
            kind: Service
            metadata:
              name: flux-engine-steam
              namespace: classification
              labels:
                app.kubernetes.io/managed-by: batman
              annotations:
                github.com/repository_url: git@github.com:flux-engine-steam
                this_is: new
            spec:
              ports:
                - port: 3000
            "#};

        let primary = read_doc(primary, camino::Utf8PathBuf::default())
            .unwrap()
            .remove(0);

        let secondary = indoc::indoc! {r#"
            ---
            apiVersion: v1
            kind: Service
            metadata:
              name: flux-engine-steam
              namespace: classification
              labels:
                app.kubernetes.io/managed-by: batman
              annotations:
                github.com/repository_url: git@github.com:flux-engine-steam
            spec:
              ports:
                - port: 3000
            "#};
        let secondary = read_doc(secondary, camino::Utf8PathBuf::default())
            .unwrap()
            .remove(0);

        let location = Path::parse_str(".metadata.annotations.this_is");

        let actual_start = gap_start(&primary, &secondary, location);

        // The split we are looking for is
        // [1] person:
        // [2]   name: Steve E. Anderson
        // <--- the gap --->
        // [3]   age: 12
        assert_eq!(actual_start, Line::new(9).unwrap());
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
            Either::Left(filler().take(above_filler)),
            Either::Right(empty::<String>()),
        )
    } else {
        (
            Either::Right(empty::<String>()),
            Either::Left(filler().take(above_filler)),
        )
    };

    let (left_bottom_filler, right_bottom_filler) = if left.lines_below < right.lines_below {
        (
            Either::Left(filler().take(below_filler)),
            Either::Right(empty::<String>()),
        )
    } else {
        (
            Either::Right(empty::<String>()),
            Either::Left(filler().take(below_filler)),
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

    let width = ctx.half_width() + 6;

    let fixed_with_line = |(left, right)| format!("│ {left:<width$}│ {right:<width$}");

    let body = left
        .zip(right)
        .map(fixed_with_line)
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
    let start = changed_line.saturating_sub(context);
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
            let line = if line_nr == changed_line {
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

            let line_nr = LineWidget(Some(line_nr));
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

// TODO: remove the `after node`
fn surrounding_paths(parent_node: &MarkedYamlOwned, path: &Path) -> (Option<Path>, Option<Path>) {
    let parent_path = path.parent().unwrap();
    log::trace!("the parent is: {}", parent_path.jq_like());
    log::trace!("the parent node is: {:#?}", parent_node);
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
            log::debug!("looking for: {target_key}");
            let keys: Vec<_> = children.keys().filter_map(|k| k.data.as_str()).collect();

            log::debug!("possible children keys: {:?}", keys);
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
    use test_log::test;

    use expect_test::expect;
    use indoc::indoc;

    use crate::{
        YamlSource,
        diff::{ArrayOrdering, Context, Difference, diff},
        read_doc, render,
    };

    use super::{RenderContext, render_added, render_difference, render_removal};

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
            │   1 │ person:                         │   1 │ person:                         
            │   2 │   name: Steve E. Anderson       │   2 │   name: Robert Anderson         
            │   3 │   age: 12                       │   3 │   age: 12                       "#]]
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

        let differences = diff(Context::default(), &left_doc.yaml, &right_doc.yaml);

        let content = render(ctx(), &left_doc, &right_doc, differences, true);

        expect![[r#"
            Removed: .person.address:
            │   1 │ person:                         │   1 │ person:                         
            │   2 │   name: Robert Anderson         │   2 │   name: Robert Anderson         
            │   3 │   address:                      │     │                                 
            │   4 │     street: foo bar             │     │                                 
            │   5 │     nr: 1                       │     │                                 
            │   6 │     postcode: ABC123            │     │                                 
            │   7 │   age: 12                       │   3 │   age: 12                       
            │   8 │   foo: bar                      │   4 │   foo: bar                      

        "#]]
        .assert_eq(content.as_str());
    }

    #[test]
    fn display_the_addition_of_a_node() {
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

        let differences = diff(Context::default(), &left_doc.yaml, &right_doc.yaml);

        let content = render(ctx(), &left_doc, &right_doc, differences, true);

        expect![[r#"
            Added: .person.address:
            │   1 │ person:                         │   1 │ person:                         
            │   2 │   name: Robert Anderson         │   2 │   name: Robert Anderson         
            │     │                                 │   3 │   address:                      
            │     │                                 │   4 │     street: foo bar             
            │     │                                 │   5 │     nr: 1                       
            │     │                                 │   6 │     postcode: ABC123            
            │   3 │   age: 12                       │   7 │   age: 12                       
            │   4 │   foo: bar                      │   8 │   foo: bar                      

        "#]]
        .assert_eq(content.as_str());
    }

    #[test]
    fn display_addition_of_node_in_array() {
        let left_doc = yaml_source(indoc! {r#"
            ---
            people:
              - name: Robert Anderson
                age: 20
              - name: Sarah Foo
                age: 31
        "#});

        // the entire `adress` section is new!
        let right_doc = yaml_source(indoc! {r#"
            ---
            people:
              - name: Robert Anderson
                age: 20
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
            │   1 │ people:                         │   1 │ people:                         
            │   2 │   - name: Robert Anderson       │   2 │   - name: Robert Anderson       
            │   3 │     age: 20                     │   3 │     age: 20                     
            │     │                                 │   4 │   - name: Adam Bar              
            │     │                                 │   5 │     age: 32                     
            │   4 │   - name: Sarah Foo             │   6 │   - name: Sarah Foo             
            │   5 │     age: 31                     │   7 │     age: 31                     "#]]
        .assert_eq(content.as_str());
    }

    #[test]
    fn display_addition_at_start_of_array() {
        let left_doc = yaml_source(indoc! {r#"
            ---
            people:
              - name: Robert Anderson
                age: 20
              - name: Sarah Foo
                age: 31
        "#});

        // A new person is added at the START of the array
        let right_doc = yaml_source(indoc! {r#"
            ---
            people:
              - name: New First Person
                age: 25
              - name: Robert Anderson
                age: 20
              - name: Sarah Foo
                age: 31
        "#});

        let mut diff_ctx = Context::default();
        diff_ctx.array_ordering = ArrayOrdering::Dynamic;

        let mut differences = diff(diff_ctx, &left_doc.yaml, &right_doc.yaml);

        let first = differences.remove(0);
        let Difference::Added { path, value } = first else {
            panic!("Should have gotten an Addition, got: {:?}", first);
        };
        let content = render_added(&ctx(), path, value, &left_doc, &right_doc);

        // The gap on the left should align with the new element on the right
        // Both sides should show the `people:` array context
        expect![[r#"
            │   1 │ people:                         │   1 │ people:                         
            │     │                                 │   2 │   - name: New First Person      
            │     │                                 │   3 │     age: 25                     
            │   2 │   - name: Robert Anderson       │   4 │   - name: Robert Anderson       
            │   3 │     age: 20                     │   5 │     age: 20                     
            │   4 │   - name: Sarah Foo             │   6 │   - name: Sarah Foo             
            │   5 │     age: 31                     │   7 │     age: 31                     "#]]
        .assert_eq(content.as_str());
    }

    #[test]
    fn display_addition_at_start_of_deeply_nested_array() {
        // This test reproduces the bug where adding at index 0 of a deeply nested
        // array causes gap_start to fall back to Line::one(), showing the wrong
        // part of the document on the left side.
        let left_doc = yaml_source(indoc! {r#"
            ---
            apiVersion: apps/v1
            kind: Deployment
            metadata:
              name: my-app
            spec:
              template:
                spec:
                  containers:
                  - name: app
                    env:
                    - name: EXISTING_VAR
                      value: "existing"
        "#});

        let right_doc = yaml_source(indoc! {r#"
            ---
            apiVersion: apps/v1
            kind: Deployment
            metadata:
              name: my-app
            spec:
              template:
                spec:
                  containers:
                  - name: app
                    env:
                    - name: NEW_FIRST_VAR
                      value: "new"
                    - name: EXISTING_VAR
                      value: "existing"
        "#});

        let mut diff_ctx = Context::default();
        diff_ctx.array_ordering = ArrayOrdering::Dynamic;

        let mut differences = diff(diff_ctx, &left_doc.yaml, &right_doc.yaml);

        let first = differences.remove(0);
        let Difference::Added { path, value } = first else {
            panic!("Should have gotten an Addition, got: {:?}", first);
        };

        // Verify the path is what we expect
        assert_eq!(path.jq_like(), ".spec.template.spec.containers[0].env[0]");

        let content = render_added(&ctx(), path, value, &left_doc, &right_doc);

        // The left side should show the area around the `env:` array,
        // NOT the beginning of the file (line 1)
        expect![[r#"
            │   6 │   template:                     │   6 │   template:                     
            │   7 │     spec:                       │   7 │     spec:                       
            │   8 │       containers:               │   8 │       containers:               
            │   9 │       - name: app               │   9 │       - name: app               
            │  10 │         env:                    │  10 │         env:                    
            │     │                                 │  11 │         - name: NEW_FIRST_VAR   
            │     │                                 │  12 │           value: "new"          
            │  11 │         - name: EXISTING_VAR    │  13 │         - name: EXISTING_VAR    
            │  12 │           value: "existing"     │  14 │           value: "existing"     "#]]
        .assert_eq(content.as_str());
    }

    #[test]
    fn show_a_change_and_an_additon_at_the_same_time() {
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
            │   1 │ person:                         │   1 │ person:                         
            │   2 │   name: Steve E. Anderson       │   2 │   name: Steven Anderson         
            │   3 │   age: 12                       │   3 │   location:                     
            │                                       │   4 │     street: 1 Kentish Street    
            │                                       │   5 │     postcode: KS87JJ            
            │                                       │   6 │   age: 34                       

            Changed: .person.age:
            │                                       │   1 │ person:                         
            │                                       │   2 │   name: Steven Anderson         
            │                                       │   3 │   location:                     
            │   1 │ person:                         │   4 │     street: 1 Kentish Street    
            │   2 │   name: Steve E. Anderson       │   5 │     postcode: KS87JJ            
            │   3 │   age: 12                       │   6 │   age: 34                       

            Added: .person.location:
            │   1 │ person:                         │   1 │ person:                         
            │   2 │   name: Steve E. Anderson       │   2 │   name: Steven Anderson         
            │     │                                 │   3 │   location:                     
            │     │                                 │   4 │     street: 1 Kentish Street    
            │     │                                 │   5 │     postcode: KS87JJ            
            │   3 │   age: 12                       │   6 │   age: 34                       

        "#]]
        .assert_eq(content.as_str());
    }

    #[test]
    fn real_life_example() {
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
            │   9 │     app: flux-engine-steam                                         │   9 │     app: flux-engine-steam                                         
            │  10 │     app.kubernetes.io/version: 0.0.27-pre1                         │  10 │     app.kubernetes.io/version: 0.0.27-pre1                         
            │  11 │     app.kubernetes.io/managed-by: batman                           │  11 │     app.kubernetes.io/managed-by: batman                           
            │  12 │   annotations:                                                     │  12 │   annotations:                                                     
            │  13 │     github.com/repository_url: git@github.com:flux-engine-steam    │  13 │     github.com/repository_url: git@github.com:flux-engine-steam    
            │     │                                                                    │  14 │     this_is: new                                                   
            │  14 │ spec:                                                              │  15 │ spec:                                                              
            │  15 │   ports:                                                           │  16 │   ports:                                                           
            │  16 │     - targetPort: 8501                                             │  17 │     - targetPort: 8502                                             
            │  17 │       port: 3000                                                   │  18 │       port: 3000                                                   
            │  18 │       name: https                                                  │  19 │       name: https                                                  

            Changed: .spec.ports[0].targetPort:
            │  11 │     app.kubernetes.io/managed-by: batman                           │  12 │   annotations:                                                     
            │  12 │   annotations:                                                     │  13 │     github.com/repository_url: git@github.com:flux-engine-steam    
            │  13 │     github.com/repository_url: git@github.com:flux-engine-steam    │  14 │     this_is: new                                                   
            │  14 │ spec:                                                              │  15 │ spec:                                                              
            │  15 │   ports:                                                           │  16 │   ports:                                                           
            │  16 │     - targetPort: 8501                                             │  17 │     - targetPort: 8502                                             
            │  17 │       port: 3000                                                   │  18 │       port: 3000                                                   
            │  18 │       name: https                                                  │  19 │       name: https                                                  
            │  19 │   selector:                                                        │  20 │   selector:                                                        
            │  20 │     app: flux-engine-steam                                         │  21 │     app: flux-engine-steam                                         

        "#]].assert_eq(content.as_str());
    }

    #[test]
    fn display_removal_from_middle_of_array() {
        // Removal of an element from the middle of an array
        let left_doc = yaml_source(indoc! {r#"
            ---
            people:
              - name: Alice
                age: 25
              - name: Bob
                age: 30
              - name: Charlie
                age: 35
        "#});

        let right_doc = yaml_source(indoc! {r#"
            ---
            people:
              - name: Alice
                age: 25
              - name: Charlie
                age: 35
        "#});

        let mut diff_ctx = Context::default();
        diff_ctx.array_ordering = ArrayOrdering::Dynamic;

        let mut differences = diff(diff_ctx, &left_doc.yaml, &right_doc.yaml);

        let first = differences.remove(0);
        let Difference::Removed { path, value } = first else {
            panic!("Should have gotten a Removal, got: {:?}", first);
        };
        let content = render_removal(&ctx(), path, value, &left_doc, &right_doc);

        expect![[r#"
            │   1 │ people:                         │   1 │ people:                         
            │   2 │   - name: Alice                 │   2 │   - name: Alice                 
            │   3 │     age: 25                     │   3 │     age: 25                     
            │   4 │   - name: Bob                   │   4 │   - name: Charlie               
            │   5 │     age: 30                     │   5 │     age: 35                     
            │   6 │   - name: Charlie               │     │                                 
            │   7 │     age: 35                     │     │                                 "#]]
        .assert_eq(content.as_str());
    }

    #[test]
    fn display_removal_at_start_of_array() {
        // Removal of the first element of an array (index 0)
        let left_doc = yaml_source(indoc! {r#"
            ---
            people:
              - name: First Person
                age: 20
              - name: Second Person
                age: 30
              - name: Third Person
                age: 40
        "#});

        let right_doc = yaml_source(indoc! {r#"
            ---
            people:
              - name: Second Person
                age: 30
              - name: Third Person
                age: 40
        "#});

        let mut diff_ctx = Context::default();
        diff_ctx.array_ordering = ArrayOrdering::Dynamic;

        let mut differences = diff(diff_ctx, &left_doc.yaml, &right_doc.yaml);

        let first = differences.remove(0);
        let Difference::Removed { path, value } = first else {
            panic!("Should have gotten a Removal, got: {:?}", first);
        };

        let content = render_removal(&ctx(), path, value, &left_doc, &right_doc);

        expect![[r#"
            │   1 │ people:                         │   1 │ people:                         
            │   2 │   - name: First Person          │   2 │   - name: Second Person         
            │   3 │     age: 20                     │   3 │     age: 30                     
            │   4 │   - name: Second Person         │   4 │   - name: Third Person          
            │   5 │     age: 30                     │   5 │     age: 40                     
            │   6 │   - name: Third Person          │     │                                 
            │   7 │     age: 40                     │     │                                 "#]]
        .assert_eq(content.as_str());
    }

    #[test]
    fn display_removal_where_before_node_is_complex_mapping() {
        // This tests the fix where "before" node is a complex mapping (not a scalar)
        // and span.end.line() needs adjustment
        let left_doc = yaml_source(indoc! {r#"
            ---
            metadata:
              name: my-service
              labels:
                app: my-app
                version: "1.0"
                environment: production
              annotations:
                description: "My service description"
            spec:
              replicas: 3
        "#});

        let right_doc = yaml_source(indoc! {r#"
            ---
            metadata:
              name: my-service
              labels:
                app: my-app
                version: "1.0"
                environment: production
            spec:
              replicas: 3
        "#});

        let differences = diff(Context::default(), &left_doc.yaml, &right_doc.yaml);

        let content = render(ctx(), &left_doc, &right_doc, differences, true);

        // The gap on the right should align correctly with the removed annotations
        // Both sides should start at the same line number
        expect![[r#"
            Removed: .metadata.annotations:
            │   2 │   name: my-service              │   2 │   name: my-service              
            │   3 │   labels:                       │   3 │   labels:                       
            │   4 │     app: my-app                 │   4 │     app: my-app                 
            │   5 │     version: "1.0"              │   5 │     version: "1.0"              
            │   6 │     environment: production     │   6 │     environment: production     
            │   7 │   annotations:                  │     │                                 
            │   8 │     description: "My service description"│     │                                 
            │   9 │ spec:                           │   7 │ spec:                           
            │  10 │   replicas: 3                   │   8 │   replicas: 3                   

        "#]]
        .assert_eq(content.as_str());
    }

    #[test]
    fn display_change_within_array_element() {
        // A scalar change inside an array element
        let left_doc = yaml_source(indoc! {r#"
            ---
            servers:
              - host: server1.example.com
                port: 8080
              - host: server2.example.com
                port: 9090
        "#});

        let right_doc = yaml_source(indoc! {r#"
            ---
            servers:
              - host: server1.example.com
                port: 8080
              - host: server2.example.com
                port: 9091
        "#});

        let mut diff_ctx = Context::default();
        diff_ctx.array_ordering = ArrayOrdering::Dynamic;

        let differences = diff(diff_ctx, &left_doc.yaml, &right_doc.yaml);

        let content = render(ctx(), &left_doc, &right_doc, differences, true);

        expect![[r#"
            Changed: .servers[1].port:
            │   1 │ servers:                        │   1 │ servers:                        
            │   2 │   - host: server1.example.com   │   2 │   - host: server1.example.com   
            │   3 │     port: 8080                  │   3 │     port: 8080                  
            │   4 │   - host: server2.example.com   │   4 │   - host: server2.example.com   
            │   5 │     port: 9090                  │   5 │     port: 9091                  

        "#]]
        .assert_eq(content.as_str());
    }

    #[test]
    fn display_removal_of_last_key_in_mapping() {
        // Removal of the last key in a mapping
        let left_doc = yaml_source(indoc! {r#"
            ---
            config:
              database:
                host: localhost
                port: 5432
              cache:
                enabled: true
                ttl: 3600
        "#});

        let right_doc = yaml_source(indoc! {r#"
            ---
            config:
              database:
                host: localhost
                port: 5432
        "#});

        let differences = diff(Context::default(), &left_doc.yaml, &right_doc.yaml);

        let content = render(ctx(), &left_doc, &right_doc, differences, true);

        expect![[r#"
            Removed: .config.cache:
            │   1 │ config:                         │   1 │ config:                         
            │   2 │   database:                     │   2 │   database:                     
            │   3 │     host: localhost             │   3 │     host: localhost             
            │   4 │     port: 5432                  │   4 │     port: 5432                  
            │   5 │   cache:                        │     │                                 
            │   6 │     enabled: true               │     │                                 
            │   7 │     ttl: 3600                   │     │                                 

        "#]]
        .assert_eq(content.as_str());
    }

    #[test]
    fn display_addition_of_last_key_in_mapping() {
        // Addition at the end of a mapping (no "after" node)
        let left_doc = yaml_source(indoc! {r#"
            ---
            config:
              database:
                host: localhost
                port: 5432
        "#});

        let right_doc = yaml_source(indoc! {r#"
            ---
            config:
              database:
                host: localhost
                port: 5432
              cache:
                enabled: true
                ttl: 3600
        "#});

        let differences = diff(Context::default(), &left_doc.yaml, &right_doc.yaml);

        let content = render(ctx(), &left_doc, &right_doc, differences, true);

        expect![[r#"
            Added: .config.cache:
            │   1 │ config:                         │   1 │ config:                         
            │   2 │   database:                     │   2 │   database:                     
            │   3 │     host: localhost             │   3 │     host: localhost             
            │   4 │     port: 5432                  │   4 │     port: 5432                  
            │     │                                 │   5 │   cache:                        
            │     │                                 │   6 │     enabled: true               
            │     │                                 │   7 │     ttl: 3600                   

        "#]]
        .assert_eq(content.as_str());
    }

    #[test]
    fn display_removal_of_last_element_in_array() {
        // Removal of the last element in an array
        let left_doc = yaml_source(indoc! {r#"
            ---
            items:
              - first
              - second
              - third
        "#});

        let right_doc = yaml_source(indoc! {r#"
            ---
            items:
              - first
              - second
        "#});

        let mut diff_ctx = Context::default();
        diff_ctx.array_ordering = ArrayOrdering::Dynamic;

        let mut differences = diff(diff_ctx, &left_doc.yaml, &right_doc.yaml);

        let first = differences.remove(0);
        let Difference::Removed { path, value } = first else {
            panic!("Should have gotten a Removal, got: {:?}", first);
        };

        let content = render_removal(&ctx(), path, value, &left_doc, &right_doc);

        expect![[r#"
            │   1 │ items:                          │   1 │ items:                          
            │   2 │   - first                       │   2 │   - first                       
            │   3 │   - second                      │   3 │   - second                      
            │   4 │   - third                       │     │                                 "#]]
        .assert_eq(content.as_str());
    }

    #[test]
    fn display_addition_of_element_at_end_of_array() {
        // Addition at the end of an array
        let left_doc = yaml_source(indoc! {r#"
            ---
            items:
              - first
              - second
        "#});

        let right_doc = yaml_source(indoc! {r#"
            ---
            items:
              - first
              - second
              - third
        "#});

        let mut diff_ctx = Context::default();
        diff_ctx.array_ordering = ArrayOrdering::Dynamic;

        let mut differences = diff(diff_ctx, &left_doc.yaml, &right_doc.yaml);

        let first = differences.remove(0);
        let Difference::Added { path, value } = first else {
            panic!("Should have gotten an Addition, got: {:?}", first);
        };

        let content = render_added(&ctx(), path, value, &left_doc, &right_doc);

        expect![[r#"
            │   1 │ items:                          │   1 │ items:                          
            │   2 │   - first                       │   2 │   - first                       
            │   3 │   - second                      │   3 │   - second                      
            │     │                                 │   4 │   - third                       "#]]
        .assert_eq(content.as_str());
    }
}
