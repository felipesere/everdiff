use core::option::Option::None;
use std::{
    cmp::min,
    fmt::{self},
};

use crate::wrapping::Column;
use everdiff_diff::{
    Entry,
    path::{NonEmptyPath, Path, Segment},
};
use everdiff_line::Line;
use everdiff_multidoc::source::YamlSource;
use saphyr::{MarkedYamlOwned, YamlDataOwned};

use crate::inline_diff::{InlinePart, compute_inline_diff, extract_yaml_prefix};
use crate::node::node_in;

pub type Highlight = fn(&str) -> String;

#[derive(Copy, Clone)]
pub struct Theme {
    pub added: Highlight,
    pub removed: Highlight,
    pub changed: Highlight,
    pub dimmed: Highlight,
    pub header: Highlight,
}

impl Theme {
    pub fn colored() -> Self {
        use owo_colors::OwoColorize;
        Theme {
            added: |s| s.green().to_string(),
            removed: |s| s.red().to_string(),
            changed: |s| s.yellow().to_string(),
            dimmed: |s| s.dimmed().to_string(),
            header: |s| s.bold().to_string(),
        }
    }

    pub fn markers() -> Self {
        Theme {
            added: |s| format!("[green]{s}[/]"),
            removed: |s| format!("[red]{s}[/]"),
            changed: |s| format!("[yellow]{s}[/]"),
            dimmed: |s| format!("[dim]{s}[/]"),
            header: |s| format!("[bold]{s}[/]"),
        }
    }

    pub fn plain() -> Self {
        Theme {
            added: |s| s.to_string(),
            removed: |s| s.to_string(),
            changed: |s| s.to_string(),
            dimmed: |s| s.to_string(),
            header: |s| s.to_string(),
        }
    }

    pub fn added(&self, s: &str) -> String {
        (self.added)(s)
    }
    pub fn removed(&self, s: &str) -> String {
        (self.removed)(s)
    }
    pub fn changed(&self, s: &str) -> String {
        (self.changed)(s)
    }
    pub fn dimmed(&self, s: &str) -> String {
        (self.dimmed)(s)
    }
    pub fn header(&self, s: &str) -> String {
        (self.header)(s)
    }
}

#[derive(Clone)]
pub struct RenderContext {
    pub max_width: u16,
    pub visual_context: usize,
    pub theme: Theme,
}

impl RenderContext {
    pub fn new(max_width: u16) -> Self {
        RenderContext {
            max_width,
            visual_context: 5,
            theme: Theme::colored(),
        }
    }

    pub fn half_width(&self) -> usize {
        // Fixed chrome per side: outer "│ " (2) + line widget "{:>3} " (4) + inner "│ " (2) = 8
        // Two sides: 16 chars of total non-content width.
        const CHROME: u16 = 8 * 2;
        ((self.max_width - CHROME) / 2) as usize
    }
}

impl From<Line> for LineWidget {
    fn from(value: Line) -> Self {
        // TODO: We still do gross `±1` math in here
        // if the `Line` concept pans out we can clear it
        Self::Nr(value.get() - 1)
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
    /// Creates a snippet that will safely clamp the `to` value
    /// to not exceed the number of `lines`
    pub fn new_clamped<'source>(
        lines: &'source [&'source str],
        from: Line,
        to: Line,
    ) -> Snippet<'source> {
        assert!(
            !lines.is_empty(),
            "Can not create a snippet from empty lines"
        );
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
        let left = Snippet::new_clamped(self.lines, self.from, split_at);
        let right = Snippet::new_clamped(self.lines, split_at + 1, self.to);
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

        let snippet = Snippet::new_clamped(content, Line::unchecked(2), Line::unchecked(4));

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

        let snippet = Snippet::new_clamped(content, Line::unchecked(2), Line::unchecked(8));

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
    content: crate::wrapping::Column,
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
    path_to_change: NonEmptyPath,
    removal: Entry,
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
    path_to_change: NonEmptyPath,
    addition: Entry,
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
    path_to_change: NonEmptyPath,
    changed_yaml: Entry,
    left_doc: &YamlSource,
    right_doc: &YamlSource,
    change_type: ChangeType,
) -> String {
    log::debug!("Rendering change for {path_to_change}");
    log::debug!("The changed yaml node looks like: {:#?}", changed_yaml);

    // Select primary and secondary documents based on change type
    // The `larger_document` has more content and the changed_yaml will be highlighted.
    // The `gapped_document` has the gap in it
    let (larger_document, gapped_document) = match change_type {
        ChangeType::Removal => (left_doc, right_doc),
        ChangeType::Addition => (right_doc, left_doc),
    };

    let highlighting = match change_type {
        ChangeType::Removal => ctx.theme.removed,
        ChangeType::Addition => ctx.theme.added,
    };

    let primary = render_primary_side(
        ctx,
        larger_document,
        &changed_yaml,
        (highlighting, ctx.theme.dimmed),
    );
    let gap_size = changed_yaml.height();
    let primary_row_count = primary.row_count();
    let secondary = render_secondary_side(
        ctx,
        larger_document,
        gapped_document,
        path_to_change,
        primary_row_count,
        gap_size,
        ctx.theme.dimmed,
    );

    log::debug!(
        "Sizes:  primary {}, secondary {}",
        primary.row_count(),
        secondary.row_count()
    );

    // Combine the two sides based on change type
    let lines = match change_type {
        ChangeType::Removal => primary.zip_with(secondary, ctx.half_width()),
        ChangeType::Addition => secondary.zip_with(primary, ctx.half_width()),
    };

    lines.join("\n")
}

fn render_primary_side(
    ctx: &RenderContext,
    primary_doc: &YamlSource,
    item: &Entry,
    (highlighting, unchanged): (Highlight, Highlight),
) -> Column {
    use crate::wrapping::{SourceLineGroup, WrappedLine};

    // Extract lines from primary document
    let primary_lines = primary_doc.lines();

    let (change_start, change_end) = match item {
        Entry::KV { key, value } => (
            primary_doc.relative_line(key.span.start.line()),
            primary_doc.relative_line(value.span.end.line()),
        ),
        Entry::ArrayElement { value, .. } => (
            primary_doc.relative_line(value.span.start.line()),
            primary_doc.relative_line(value.span.end.line()),
        ),
    };

    // Show a few more lines before and after the lines that have changed
    let start = change_start.saturating_sub(ctx.visual_context);
    let end = min(change_end + ctx.visual_context, primary_doc.last_line);
    log::debug!("Snippet for primary document");
    let primary_snippet = Snippet::new_clamped(&primary_lines, start, end);

    // Format the primary side
    let mut changed_range = change_start..change_end;
    if changed_range.is_empty() {
        // We need to at least highlight 1 line!
        changed_range = change_start..(change_end + 1);
    }
    log::debug!("We will highlight {change_start}..={change_end}");
    let groups: Vec<SourceLineGroup> = primary_snippet
        .iter()
        .map(move |(line_nr, line)| {
            let style = if changed_range.contains(&line_nr) {
                highlighting
            } else {
                unchanged
            };

            let wrapped = WrappedLine::new(line_nr, line, ctx.half_width());
            wrapped.format(style, ctx.half_width())
        })
        .collect();

    Column(groups)
}

fn render_secondary_side(
    ctx: &RenderContext,
    primary_doc: &YamlSource,
    secondary_doc: &YamlSource,
    path_to_changed_node: NonEmptyPath,
    primary_row_count: usize,
    gap_size: usize,
    unchanged: Highlight,
) -> Column {
    use crate::wrapping::{FormattedRow, SourceLineGroup, WrappedLine};

    log::debug!("changed_node: {path_to_changed_node}");

    let gap_start =
        gap_start(primary_doc, secondary_doc, path_to_changed_node).unwrap_or(Line::one());
    log::debug!("The gap should be right after: {gap_start}");
    let start = (gap_start + 1).saturating_sub(ctx.visual_context);
    let end: Line = gap_start + ctx.visual_context + 1;

    let lines = secondary_doc.lines();

    let s = Snippet::new_clamped(&lines, start, end);
    log::debug!("Secondary snippet len: {}", s.lines.len());
    log::debug!("{:?}", &s.lines);
    let (before_gap, after_gap) = s.split(gap_start);
    log::debug!("after split:");
    log::debug!("before_gap: {}->{}", before_gap.from, before_gap.to);
    log::debug!("after_gap: {}->{}", after_gap.from, after_gap.to);

    let filler_len = if end.distance(&start) > primary_row_count {
        0
    } else {
        (end.distance(&start)).saturating_sub(primary_row_count)
    };
    log::debug!("Filler will be {filler_len}");

    let mut groups: Vec<SourceLineGroup> = Vec::new();

    // Filler lines (single-row groups)
    for _ in 0..filler_len {
        groups.push(SourceLineGroup(vec![FormattedRow::blank(ctx.half_width())]));
    }

    // Pre-gap lines
    for (line_nr, line) in before_gap.iter() {
        let wrapped = WrappedLine::new(line_nr, line, ctx.half_width());
        groups.push(wrapped.format(unchanged, ctx.half_width()));
    }

    // Gap lines (blank rows)
    for _ in 0..gap_size {
        groups.push(SourceLineGroup(vec![FormattedRow::blank(ctx.half_width())]));
    }

    // Post-gap lines
    for (line_nr, line) in after_gap.iter() {
        let wrapped = WrappedLine::new(line_nr, line, ctx.half_width());
        groups.push(wrapped.format(unchanged, ctx.half_width()));
    }

    Column(groups)
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
    path_to_change: NonEmptyPath,
) -> Option<Line> {
    let parent = path_to_change.parent();
    let primary_parent_node = node_in(&primary_doc.yaml, &parent)?;

    let (before_path, after_path) =
        surrounding_paths(primary_parent_node, parent.clone(), path_to_change.head())?;

    log::debug!(
        "The before node is {:?}",
        &before_path.as_ref().map(|p| p.to_string())
    );
    log::debug!(
        "The after node is {:?}",
        &after_path.as_ref().map(|p| p.to_string())
    );

    // TODO: I think this needs something similar to what I did with Entry::KV and Entry::ArrayElement
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
        Some(secondary_doc.relative_line(before.span.end.line() - adjustment))
    } else if let Some(after) = after_path {
        // No "before" node (e.g., adding at index 0 of an array).
        // Use the "after" node to find where the gap should go.
        // For sequences, the after_path index needs to be decremented by 1
        // because secondary doesn't have the new element.
        let adjusted_path = adjust_path_for_secondary(&after, &primary_parent_node.data);
        log::debug!(
            "Adjusted after_path for secondary: {:?}",
            adjusted_path.to_string()
        );

        if let Some(after_node) = node_in(&secondary_doc.yaml, &adjusted_path) {
            // Gap should appear just before this element
            let start_line = after_node.span.start.line();
            log::debug!(
                "After node starts at line {}, gap_start will be {}",
                start_line,
                start_line - 1
            );
            Some(secondary_doc.relative_line(start_line - 1))
        } else {
            // Fallback: use parent node's start
            log::debug!("Could not find after node in secondary, falling back to parent");
            let secondary_parent = node_in(&secondary_doc.yaml, &parent);
            Some(
                secondary_parent
                    .map(|p| secondary_doc.relative_line(p.span.start.line()))
                    .unwrap_or(Line::one()),
            )
        }
    } else {
        // No before or after path, fall back to line 1
        log::debug!("No before or after path, falling back to Line::one()");
        Some(Line::one())
    }
}

#[cfg(test)]
mod test_node_height {
    use indoc::indoc;
    use saphyr::{LoadableYamlNode, MarkedYamlOwned, SafelyIndex};

    use everdiff_diff::Entry;

    #[test]
    fn height_of_simple_string() {
        let raw = indoc! {r#"
          element: "Hi there"
        "#};

        let mut yaml = MarkedYamlOwned::load_from_str(raw).unwrap();
        let yaml = yaml.remove(0);
        let (key, value) = yaml.data.as_mapping().unwrap().into_iter().next().unwrap();
        let item = Entry::KV {
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
        let item = Entry::KV {
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
        let item = Entry::KV {
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
        let item = Entry::KV {
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
        let item = Entry::KV {
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
        let item = Entry::KV {
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
        let item = Entry::ArrayElement {
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
        let item = Entry::KV {
            key: (*key).clone(),
            value: (*value).clone(),
        };

        assert_eq!(5, item.height());
    }
}

#[cfg(test)]
mod test_gap_start {
    use everdiff_diff::path::{NonEmptyPath, Path};
    use everdiff_line::Line;
    use everdiff_multidoc::source::read_doc;
    use test_log::test;

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

        let primary = read_doc(primary, &camino::Utf8PathBuf::default())
            .unwrap()
            .remove(0);

        let secondary = indoc::indoc! {r#"
            ---
            person:
              name: Steve E. Anderson
              age: 12
            "#};
        let secondary = read_doc(secondary, &camino::Utf8PathBuf::default())
            .unwrap()
            .remove(0);

        let location = NonEmptyPath::try_from(Path::parse_str(".person.location").unwrap())
            .expect("non-empty path");

        let actual_start = gap_start(&primary, &secondary, location);

        // The split we are looking for is
        // [1] person:
        // [2]   name: Steve E. Anderson
        // <--- the gap --->
        // [3]   age: 12
        assert_eq!(actual_start, Some(Line::unchecked(2)));
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

        let primary = read_doc(primary, &camino::Utf8PathBuf::default())
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
        let secondary = read_doc(secondary, &camino::Utf8PathBuf::default())
            .unwrap()
            .remove(0);

        let location =
            NonEmptyPath::try_from(Path::parse_str(".metadata.annotations.this_is").unwrap())
                .expect("non-empty path");

        let actual_start = gap_start(&primary, &secondary, location);

        assert_eq!(actual_start, Some(Line::new(9).unwrap()));
    }

    #[test]
    fn empty_path_cannot_be_converted_to_non_empty_path() {
        // The type system now prevents calling gap_start with an empty path.
        // NonEmptyPath::try_from rejects empty paths at construction time.
        assert!(NonEmptyPath::try_from(Path::default()).is_err());
        assert!(NonEmptyPath::try_new(vec![]).is_none());
    }

    #[test]
    fn gap_start_returns_none_when_parent_path_missing_from_primary() {
        let doc = read_doc(
            indoc::indoc! {r#"
                ---
                real: value
            "#},
            &camino::Utf8PathBuf::default(),
        )
        .unwrap()
        .remove(0);

        let path = NonEmptyPath::try_from(Path::parse_str(".ghost.field").unwrap()).unwrap();
        assert!(gap_start(&doc, &doc, path).is_none());
    }

    #[test]
    fn gap_start_returns_none_when_field_segment_points_into_sequence() {
        use everdiff_diff::path::Segment;

        let doc = read_doc(
            indoc::indoc! {r#"
                ---
                items:
                  - foo
                  - bar
            "#},
            &camino::Utf8PathBuf::default(),
        )
        .unwrap()
        .remove(0);

        let path = NonEmptyPath::try_new(vec![
            Segment::Field("items".to_string()),
            Segment::Field("name".to_string()),
        ])
        .unwrap();
        assert!(gap_start(&doc, &doc, path).is_none());
    }

    #[test]
    fn gap_start_returns_none_when_index_segment_points_into_mapping() {
        use everdiff_diff::path::Segment;

        let doc = read_doc(
            indoc::indoc! {r#"
                ---
                data:
                  key1: val1
                  key2: val2
            "#},
            &camino::Utf8PathBuf::default(),
        )
        .unwrap()
        .remove(0);

        let path =
            NonEmptyPath::try_new(vec![Segment::Field("data".to_string()), Segment::Index(0)])
                .unwrap();
        assert!(gap_start(&doc, &doc, path).is_none());
    }
}

pub fn render_difference(
    ctx: &RenderContext,
    path_to_change: Option<NonEmptyPath>,
    left: MarkedYamlOwned,
    left_doc: &YamlSource,
    right: MarkedYamlOwned,
    right_doc: &YamlSource,
) -> String {
    let title = match &path_to_change {
        Some(path) => format!("Changed: {}:", ctx.theme.header(&path.to_string())),
        None => "Changed:".to_string(),
    };

    const CHROME: u16 = 8 * 2;
    let max_width = (ctx.max_width - CHROME) / 2;
    let smaller_context = RenderContext {
        max_width,
        theme: ctx.theme,
        visual_context: 5, // this will become a parameter down the line
    };

    let (left, right) = render_changed_pair(&smaller_context, left, left_doc, right, right_doc);

    use crate::wrapping::{FormattedRow, SourceLineGroup};

    let above_filler = left.lines_above.abs_diff(right.lines_above);
    let below_filler = left.lines_below.abs_diff(right.lines_below);

    let half_width = usize::from(max_width);
    let filler_group = || SourceLineGroup(vec![FormattedRow::blank(half_width)]);

    let mut left_groups = left.content.0;
    let mut right_groups = right.content.0;

    // Prepend top filler to the side with fewer lines above
    if left.lines_above < right.lines_above {
        let mut filler: Vec<_> = (0..above_filler).map(|_| filler_group()).collect();
        filler.append(&mut left_groups);
        left_groups = filler;
    } else {
        let mut filler: Vec<_> = (0..above_filler).map(|_| filler_group()).collect();
        filler.append(&mut right_groups);
        right_groups = filler;
    }

    // Append bottom filler to the side with fewer lines below
    if left.lines_below < right.lines_below {
        left_groups.extend((0..below_filler).map(|_| filler_group()));
    } else {
        right_groups.extend((0..below_filler).map(|_| filler_group()));
    }

    let left_col = Column(left_groups);
    let right_col = Column(right_groups);

    let body = left_col.zip_with(right_col, ctx.half_width()).join("\n");

    format!("{title}\n{body}")
}

fn render_changed_pair(
    ctx: &RenderContext,
    left: MarkedYamlOwned,
    left_doc: &YamlSource,
    right: MarkedYamlOwned,
    right_doc: &YamlSource,
) -> (Rendered, Rendered) {
    let (left_parts, right_parts) = left.data.as_str()
        .zip(right.data.as_str())
        .map(|(l, r)| compute_inline_diff(l, r))
        .unzip();

    let left = render_changed_snippet(ctx, left_doc, left, left_parts);
    let right = render_changed_snippet(ctx, right_doc, right, right_parts);
    (left, right)
}

fn render_changed_snippet(
    ctx: &RenderContext,
    source: &YamlSource,
    changed_yaml: MarkedYamlOwned,
    inline_parts: Option<Vec<InlinePart>>,
) -> Rendered {
    use crate::wrapping::{SourceLineGroup, WrappedLineUsize, format_with_inline_highlights};

    // lines to render above and below if available...
    let context = 5;
    let start_line_of_document = source.yaml.span.start.line();

    let lines: Vec<_> = source.content.lines().map(|s| s.to_string()).collect();

    let changed_line = changed_yaml.span.start.line() - start_line_of_document;
    let start = changed_line.saturating_sub(context);
    let end = min(changed_line + context, lines.len());
    let left_snippet = &lines[start..end];

    let lines_above = changed_line - start;
    let lines_below = end - changed_line;

    let width = usize::from(ctx.max_width);

    let groups: Vec<SourceLineGroup> = left_snippet
        .iter()
        .zip(start..end)
        .map(|(line, line_nr)| {
            if line_nr == changed_line {
                if let Some(parts) = &inline_parts {
                    let prefix = extract_yaml_prefix(line);
                    format_with_inline_highlights(line_nr, prefix, parts, ctx.theme, width)
                } else {
                    let wrapped = WrappedLineUsize {
                        line_nr,
                        segments: crate::wrapping::wrap_text(line, width),
                    };
                    wrapped.format_with_usize(ctx.theme.changed, width)
                }
            } else {
                let wrapped = WrappedLineUsize {
                    line_nr,
                    segments: crate::wrapping::wrap_text(line, width),
                };
                wrapped.format_with_usize(ctx.theme.dimmed, width)
            }
        })
        .collect();

    Rendered {
        content: Column(groups),
        lines_above,
        lines_below,
    }
}

// pub struct LineWidget(pub Option<usize>);
pub enum LineWidget {
    Nr(usize),
    Continuation,
    Filler,
}

impl fmt::Display for LineWidget {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Nr(idx) => write!(f, "{:>3} ", idx + 1),
            Self::Continuation => write!(f, "  ┆ "),
            Self::Filler => write!(f, "    "),
        }
    }
}

fn surrounding_paths(
    parent_node: &MarkedYamlOwned,
    parent_path: Path,
    head: &Segment,
) -> Option<(Option<Path>, Option<Path>)> {
    log::trace!("the parent is: {parent_path}");
    log::trace!("the parent node is: {:#?}", parent_node);
    match &parent_node.data {
        YamlDataOwned::Sequence(children) => {
            let idx = head.as_index()?;
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
            Some((left, right))
        }
        YamlDataOwned::Mapping(children) => {
            // Consider extracting this...
            let target_key = head.as_field()?;
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
                Some((
                    before.map(|k| parent_path.push(k)),
                    after.map(|k| parent_path.push(k)),
                ))
            } else {
                Some((None, None))
            }
        }
        _ => unreachable!("parent has to be a container"),
    }
}

#[cfg(test)]
mod test {
    use everdiff_multidoc::source::{YamlSource, read_doc};
    use test_log::test;

    use expect_test::expect;
    use indoc::indoc;

    use crate::render;
    use everdiff_diff::{ArrayOrdering, Context, Difference, diff};

    use super::{RenderContext, render_added, render_difference, render_removal};

    fn ctx() -> RenderContext {
        RenderContext {
            max_width: 80,
            theme: super::Theme::markers(),
            visual_context: 5,
        }
    }

    fn yaml_source(yaml: &'static str) -> YamlSource {
        let mut docs =
            read_doc(yaml, &camino::Utf8PathBuf::new()).expect("to have parsed properly");
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
            Changed: [bold].person.name[/]:
            │   1 │ [dim]person:[/]                 │   1 │ [dim]person:[/]                 
            │   2 │ [dim]  [/][yellow]name[/][dim]: [/][yellow]S[/][dim]t[/][yellow]eve E.[/][dim] Anderson[/]│   2 │ [dim]  [/][yellow]name[/][dim]: [/][yellow]Rober[/][dim]t Anderson[/]
            │   3 │ [dim]  age: 12[/]               │   3 │ [dim]  age: 12[/]               "#]]
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

        let content = render(ctx(), &left_doc, &right_doc, differences);

        expect![[r#"
            Removed: .person.address:
            │   1 │ [dim]person:[/]                 │   1 │ [dim]person:[/]                 
            │   2 │ [dim]  name: Robert Anderson[/] │   2 │ [dim]  name: Robert Anderson[/] 
            │   3 │ [red]  address:[/]              │     │                                 
            │   4 │ [red]    street: foo bar[/]     │     │                                 
            │   5 │ [red]    nr: 1[/]               │     │                                 
            │   6 │ [red]    postcode: ABC123[/]    │     │                                 
            │   7 │ [dim]  age: 12[/]               │   3 │ [dim]  age: 12[/]               
            │   8 │ [dim]  foo: bar[/]              │   4 │ [dim]  foo: bar[/]              

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

        let content = render(ctx(), &left_doc, &right_doc, differences);

        expect![[r#"
            Added: [bold].person.address[/]:
            │   1 │ [dim]person:[/]                 │   1 │ [dim]person:[/]                 
            │   2 │ [dim]  name: Robert Anderson[/] │   2 │ [dim]  name: Robert Anderson[/] 
            │     │                                 │   3 │ [green]  address:[/]            
            │     │                                 │   4 │ [green]    street: foo bar[/]   
            │     │                                 │   5 │ [green]    nr: 1[/]             
            │     │                                 │   6 │ [green]    postcode: ABC123[/]  
            │   3 │ [dim]  age: 12[/]               │   7 │ [dim]  age: 12[/]               
            │   4 │ [dim]  foo: bar[/]              │   8 │ [dim]  foo: bar[/]              

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
            │   1 │ [dim]people:[/]                 │   1 │ [dim]people:[/]                 
            │   2 │ [dim]  - name: Robert Anderson[/]│   2 │ [dim]  - name: Robert Anderson[/]
            │   3 │ [dim]    age: 20[/]             │   3 │ [dim]    age: 20[/]             
            │     │                                 │   4 │ [green]  - name: Adam Bar[/]    
            │     │                                 │   5 │ [green]    age: 32[/]           
            │   4 │ [dim]  - name: Sarah Foo[/]     │   6 │ [dim]  - name: Sarah Foo[/]     
            │   5 │ [dim]    age: 31[/]             │   7 │ [dim]    age: 31[/]             "#]]
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
            │   1 │ [dim]people:[/]                 │   1 │ [dim]people:[/]                 
            │     │                                 │   2 │ [green]  - name: New First Person[/]
            │     │                                 │   3 │ [green]    age: 25[/]           
            │   2 │ [dim]  - name: Robert Anderson[/]│   4 │ [dim]  - name: Robert Anderson[/]
            │   3 │ [dim]    age: 20[/]             │   5 │ [dim]    age: 20[/]             
            │   4 │ [dim]  - name: Sarah Foo[/]     │   6 │ [dim]  - name: Sarah Foo[/]     
            │   5 │ [dim]    age: 31[/]             │   7 │ [dim]    age: 31[/]             "#]]
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
        assert_eq!(path.to_string(), ".spec.template.spec.containers[0].env[0]");

        let content = render_added(&ctx(), path, value, &left_doc, &right_doc);

        // The left side should show the area around the `env:` array,
        // NOT the beginning of the file (line 1)
        expect![[r#"
            │   6 │ [dim]  template:[/]             │   6 │ [dim]  template:[/]             
            │   7 │ [dim]    spec:[/]               │   7 │ [dim]    spec:[/]               
            │   8 │ [dim]      containers:[/]       │   8 │ [dim]      containers:[/]       
            │   9 │ [dim]      - name: app[/]       │   9 │ [dim]      - name: app[/]       
            │  10 │ [dim]        env:[/]            │  10 │ [dim]        env:[/]            
            │     │                                 │  11 │ [green]        - name: NEW_FIRST_VAR[/]
            │     │                                 │  12 │ [green]          value: "new"[/]
            │  11 │ [dim]        - name: EXISTING_VAR[/]│  13 │ [dim]        - name: EXISTING_VAR[/]
            │  12 │ [dim]          value: "existing"[/]│  14 │ [dim]          value: "existing"[/]"#]]
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
            RenderContext {
                max_width: 80,
                theme: super::Theme::markers(),
                visual_context: 5,
            },
            &left_doc,
            &right_doc,
            differences,
        );

        expect![[r#"
            Changed: [bold].person.name[/]:
            │   1 │ [dim]person:[/]                 │   1 │ [dim]person:[/]                 
            │   2 │ [dim]  [/][yellow]name[/][dim]: [/][dim]Steve[/][yellow] E.[/][dim] Anderson[/]│   2 │ [dim]  [/][yellow]name[/][dim]: [/][dim]Steve[/][yellow]n[/][dim] Anderson[/]
            │   3 │ [dim]  age: 12[/]               │   3 │ [dim]  location:[/]             
            │     │                                 │   4 │ [dim]    street: 1 Kentish Street[/]
            │     │                                 │   5 │ [dim]    postcode: KS87JJ[/]    
            │     │                                 │   6 │ [dim]  age: 34[/]               

            Changed: [bold].person.age[/]:
            │     │                                 │   1 │ [dim]person:[/]                 
            │     │                                 │   2 │ [dim]  name: Steven Anderson[/] 
            │     │                                 │   3 │ [dim]  location:[/]             
            │   1 │ [dim]person:[/]                 │   4 │ [dim]    street: 1 Kentish Street[/]
            │   2 │ [dim]  name: Steve E. Anderson[/]│   5 │ [dim]    postcode: KS87JJ[/]    
            │   3 │ [yellow]  age: 12[/]            │   6 │ [yellow]  age: 34[/]            

            Added: [bold].person.location[/]:
            │   1 │ [dim]person:[/]                 │   1 │ [dim]person:[/]                 
            │   2 │ [dim]  name: Steve E. Anderson[/]│   2 │ [dim]  name: Steven Anderson[/] 
            │     │                                 │   3 │ [green]  location:[/]           
            │     │                                 │   4 │ [green]    street: 1 Kentish Street[/]
            │     │                                 │   5 │ [green]    postcode: KS87JJ[/]  
            │   3 │ [dim]  age: 12[/]               │   6 │ [dim]  age: 34[/]               

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
            RenderContext {
                max_width: 150,
                theme: super::Theme::markers(),
                visual_context: 5,
            },
            &left_doc,
            &right_doc,
            differences,
        );

        expect![[r#"
            Added: [bold].metadata.annotations.this_is[/]:
            │   9 │ [dim]    app: flux-engine-steam[/]                                 │   9 │ [dim]    app: flux-engine-steam[/]                                 
            │  10 │ [dim]    app.kubernetes.io/version: 0.0.27-pre1[/]                 │  10 │ [dim]    app.kubernetes.io/version: 0.0.27-pre1[/]                 
            │  11 │ [dim]    app.kubernetes.io/managed-by: batman[/]                   │  11 │ [dim]    app.kubernetes.io/managed-by: batman[/]                   
            │  12 │ [dim]  annotations:[/]                                             │  12 │ [dim]  annotations:[/]                                             
            │  13 │ [dim]    github.com/repository_url: git@github.com:flux-engine-steam[/]│  13 │ [dim]    github.com/repository_url: git@github.com:flux-engine-steam[/]
            │     │                                                                    │  14 │ [green]    this_is: new[/]                                         
            │  14 │ [dim]spec:[/]                                                      │  15 │ [dim]spec:[/]                                                      
            │  15 │ [dim]  ports:[/]                                                   │  16 │ [dim]  ports:[/]                                                   
            │  16 │ [dim]    - targetPort: 8501[/]                                     │  17 │ [dim]    - targetPort: 8502[/]                                     
            │  17 │ [dim]      port: 3000[/]                                           │  18 │ [dim]      port: 3000[/]                                           
            │  18 │ [dim]      name: https[/]                                          │  19 │ [dim]      name: https[/]                                          

            Changed: [bold].spec.ports[0].targetPort[/]:
            │  11 │ [dim]    app.kubernetes.io/managed-by: batman[/]                   │  12 │ [dim]  annotations:[/]                                             
            │  12 │ [dim]  annotations:[/]                                             │  13 │ [dim]    github.com/repository_url: git@github.com:flux-engine-steam[/]
            │  13 │ [dim]    github.com/repository_url: git@github.com:flux-engine-steam[/]│  14 │ [dim]    this_is: new[/]                                           
            │  14 │ [dim]spec:[/]                                                      │  15 │ [dim]spec:[/]                                                      
            │  15 │ [dim]  ports:[/]                                                   │  16 │ [dim]  ports:[/]                                                   
            │  16 │ [yellow]    - targetPort: 8501[/]                                  │  17 │ [yellow]    - targetPort: 8502[/]                                  
            │  17 │ [dim]      port: 3000[/]                                           │  18 │ [dim]      port: 3000[/]                                           
            │  18 │ [dim]      name: https[/]                                          │  19 │ [dim]      name: https[/]                                          
            │  19 │ [dim]  selector:[/]                                                │  20 │ [dim]  selector:[/]                                                
            │  20 │ [dim]    app: flux-engine-steam[/]                                 │  21 │ [dim]    app: flux-engine-steam[/]                                 

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
            │   1 │ [dim]people:[/]                 │   1 │ [dim]people:[/]                 
            │   2 │ [dim]  - name: Alice[/]         │   2 │ [dim]  - name: Alice[/]         
            │   3 │ [dim]    age: 25[/]             │   3 │ [dim]    age: 25[/]             
            │   4 │ [dim]  - name: Bob[/]           │   4 │ [dim]  - name: Charlie[/]       
            │   5 │ [dim]    age: 30[/]             │   5 │ [dim]    age: 35[/]             
            │   6 │ [red]  - name: Charlie[/]       │     │                                 
            │   7 │ [red]    age: 35[/]             │     │                                 "#]]
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
            │   1 │ [dim]people:[/]                 │   1 │ [dim]people:[/]                 
            │   2 │ [dim]  - name: First Person[/]  │   2 │ [dim]  - name: Second Person[/] 
            │   3 │ [dim]    age: 20[/]             │   3 │ [dim]    age: 30[/]             
            │   4 │ [dim]  - name: Second Person[/] │   4 │ [dim]  - name: Third Person[/]  
            │   5 │ [dim]    age: 30[/]             │   5 │ [dim]    age: 40[/]             
            │   6 │ [red]  - name: Third Person[/]  │     │                                 
            │   7 │ [red]    age: 40[/]             │     │                                 "#]]
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

        let content = render(ctx(), &left_doc, &right_doc, differences);

        // The gap on the right should align correctly with the removed annotations
        // Both sides should start at the same line number
        expect![[r#"
            Removed: .metadata.annotations:
            │   2 │ [dim]  name: my-service[/]      │   2 │ [dim]  name: my-service[/]      
            │   3 │ [dim]  labels:[/]               │   3 │ [dim]  labels:[/]               
            │   4 │ [dim]    app: my-app[/]         │   4 │ [dim]    app: my-app[/]         
            │   5 │ [dim]    version: "1.0"[/]      │   5 │ [dim]    version: "1.0"[/]      
            │   6 │ [dim]    environment: production[/]│   6 │ [dim]    environment: production[/]
            │   7 │ [red]  annotations:[/]          │     │                                 
            │   8 │ [red]    description: "My service des[/]│     │                                 
            │   ┆ │ [red]cription"[/]               │     │                                 
            │   9 │ [dim]spec:[/]                   │   7 │ [dim]spec:[/]                   
            │  10 │ [dim]  replicas: 3[/]           │   8 │ [dim]  replicas: 3[/]           

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

        let content = render(ctx(), &left_doc, &right_doc, differences);

        expect![[r#"
            Changed: [bold].servers[1].port[/]:
            │   1 │ [dim]servers:[/]                │   1 │ [dim]servers:[/]                
            │   2 │ [dim]  - host: server1.example.com[/]│   2 │ [dim]  - host: server1.example.com[/]
            │   3 │ [dim]    port: 8080[/]          │   3 │ [dim]    port: 8080[/]          
            │   4 │ [dim]  - host: server2.example.com[/]│   4 │ [dim]  - host: server2.example.com[/]
            │   5 │ [yellow]    port: 9090[/]       │   5 │ [yellow]    port: 9091[/]       

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

        let content = render(ctx(), &left_doc, &right_doc, differences);

        expect![[r#"
            Removed: .config.cache:
            │   1 │ [dim]config:[/]                 │   1 │ [dim]config:[/]                 
            │   2 │ [dim]  database:[/]             │   2 │ [dim]  database:[/]             
            │   3 │ [dim]    host: localhost[/]     │   3 │ [dim]    host: localhost[/]     
            │   4 │ [dim]    port: 5432[/]          │   4 │ [dim]    port: 5432[/]          
            │   5 │ [red]  cache:[/]                │     │                                 
            │   6 │ [red]    enabled: true[/]       │     │                                 
            │   7 │ [red]    ttl: 3600[/]           │     │                                 

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

        let content = render(ctx(), &left_doc, &right_doc, differences);

        expect![[r#"
            Added: [bold].config.cache[/]:
            │   1 │ [dim]config:[/]                 │   1 │ [dim]config:[/]                 
            │   2 │ [dim]  database:[/]             │   2 │ [dim]  database:[/]             
            │   3 │ [dim]    host: localhost[/]     │   3 │ [dim]    host: localhost[/]     
            │   4 │ [dim]    port: 5432[/]          │   4 │ [dim]    port: 5432[/]          
            │     │                                 │   5 │ [green]  cache:[/]              
            │     │                                 │   6 │ [green]    enabled: true[/]     
            │     │                                 │   7 │ [green]    ttl: 3600[/]         

        "#]]
        .assert_eq(content.as_str());
    }

    #[test]
    fn display_addition_of_final_key_in_document() {
        // The added key is the very last item in the document: `gap_start` lands on
        // the last line of the left (gapped) doc, so the right half of the Snippet
        // split is empty.  This previously triggered a debug_assert in new_clamped.
        let left_doc = yaml_source(indoc! {r#"
            ---
            person:
              name: Alice
              age: 30
        "#});

        let right_doc = yaml_source(indoc! {r#"
            ---
            person:
              name: Alice
              age: 30
              city: London
        "#});

        let differences = diff(Context::default(), &left_doc.yaml, &right_doc.yaml);
        let content = render(ctx(), &left_doc, &right_doc, differences);

        expect![[r#"
            Added: [bold].person.city[/]:
            │   1 │ [dim]person:[/]                 │   1 │ [dim]person:[/]                 
            │   2 │ [dim]  name: Alice[/]           │   2 │ [dim]  name: Alice[/]           
            │   3 │ [dim]  age: 30[/]               │   3 │ [dim]  age: 30[/]               
            │     │                                 │   4 │ [green]  city: London[/]        

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
            │   1 │ [dim]items:[/]                  │   1 │ [dim]items:[/]                  
            │   2 │ [dim]  - first[/]               │   2 │ [dim]  - first[/]               
            │   3 │ [dim]  - second[/]              │   3 │ [dim]  - second[/]              
            │   4 │ [red]  - third[/]               │     │                                 "#]]
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
            │   1 │ [dim]items:[/]                  │   1 │ [dim]items:[/]                  
            │   2 │ [dim]  - first[/]               │   2 │ [dim]  - first[/]               
            │   3 │ [dim]  - second[/]              │   3 │ [dim]  - second[/]              
            │     │                                 │   4 │ [green]  - third[/]             "#]]
        .assert_eq(content.as_str());
    }
}
