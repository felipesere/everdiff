use std::cmp::min;

use owo_colors::OwoColorize;
use saphyr::MarkedYaml;

use crate::{YamlSource, path::Path};

pub fn render_difference(
    path_to_change: Path,
    left: MarkedYaml,
    left_doc: &YamlSource,
    right: MarkedYaml,
    right_doc: &YamlSource,
    max_width: u16,
) -> String {
    println!("Changed: {p}:", p = path_to_change.jq_like().bold());

    let max_left = ((max_width - 8) / 2) as usize; // includes a bit of random padding, do this proper later
    let left = render_snippet(max_left, left_doc, left);
    let right = render_snippet(max_left, right_doc, right);

    left.iter()
        .zip(right)
        .map(|(l, r)| format!("{l} │ {r}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn render_snippet(
    max_width: usize,
    source: &YamlSource,
    changed_yaml: MarkedYaml,
) -> Vec<String> {
    let start_line_of_document = source.yaml.span.start.line();

    let lines: Vec<_> = source.content.lines().map(|s| s.to_string()).collect();

    let changed_line = changed_yaml.span.start.line() - start_line_of_document;
    let start = changed_line.saturating_sub(5) + 1;
    let end = min(changed_line + 5, lines.len());
    let left_snippet = &lines[start..end];

    left_snippet
        .iter()
        .zip(start..end)
        .map(|(line, nr)| {
            let (w, line) = if nr == changed_line + 1 {
                // TODO: Why do I need to make this wider?
                (max_width + 2, OwoColorize::green(&line).to_string())
            } else {
                (max_width, OwoColorize::dimmed(&line).to_string())
            };

            format!("{nr:<3}│ {line:<w$}")
        })
        .collect::<Vec<_>>()
}
