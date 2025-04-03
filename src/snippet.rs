use std::{
    cmp::min,
    fmt::{self},
    num::NonZeroUsize,
};

use ansi_width::ansi_width;
use anyhow::Context;
use owo_colors::{OwoColorize, Style};
use saphyr::{MarkedYaml, YamlData};

use crate::{YamlSource, path::Path};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Color {
    Enabled,
    // mostly used in tests
    #[allow(dead_code)]
    Disabled,
}

pub type Line = NonZeroUsize;

impl From<Line> for LineWidget {
    fn from(value: Line) -> Self {
        // TODO: We still do gross `+1` math in here
        // if the `Line` concept pans out we can clear it
        Self(Some(value.get() - 1))
    }
}

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
        if lines.len() < usize::from(to) {
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
            from: split_at.saturating_add(1),
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
    removal: MarkedYaml,
    left_doc: &YamlSource,
    right_doc: &YamlSource,
    max_width: u16,
    color: Color,
) -> String {
    let ctx_size = 5;
    let highlight = if color == Color::Enabled {
        Style::new().bold()
    } else {
        Style::new()
    };
    let title = format!(
        "Removed: {p}:",
        p = highlight.style(path_to_change.jq_like())
    );

    let max_left = ((max_width - 16) / 2) as usize; // includes a bit of random padding, do this proper later

    let parent = path_to_change.parent().unwrap();
    let parent_node = node_in(&left_doc.yaml, &parent).unwrap();

    let (before, after) = match &parent_node.data {
        YamlData::Array(_) => todo!("not dealing with arrays yet"),
        YamlData::Hash(linked_hash_map) => {
            // Consider extracting this...
            let target_key = path_to_change.head().unwrap();
            let keys: Vec<_> = linked_hash_map.keys().collect();
            if let Some(idx) = keys.iter().position(|k| &k.data == target_key) {
                let before = if idx > 0 { Some(keys[idx - 1]) } else { None };
                let after = if idx < keys.len() - 1 {
                    Some(keys[idx + 1])
                } else {
                    None
                };
                (before, after)
            } else {
                (None, None)
            }
        }
        _ => unreachable!("parent has to be a container"),
    };

    let start_line_of_left_document = left_doc.yaml.span.start.line();
    let left_lines: Vec<_> = left_doc
        .content
        .lines()
        .skip_while(|s| *s == "---")
        .collect();

    let removal_start = Line::new(removal.span.start.line() - start_line_of_left_document + 1)
        .expect("removed line start");
    let removal_end = Line::new(removal.span.end.line() - start_line_of_left_document + 1)
        .expect("removed line end");

    let start = checked_sub(removal_start, ctx_size);
    let end = min(
        removal_end.checked_add(ctx_size),
        Line::new(left_lines.len()),
    )
    .expect("either one of them should be positive");

    let left_snippet =
        Snippet::try_new(&left_lines, start, end).expect("Left snippet could not be created");

    let (delete, unchaged) = match color {
        Color::Enabled => (
            owo_colors::Style::new().red(),
            owo_colors::Style::new().dimmed(),
        ),
        Color::Disabled => (owo_colors::Style::new(), owo_colors::Style::new()),
    };

    let removal_range = removal_start..=removal_end;
    let left = left_snippet.iter().map(|(line_nr, line)| {
        let line = if removal_range.contains(&line_nr) {
            line.style(delete).to_string()
        } else {
            line.style(unchaged).to_string()
        };

        // Why are we adding "extras"?
        // The line may contain non-printable color codes which count for the padding
        // in format!(...) but don't add to the width on the terminal.
        // To accomodate, we pretend to make the padding wider again
        // because we know some of the width won't be visible.
        let extras = line.len() - ansi_width(&line);

        let line_nr = LineWidget::from(line_nr);
        format!("{line_nr}│ {line:<width$}", width = max_left + extras)
    });

    let before = before
        .map(|key| parent.push(key.data.clone()))
        .and_then(|path| node_in(&right_doc.yaml, &path));

    let after = after
        .map(|key| parent.push(key.data.clone()))
        .and_then(|path| node_in(&right_doc.yaml, &path));

    //-----------------------
    // now we build the right
    //-----------------------

    let right_lines: Vec<_> = right_doc
        .content
        .lines()
        .skip_while(|line| *line == "---")
        .map(|s| s.to_string())
        .collect();

    let gap_start = before.map(|n| n.span.end.line() - 1).unwrap_or(0);
    let gap_end = after.map(|n| n.span.start.line()).unwrap_or(100); // TODO: what is the correct default here?

    let snippet_start = gap_start.saturating_sub(ctx_size) + 1;
    let snippet_end = min(gap_end + ctx_size, right_lines.len());

    let lines: Vec<_> = right_doc
        .content
        .lines()
        .skip_while(|line| *line == "---")
        .collect();
    let snippet = Snippet::try_new(
        &lines,
        Line::new(snippet_start).unwrap(),
        Line::new(snippet_end).unwrap(),
    )
    .with_context(|| {
        format!(
            "Failed to create a snippet for change {} in  {}:{}",
            path_to_change.jq_like(),
            right_doc.file,
            right_doc.index,
        )
    })
    .unwrap();

    let (before_gap, after_gap) = snippet.split(Line::new(gap_start).unwrap());

    let removal_size = removal.span.end.line() - removal.span.start.line();

    let pre_gap = before_gap.iter().map(|(line_nr, line)| {
        let line = line.style(unchaged).to_string();
        let extras = line.len() - ansi_width(&line);

        let line_nr = LineWidget::from(line_nr);
        format!("{line_nr}│ {line:<width$}", width = max_left + extras)
    });

    let gap = (0..=removal_size).map(|_| {
        let l = LineWidget(None);
        format!("{l}│")
    });

    let post_gap = after_gap.iter().map(|(line_nr, line)| {
        let line = line.style(unchaged).to_string();
        let extras = line.len() - ansi_width(&line);

        let line_nr = LineWidget::from(line_nr);
        format!("{line_nr}│ {line:<width$}", width = max_left + extras)
    });

    let right = pre_gap.chain(gap).chain(post_gap);
    let body = left
        .zip(right)
        .map(|(l, r)| format!("{l} │ {r}"))
        .collect::<Vec<_>>()
        .join("\n");

    format!("{title}\n{body}")
}

fn checked_sub(removal_start: Line, ctx_size: usize) -> Line {
    let n = removal_start.get();
    n.checked_sub(ctx_size)
        .and_then(Line::new)
        .or_else(|| Line::new(1))
        .unwrap() // this is safe...
}

pub fn render_difference(
    path_to_change: Path,
    left: MarkedYaml,
    left_doc: &YamlSource,
    right: MarkedYaml,
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
    changed_yaml: MarkedYaml,
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
            owo_colors::Style::new().green(),
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

fn node_in<'y>(yaml: &'y MarkedYaml, path: &Path) -> Option<&'y MarkedYaml> {
    let mut n = Some(yaml);
    for p in path.segments() {
        match p {
            crate::path::Segment::Field(f) => {
                let v = n.and_then(|n| n.get(f))?;
                n = Some(v);
            }
            crate::path::Segment::Index(nr) => {
                let v = n.and_then(|n| n.get(nr))?;
                n = Some(v);
            }
        }
    }
    n
}

#[cfg(test)]
mod test {
    use expect_test::expect;
    use indoc::indoc;
    use saphyr::MarkedYaml;

    use crate::{
        YamlSource,
        diff::{Context, Difference, diff},
    };

    use super::{Line, render_difference, render_removal};

    fn marked_yaml(yaml: &'static str) -> MarkedYaml {
        let mut m = MarkedYaml::load_from_str(yaml).unwrap();
        m.remove(0)
    }

    fn yaml_source(yaml: &'static str) -> YamlSource {
        YamlSource {
            file: camino::Utf8PathBuf::new(),
            yaml: marked_yaml(yaml),
            content: yaml.into(),
            index: 0,
            first_line: Line::new(2).unwrap(),
            // we substract the `---` at the top?
            last_line: Line::new(yaml.lines().count() - 1).unwrap(),
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
            Removed: .person.address:
              1 │ person:                          │   1 │ person:                         
              2 │   name: Robert Anderson          │   2 │   name: Robert Anderson         
              3 │   address:                       │     │
              4 │     street: foo bar              │     │
              5 │     nr: 1                        │     │
              6 │     postcode: ABC123             │     │
              7 │   age: 12                        │   3 │   age: 12                       
              8 │   foo: bar                       │   4 │   foo: bar                      "#]]
        .assert_eq(content.as_str());
    }
}
