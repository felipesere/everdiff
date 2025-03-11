use std::{cmp::min, fmt};

use ansi_width::ansi_width;
use owo_colors::{OwoColorize, Style};
use saphyr::MarkedYaml;

use crate::{YamlSource, path::Path};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Color {
    Enabled,
    Disabled,
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
    let left = render_snippet(max_left, left_doc, left, color);
    let right = render_snippet(max_left, right_doc, right, color);

    let body = left
        .iter()
        .zip(right)
        .map(|(l, r)| format!("{l} │ {r}"))
        .collect::<Vec<_>>()
        .join("\n");

    format!("{title}\n{body}")
}

pub fn render_snippet(
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

#[cfg(test)]
mod test {
    use expect_test::expect;
    use indoc::indoc;
    use saphyr::MarkedYaml;

    use crate::{
        YamlSource,
        diff::{Context, Difference, diff},
    };

    use super::render_difference;

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
}
