use std::{io::IsTerminal, io::Write};

use everdiff_diff::{Difference, path::IgnorePath};
use everdiff_multidoc::{AdditionalDoc, DocDifference, MissingDoc, source::YamlSource};
use owo_colors::OwoColorize;

mod inline_diff;
mod node;
mod snippet;
pub mod wrapping;

pub use snippet::{
    Highlight, LineWidget, RenderContext, Theme, gap_start, render_added, render_difference,
    render_removal,
};

// TODO: Add more output format options (JSON, machine-readable formats, colored HTML output)
pub fn render_multidoc_diff<W: Write>(
    (left, right): (Vec<YamlSource>, Vec<YamlSource>),
    mut differences: Vec<DocDifference>,
    ignore_moved: bool,
    ignore: &[IgnorePath],
    word_wise_diff: bool,
    writer: &mut W,
) -> std::io::Result<()> {
    if differences.is_empty() {
        writeln!(writer, "No differences found")?;
    }

    differences.sort();

    for d in differences {
        match d {
            DocDifference::Addition(AdditionalDoc { key, .. }) => {
                writeln!(writer, "{m}", m = "Additional document:".green())?;
                writeln!(writer, "{key}")?;
            }
            DocDifference::Missing(MissingDoc { key, .. }) => {
                writeln!(writer, "{m}", m = "Missing document:".red())?;
                writeln!(writer, "{key}")?;
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
                        diff.path().is_none_or(|path| {
                            !ignore.iter().any(|path_match| path_match.matches(path))
                        })
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

                writeln!(writer)?;
                writeln!(writer, "{}", "Changed document:".bold().underline())?;
                writeln!(writer, "{key}")?;
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

                let ctx = RenderContext::new(max_width, word_wise_diff);
                write!(
                    writer,
                    "{}",
                    render(ctx, actual_left_doc, actual_right_doc, differences)
                )?;
            }
        }
    }
    Ok(())
}

pub fn render(
    ctx: RenderContext,
    left_doc: &YamlSource,
    right_doc: &YamlSource,
    differences: Vec<Difference>,
) -> String {
    use std::fmt::Write;
    let mut buf = String::new();
    for d in differences {
        match d {
            Difference::Added { path, value } => {
                writeln!(&mut buf, "Added: {}:", ctx.theme.header(&path.to_string())).unwrap();

                let added = render_added(&ctx, path, value, left_doc, right_doc);
                writeln!(&mut buf, "{added}").unwrap();
            }
            Difference::Removed { path, value } => {
                writeln!(&mut buf, "Removed: {path}:").unwrap();
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
                    p = ctx.theme.changed(&original_path.to_string()),
                    q = ctx.theme.changed(&new_path.to_string())
                )
                .unwrap();
            }
        }
        writeln!(&mut buf).unwrap()
    }
    buf
}
