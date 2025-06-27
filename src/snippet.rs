use std::{
    cmp::{max, min},
    fmt::{self},
    num::NonZeroUsize,
    ops::{Add, Sub},
};

use ansi_width::ansi_width;
use anyhow::Context;
use owo_colors::{OwoColorize, Style};
use saphyr::{Indexable, MarkedYamlOwned, YamlDataOwned};

use crate::{YamlSource, path::Path};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Color {
    Enabled,
    // mostly used in tests
    #[allow(dead_code)]
    Disabled,
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
pub struct Line(NonZeroUsize);

impl Line {
    fn get(self) -> usize {
        self.0.get()
    }

    pub fn new(raw: usize) -> Option<Self> {
        Some(Line(NonZeroUsize::try_from(raw).ok()?))
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

impl Sub<usize> for Line {
    type Output = Line;

    fn sub(self, rhs: usize) -> Self::Output {
        let val = self.0.get();
        if val <= rhs {
            Line::new(1).unwrap()
        } else {
            let val = val - rhs;
            Line::new(val).unwrap()
        }
    }
}

impl Sub<Line> for usize {
    type Output = Line;

    fn sub(self, rhs: Line) -> Self::Output {
        let val = self - rhs.0.get();
        Line::new(max(val, 1)).expect("Value can't drop below 1")
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

#[derive(Debug)]
struct Snippet<'source> {
    lines: &'source [&'source str],
    from: Line,
    to: Line,
}

impl Snippet<'_> {
    pub fn try_new<'source>(
        lines: &'source [&'source str],
        from: Line,
        to: Line,
    ) -> Result<Snippet<'source>, anyhow::Error> {
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

        let snippet =
            Snippet::try_new(content, Line::new(2).unwrap(), Line::new(4).unwrap()).unwrap();

        let actual_lines: Vec<_> = snippet
            .iter()
            .map(|(nr, content)| (nr, content.to_string()))
            .collect();

        assert_eq!(
            vec![
                (Line::new(2).unwrap(), "b".to_string()),
                (Line::new(3).unwrap(), "c".to_string()),
                (Line::new(4).unwrap(), "d".to_string())
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

        let snippet =
            Snippet::try_new(content, Line::new(2).unwrap(), Line::new(8).unwrap()).unwrap();

        let (first, second) = snippet.split(Line::new(6).unwrap());

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
                (Line::new(2).unwrap(), "b".to_string()),
                (Line::new(3).unwrap(), "c".to_string()),
                (Line::new(4).unwrap(), "d".to_string()),
                (Line::new(5).unwrap(), "e".to_string()),
                (Line::new(6).unwrap(), "f".to_string())
            ],
            first_lines
        );

        assert_eq!(
            vec![
                (Line::new(7).unwrap(), "g".to_string()),
                (Line::new(8).unwrap(), "h".to_string()),
            ],
            second_lines
        );
    }
}

// We're going to need a "render context" or "render options" at some point
// to control a couple of aspects:
// * rendering the changes "in snippets" or not?
// * how many lines above and below to to show?
// * show colors or not?
// * line numbers that match up with the actual file
//    - this matters in particular for multi-doc docs

pub fn render_removal(
    path_to_change: Path,
    removal: MarkedYamlOwned,
    left_doc: &YamlSource,
    right_doc: &YamlSource,
    max_width: u16,
    color: Color,
) -> String {
    render_change(
        path_to_change,
        removal,
        left_doc,
        right_doc,
        max_width,
        color,
        ChangeType::Removal,
    )
}

pub fn render_added(
    path_to_change: Path,
    addition: MarkedYamlOwned,
    left_doc: &YamlSource,
    right_doc: &YamlSource,
    max_width: u16,
    color: Color,
) -> String {
    render_change(
        path_to_change,
        addition,
        left_doc,
        right_doc,
        max_width,
        color,
        ChangeType::Addition,
    )
}

enum ChangeType {
    Removal,
    Addition,
}

fn render_change(
    path_to_change: Path,
    changed_yaml: MarkedYamlOwned,
    left_doc: &YamlSource,
    right_doc: &YamlSource,
    max_width: u16,
    color: Color,
    change_type: ChangeType,
) -> String {
    log::debug!("Rendering change for {}", path_to_change.jq_like());
    log::debug!("The val is: {:#?}", changed_yaml);
    let ctx_size = 5;
    let max_left = ((max_width - 16) / 2) as usize; // includes a bit of random padding, do this proper later

    // Select primary and secondary documents based on change type
    // The `primary_doc` more content and the changed_yaml will be highlighted.
    // The `secondary_doc` has the gap in it
    let (primary_doc, secondary_doc) = match change_type {
        ChangeType::Removal => (left_doc, right_doc),
        ChangeType::Addition => (right_doc, left_doc),
    };

    // Extract lines from primary document
    let primary_lines: Vec<_> = primary_doc
        .content
        .lines()
        .skip_while(|s| *s == "---")
        .collect();

    let change_start = changed_yaml.span.start.line() - primary_doc.first_line;
    let change_end = changed_yaml.span.end.line() - primary_doc.first_line;

    // Show a few more lines before and after the lines that have changed
    let start = change_start - ctx_size;
    let end = min(change_end + ctx_size, primary_doc.last_line);
    let primary_snippet =
        Snippet::try_new(&primary_lines, start, end).expect("Primary snippet could not be created");

    // Set up styles
    let (highlighting, unchanged) = match color {
        Color::Enabled => (
            match change_type {
                ChangeType::Removal => owo_colors::Style::new().green(),
                ChangeType::Addition => owo_colors::Style::new().red(),
            },
            owo_colors::Style::new().dimmed(),
        ),
        Color::Disabled => (owo_colors::Style::new(), owo_colors::Style::new()),
    };

    // Format the primary side
    let changed_range = change_start..=change_end;
    let primary = primary_snippet.iter().map(|(line_nr, line)| {
        let line = if changed_range.contains(&line_nr) {
            line.style(highlighting).to_string()
        } else {
            line.style(unchanged).to_string()
        };

        let extras = line.len() - ansi_width(&line);
        let line_nr = LineWidget::from(line_nr);
        format!("{line_nr}│ {line:<width$}", width = max_left + extras)
    });

    // -----------------------------------------------------------------

    // Build the secondary side
    let secondary_lines: Vec<_> = secondary_doc
        .content
        .lines()
        .skip_while(|line| *line == "---")
        .collect();

    // Find corresponding nodes in secondary document
    // TODO: I think this is more complex than it initially seems.
    //       The goal is to get the spans of the nodes that need to surround the gap.
    //       Therefor I need know what nodes should be there, and then translate
    //       that into the other document. I tend to do that via the `path`
    //
    //       I think that is done?
    //
    //       BUT(!) paths don't necessarily carry over to the other docment.
    //       e.g. if the path to the change is `.people.3`
    //       the surround nodes could be (.people.2, .people.4)
    //       but who knows if the array has sufficient elements?!
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

    let candidate_node_before_change = before_path.and_then(|p| node_in(&secondary_doc.yaml, &p));
    let candidate_node_after_change = after_path.and_then(|p| node_in(&secondary_doc.yaml, &p));

    let gap_start = candidate_node_before_change
        .map(|before| {
            let n = if before.data.is_mapping() || before.data.is_sequence() {
                1
            } else {
                0
            };
            log::debug!("the before line ends on: {}", before.span.end.line());
            log::debug!("weird adjustment factor: {n}");
            log::debug!(
                "the first line of the doc to adjust by is: {}",
                primary_doc.first_line
            );
            before.span.end.line() - n - primary_doc.first_line
        })
        .unwrap_or(Line::new(1).unwrap());

    log::debug!("The gap starts at: {gap_start}");

    // If we can find a node after the change use its line number, other wise guess based on the
    // start of the gap and the length of the change
    let gap_end = if let Some(after_node) = candidate_node_after_change {
        log::debug!(
            "Using start of after_node to find end of gap: {}",
            after_node.span.start.line()
        );
        after_node.span.start.line()
    } else {
        // doing "+1" because keys and values are not on the same line:
        // foo: <--- the key
        //   name: 'abc'
        //   thing: true
        //
        // the node height is 2, but the total thing should be 3
        let height = gap_start.get() + node_height(&changed_yaml) + 1;
        log::debug!("No after_node present, using height of the changed_yaml node: {height}");
        height
    };

    let snippet_start = gap_start - ctx_size;
    let snippet_end = min(gap_end + ctx_size, secondary_lines.len());

    // Create snippet for secondary document
    let snippet = Snippet::try_new(
        &secondary_lines,
        snippet_start,
        Line::new(snippet_end).unwrap(),
    )
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
        format!("{line_nr}│ {line:<width$}", width = max_left + extras)
    });

    let change_size = node_height(&changed_yaml);
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
            ChangeType::Addition => format!("{l}│ {line:<width$}", line = "", width = max_left),
        }
    });

    let post_gap = after_gap.iter().map(|(line_nr, line)| {
        let line = line.style(unchanged).to_string();
        let extras = line.len() - ansi_width(&line);

        let line_nr = LineWidget::from(line_nr);
        format!("{line_nr}│ {line:<width$}", width = max_left + extras)
    });

    let secondary = pre_gap.chain(gap).chain(post_gap);

    // Combine the two sides based on change type
    match change_type {
        ChangeType::Removal => primary
            .zip(secondary)
            .map(|(l, r)| format!("│{l} │ {r}"))
            .collect::<Vec<_>>()
            .join("\n"),
        ChangeType::Addition => secondary
            .zip(primary)
            .map(|(l, r)| format!("│{l} │ {r}"))
            .collect::<Vec<_>>()
            .join("\n"),
    }
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
    path_to_change: Path,
    left: MarkedYamlOwned,
    left_doc: &YamlSource,
    right: MarkedYamlOwned,
    right_doc: &YamlSource,
    max_width: u16,
    color: Color,
) -> String {
    let highlight = if color == Color::Enabled {
        Style::new().bold()
    } else {
        Style::new()
    };
    let title = format!(
        "Changed: {p}:",
        p = highlight.style(path_to_change.jq_like())
    );

    let max_left = ((max_width - 16) / 2) as usize; // includes a bit of random padding, do this proper later
    let left = render_changed_snippet(max_left, left_doc, left, color);
    let right = render_changed_snippet(max_left, right_doc, right, color);

    let body = left
        .iter()
        .zip(right)
        .map(|(l, r)| format!("{l} │ {r}"))
        .collect::<Vec<_>>()
        .join("\n");

    format!("{title}\n{body}")
}

pub fn render_changed_snippet(
    max_width: usize,
    source: &YamlSource,
    changed_yaml: MarkedYamlOwned,
    color: Color,
) -> Vec<String> {
    let start_line_of_document = source.yaml.span.start.line();

    let lines: Vec<_> = source.content.lines().map(|s| s.to_string()).collect();

    let changed_line = changed_yaml.span.start.line() - start_line_of_document;
    let start = changed_line.saturating_sub(5) + 1;
    let end = min(changed_line + 5, lines.len());
    let left_snippet = &lines[start..end];

    let (added, unchaged) = match color {
        Color::Enabled => (
            owo_colors::Style::new().yellow(),
            owo_colors::Style::new().dimmed(),
        ),
        Color::Disabled => (owo_colors::Style::new(), owo_colors::Style::new()),
    };

    left_snippet
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

            let line_nr = LineWidget(Some(line_nr - 1));
            format!("{line_nr}│ {line:<width$}", width = max_width + extras)
        })
        .collect::<Vec<_>>()
}

pub struct LineWidget(pub Option<usize>);

impl fmt::Display for LineWidget {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            None => write!(f, "    "),
            Some(idx) => write!(f, "{:>3} ", idx + 1),
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
    use expect_test::expect;
    use indoc::indoc;
    use saphyr::{LoadableYamlNode, MarkedYamlOwned};

    use crate::{
        YamlSource,
        diff::{ArrayOrdering, Context, Difference, diff},
    };

    use super::{Line, render_added, render_difference, render_removal};

    fn marked_yaml(yaml: &'static str) -> MarkedYamlOwned {
        let mut m = MarkedYamlOwned::load_from_str(yaml).unwrap();
        m.remove(0)
    }

    fn yaml_source(yaml: &'static str) -> YamlSource {
        let doc_separators = yaml.lines().filter(|line| line.starts_with("---")).count();
        let m_yaml = marked_yaml(yaml);
        let first_line = Line::new(m_yaml.span.start.line() - doc_separators).unwrap();
        /* substract one as the block is considered "ended" on the first line that has no content */
        let last_line = Line::new(m_yaml.span.end.line() - doc_separators - 1).unwrap();

        YamlSource {
            file: camino::Utf8PathBuf::new(),
            yaml: m_yaml,
            content: yaml.into(),
            index: 0,
            first_line,
            last_line,
        }
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
        let content = render_difference(
            path,
            left,
            &left_doc,
            right,
            &right_doc,
            80,
            super::Color::Disabled,
        );

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
        let content = render_removal(
            path,
            value,
            &left_doc,
            &right_doc,
            80,
            super::Color::Disabled,
        );

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
        let content = render_added(
            path,
            value,
            &left_doc,
            &right_doc,
            80,
            super::Color::Disabled,
        );

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

    fn init_logging() {
        if std::env::var("LOG").is_ok() {
            env_logger::Builder::new()
                .filter_level(log::LevelFilter::Debug)
                .init();
        }
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

        let mut ctx = Context::default();
        ctx.array_ordering = ArrayOrdering::Dynamic;

        let mut differences = diff(ctx, &left_doc.yaml, &right_doc.yaml);

        let first = differences.remove(0);
        let Difference::Added { path, value } = first else {
            panic!("Should have gotten an Addition");
        };
        let content = render_added(
            path,
            value,
            &left_doc,
            &right_doc,
            80,
            super::Color::Disabled,
        );

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
}
