use std::{
    cmp::min,
    fmt::{self, format},
};

use ansi_width::ansi_width;
use owo_colors::{OwoColorize, Style};
use saphyr::{MarkedYaml, YamlData};

use crate::{YamlSource, path::Path};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Color {
    Enabled,
    Disabled,
}

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

    let start_line_of_document = left_doc.yaml.span.start.line();
    let left_lines: Vec<_> = left_doc.content.lines().map(|s| s.to_string()).collect();
    let removal_start = removal.span.start.line() - start_line_of_document;
    let removal_end = removal.span.end.line() - start_line_of_document;
    let start = removal_start.saturating_sub(ctx_size) + 1;
    let end = min(removal_end + ctx_size, left_lines.len());
    let left_snippet = &left_lines[start..end];

    let (delete, unchaged) = match color {
        Color::Enabled => (
            owo_colors::Style::new().red(),
            owo_colors::Style::new().dimmed(),
        ),
        Color::Disabled => (owo_colors::Style::new(), owo_colors::Style::new()),
    };

    let left = left_snippet.iter().zip(start..end).map(|(line, line_nr)| {
        let line = if (removal_start..=removal_end).contains(&line_nr) {
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

        let line_nr = Line(Some(line_nr - 1));
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

    let start_line_of_right_document = right_doc.yaml.span.start.line();

    let right_lines: Vec<_> = right_doc.content.lines().map(|s| s.to_string()).collect();
    let gap_start = before.map(|n| n.span.end.line() - 1).unwrap_or(0);
    let gap_end = after.map(|n| n.span.start.line()).unwrap_or(100); // TODO: what is the correct default here?

    let snippet_start = gap_start.saturating_sub(ctx_size) + 1;
    let snippet_end = min(gap_end + ctx_size, right_lines.len());
    let right_snippet = &right_lines[snippet_start..snippet_end];

    let removal_size = removal.span.end.line() - removal.span.start.line();

    let pre_gap = right_snippet
        .iter()
        .zip(snippet_start..gap_start)
        .map(|(line, line_nr)| {
            let line = line.style(unchaged).to_string();
            let extras = line.len() - ansi_width(&line);

            let line_nr = Line(Some(line_nr));
            format!("{line_nr}│ {line:<width$}", width = max_left + extras)
        });

    let gap = (0..=removal_size).map(|_| {
        let l = Line(None);
        format!("{l}│")
    });

    let post_gap = right_snippet
        .iter()
        .skip(ctx_size - 1)
        .zip(gap_start..snippet_end)
        .map(|(line, line_nr)| {
            let line = line.style(unchaged).to_string();
            let extras = line.len() - ansi_width(&line);

            let line_nr = Line(Some(line_nr));
            format!("{line_nr}│ {line:<width$}", width = max_left + extras)
        });

    let right = pre_gap.chain(gap).chain(post_gap);
    let x = left
        .zip(right)
        .map(|(l, r)| format!("{l} │ {r}"))
        .collect::<Vec<_>>()
        .join("\n");

    format!("{title}\n{x}")
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

            let line_nr = Line(Some(line_nr - 1));
            format!("{line_nr}│ {line:<width$}", width = max_width + extras)
        })
        .collect::<Vec<_>>()
}

pub struct Line(pub Option<usize>);

impl fmt::Display for Line {
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

fn surrounding_nodes<'y>(
    yaml: &'y MarkedYaml,
    path: &Path,
) -> (Option<&'y MarkedYaml>, Option<&'y MarkedYaml>) {
    let target = node_in(yaml, path);

    (None, None)
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

    use super::{render_difference, render_removal};

    fn marked_yaml(yaml: &'static str) -> MarkedYaml {
        let mut m = MarkedYaml::load_from_str(yaml).unwrap();
        m.remove(0)
    }

    fn yaml_source(yaml: &'static str) -> YamlSource {
        YamlSource {
            file: camino::Utf8PathBuf::new(),
            yaml: marked_yaml(yaml),
            content: yaml.into(),
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
              8 │   foo: bar                       │   4 │   foo: bar"#]]
        .assert_eq(content.as_str());
    }
}
