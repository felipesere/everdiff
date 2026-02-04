use std::{fmt::Write, io::IsTerminal};

use everdiff_diff::{Difference, path::IgnorePath};
use everdiff_multidoc::{AdditionalDoc, DocDifference, MissingDoc, source::YamlSource};
use owo_colors::OwoColorize;

mod inline_diff;
mod node;
mod snippet;
pub mod wrapping;

pub use snippet::{
    Color, LineWidget, RenderContext, gap_start, render_added, render_difference, render_removal,
};

// TODO: Add more output format options (JSON, machine-readable formats, colored HTML output)
pub fn render_multidoc_diff(
    (left, right): (Vec<YamlSource>, Vec<YamlSource>),
    mut differences: Vec<DocDifference>,
    ignore_moved: bool,
    ignore: &[IgnorePath],
    side_by_side: bool,
) {
    if differences.is_empty() {
        println!("No differences found")
    }

    differences.sort();

    for d in differences {
        match d {
            DocDifference::Addition(AdditionalDoc { key, .. }) => {
                println!("{m}", m = "Additional document:".green());
                println!("{key}");
            }
            DocDifference::Missing(MissingDoc { key, .. }) => {
                println!("{m}", m = "Missing document:".red());
                println!("{key}");
            }
            DocDifference::Changed {
                key,
                differences,
                left_doc_idx,
                right_doc_idx,
            } => {
                let differences: Vec<_> = differences
                    .into_iter()
                    .filter(|diff| {
                        !ignore
                            .iter()
                            .any(|path_match| path_match.matches(diff.path()))
                    })
                    .collect();

                let differences = if !ignore_moved {
                    differences
                } else {
                    differences
                        .into_iter()
                        .filter(|diff| !matches!(diff, Difference::Moved { .. }))
                        .collect()
                };

                println!();
                println!("{}", "Changed document:".bold().underline());
                println!("{key}");
                let actual_left_doc = &left[left_doc_idx];
                let actual_right_doc = &right[right_doc_idx];
                let max_width = if std::io::stdout().is_terminal() {
                    // Format for terminal
                    terminal_size::terminal_size()
                        .map(|(terminal_size::Width(n), _)| n)
                        .unwrap_or(80)
                } else {
                    // When piped, assume wider or no limit
                    terminal_size::terminal_size_of(std::io::stderr())
                        .map(|(terminal_size::Width(n), _)| n)
                        .unwrap_or(80)
                };

                let ctx = RenderContext::new(max_width, Color::Enabled);
                print!(
                    "{}",
                    render(
                        ctx,
                        actual_left_doc,
                        actual_right_doc,
                        differences,
                        side_by_side
                    )
                );
            }
        }
    }
}

pub fn render(
    ctx: RenderContext,
    left_doc: &YamlSource,
    right_doc: &YamlSource,
    differences: Vec<Difference>,
    _side_by_side: bool,
) -> String {
    let mut buf = String::new();
    for d in differences {
        match d {
            Difference::Added { path, value } => {
                let p = if ctx.color == Color::Enabled {
                    path.jq_like().bold().to_string()
                } else {
                    path.jq_like()
                };
                writeln!(&mut buf, "Added: {p}:").unwrap();

                let added = render_added(&ctx, path, value, left_doc, right_doc);
                writeln!(&mut buf, "{added}").unwrap();
            }
            Difference::Removed { path, value } => {
                writeln!(&mut buf, "Removed: {p}:", p = path.jq_like()).unwrap();
                let output = render_removal(&ctx, path, value, left_doc, right_doc);
                writeln!(&mut buf, "{output}").unwrap();
            }
            Difference::Changed { path, left, right } => {
                let combined = render_difference(&ctx, path, left, left_doc, right, right_doc);
                writeln!(&mut buf, "{combined}").unwrap();
            }
            Difference::Moved {
                original_path,
                new_path,
            } => {
                writeln!(
                    &mut buf,
                    "Moved: from {p} to {q}:",
                    p = original_path.jq_like().yellow(),
                    q = new_path.jq_like().yellow()
                )
                .unwrap();
            }
        }
        writeln!(&mut buf).unwrap()
    }
    buf
}
