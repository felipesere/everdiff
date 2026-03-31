use std::{
    io::{IsTerminal, Write},
    sync::Arc,
};

use everdiff_diff::{Difference, path::IgnorePath};
use everdiff_layout::{Column, ColumnPair, Highlighted, InlineParts};
use everdiff_multidoc::{AdditionalDoc, DocDifference, MissingDoc, source::YamlSource};
use owo_colors::OwoColorize;

mod inline_diff;
mod node;
mod snippet;

pub use snippet::{
    Highlight, LineWidget, RenderContext, Theme, gap_start, render_added, render_difference,
    render_removal,
};

// TODO: Add more output format options (JSON, machine-readable formats, colored HTML output)
#[allow(clippy::too_many_arguments)]
pub fn render_multidoc_diff<W: Write>(
    (left, right): (Vec<YamlSource>, Vec<YamlSource>),
    mut differences: Vec<DocDifference>,
    ignore_moved: bool,
    ignore: &[IgnorePath],
    word_wise_diff: bool,
    lines_before: usize,
    lines_after: usize,
    writer: &mut W,
) -> std::io::Result<()> {
    if differences.is_empty() {
        writeln!(writer, "No differences found")?;
    }

    // WARN: Go through these numbers at some point...
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
    } - 10;

    differences.sort();

    for d in differences {
        match d {
            DocDifference::Addition(AdditionalDoc { fields, .. }) => {
                writeln!(writer, "{m}", m = "Additional document:".green())?;
                writeln!(writer, "{fields}")?;
            }
            DocDifference::Missing(MissingDoc { fields, .. }) => {
                writeln!(writer, "{m}", m = "Missing document:".red())?;
                writeln!(writer, "{fields}")?;
            }
            DocDifference::Changed {
                left: l,
                right: r,
                fields,
                differences,
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

                {
                    let dimmed = Arc::new(Box::new(|s: &str| s.dimmed().to_string()));
                    let bold_underline =
                        Arc::new(Box::new(|s: &str| s.bold().underline().to_string()));

                    let header_pair = ColumnPair::new(max_width);
                    let mut left = header_pair.column();
                    let mut right = header_pair.column();
                    let mut inline_style = InlineParts::new();
                    inline_style.push("Changed document", bold_underline);
                    // left.new_push(Highlighted::new("Changed document:", bold_underline)); // this is meh
                    left.push(inline_style);
                    right.append_blank(1);

                    left.push(l.0.to_string());
                    right.push(r.0.to_string());

                    left.append_blank(1);
                    right.append_blank(1);

                    for (k, v) in &fields.0 {
                        if let Some(v) = v {
                            left.push(Highlighted::new(format!("{k} -> {v}"), dimmed.clone()));
                        }
                    }
                    left.append_blank(1);
                    right.append_blank(1 + fields.0.len());

                    for l in header_pair.zip(left, right) {
                        writeln!(writer, "{l}")?;
                    }
                }

                let actual_left_doc = &left[l.1];
                let actual_right_doc = &right[r.1];

                let ctx = RenderContext::new(max_width, word_wise_diff, lines_before, lines_after);
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

#[cfg(test)]
mod test {
    use everdiff_diff::{ArrayOrdering, Context, diff};
    use everdiff_layout::ColumnPair;
    use everdiff_multidoc::source::{YamlSource, read_doc};
    use expect_test::expect;
    use indoc::indoc;
    use tracing_test::traced_test;

    use crate::{RenderContext, Theme, render};

    fn yaml_source(yaml: &'static str) -> YamlSource {
        let mut docs =
            read_doc(yaml, &camino::Utf8PathBuf::new()).expect("to have parsed properly");
        docs.remove(0)
    }

    #[traced_test]
    #[test]
    fn why_does_this_not_align() {
        let max_width = 100;

        let header_pair = ColumnPair::new(max_width);
        let mut left = header_pair.column();
        let mut right = header_pair.column();
        left.push("Changed document");
        right.append_blank(1);

        left.push("left file path...");
        right.push("right file path...");

        let mut ctx = RenderContext::new(max_width, false, 2, 2);
        ctx.theme = Theme::plain();
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

        let content = render(ctx, &left_doc, &right_doc, differences);

        let rendered = header_pair.zip(left, right).join("\n");

        let complete = format!("{rendered}\n{content}\n");

        expect![[r#"
            Changed document                                                                                    
            left file path...                                 right file path...                                
            Changed: .servers[1].port:
            │   3 │     port: 8080                            │    3 │     port: 8080                          │
            │   4 │   - host: server2.example.com             │    4 │   - host: server2.example.com           │
            │   5 │     port: 9090                            │    5 │     port: 9091                          │


        "#]]
        .assert_eq(&complete);
    }
}
