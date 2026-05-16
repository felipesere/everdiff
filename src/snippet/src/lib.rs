use std::{io::Write, sync::Arc};

use everdiff_diff::{Difference, path::IgnorePath};
use everdiff_layout::{ColumnPair, Highlighted, InlineParts};
use everdiff_multidoc::{
    AdditionalDoc, DocDifference, DocumentRef, Fields, MissingDoc, source::YamlSource,
};
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
    differences: Vec<DocDifference>,
    ignore_moved: bool,
    ignore: &[IgnorePath],
    word_wise_diff: bool,
    lines_before: usize,
    lines_after: usize,
    writer: &mut W,
    max_width: u16,
) -> std::io::Result<()> {
    if differences.is_empty() {
        writeln!(writer, "No differences found")?;
    }

    for d in differences {
        match d {
            DocDifference::Addition(AdditionalDoc { fields, doc }) => {
                let pair = ColumnPair::new(max_width);
                let mut left = pair.column();
                let mut right = pair.column();
                right.push(Highlighted::new(
                    "Additional document:",
                    Arc::new(|s: &str| s.green().to_string()),
                ));
                right.push(format!("{} [{}]", doc.0, doc.1));
                for (k, v) in fields.as_ref() {
                    right.push(format!("{k} -> {}", v.as_deref().unwrap_or("∅")));
                }
                left.append_blank(2 + fields.len());
                for l in pair.zip(left, right) {
                    writeln!(writer, "{l}")?;
                }
                writeln!(writer)?;
            }
            DocDifference::Missing(MissingDoc { fields, doc }) => {
                let pair = ColumnPair::new(max_width);
                let mut left = pair.column();
                let mut right = pair.column();
                left.push(Highlighted::new(
                    "Missing document:",
                    Arc::new(|s: &str| s.red().to_string()),
                ));
                left.push(format!("{} [{}]", doc.0, doc.1));
                for (k, v) in fields.as_ref() {
                    left.push(format!("{k} -> {}", v.as_deref().unwrap_or("∅")));
                }
                right.append_blank(2 + fields.len());
                for l in pair.zip(left, right) {
                    writeln!(writer, "{l}")?;
                }
                writeln!(writer)?;
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

                let header = changed_header(&l, &r, fields, max_width).join("\n");
                writeln!(writer, "{header}")?;

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

fn changed_header(l: &DocumentRef, r: &DocumentRef, fields: Fields, max_width: u16) -> Vec<String> {
    // we resever one character of space to add a "┃" for sytling
    let usable_width = max_width - 1;
    let dimmed = Arc::new(Box::new(|s: &str| s.dimmed().to_string()));
    let bold_underline = Arc::new(Box::new(|s: &str| s.bold().underline().to_string()));

    let header_pair = ColumnPair::new(usable_width);
    let mut left = header_pair.column();
    let mut right = header_pair.column();
    let mut inline_style = InlineParts::new();
    inline_style.push("Changed document", bold_underline);
    left.push(inline_style);
    right.append_blank(1);

    // TODO: we should make DocumentRef a proper struct with a Display impl
    left.push(format!("{}:{}", l.0, l.1));
    right.push(format!("{}:{}", r.0, r.1));

    for (k, v) in fields.as_ref() {
        if let Some(v) = v {
            left.push(Highlighted::new(format!("{k} -> {v}"), dimmed.clone()));
        }
    }
    right.append_blank(fields.len());

    let mut content: Vec<_> = header_pair
        .zip(left, right)
        .iter()
        .map(|l| format!("{l} ┃"))
        .collect();
    let line = "━".repeat(usable_width as usize);
    content.insert(0, format!("{line}┓"));
    content.push(format!("{line}┛"));

    content
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
                let added = render_added(&ctx, path, value, left_doc, right_doc);
                writeln!(&mut buf, "{added}").unwrap();
            }
            Difference::Removed { path, value } => {
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
                let pair = ColumnPair::new(ctx.max_width);
                let mut left = pair.column();
                let mut right = pair.column();
                left.push(format!(
                    "Moved: from {}",
                    ctx.theme.changed(&original_path.to_string())
                ));
                right.push(format!("to {}:", ctx.theme.changed(&new_path.to_string())));
                for line in pair.zip(left, right) {
                    writeln!(&mut buf, "{line}").unwrap();
                }
            }
        }
        writeln!(&mut buf).unwrap()
    }
    buf
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use camino::Utf8PathBuf;
    use everdiff_diff::{ArrayOrdering, Context, diff};
    use everdiff_layout::ColumnPair;
    use everdiff_multidoc::{
        Fields,
        source::{YamlSource, read_doc},
    };
    use expect_test::expect;
    use indoc::indoc;
    use tracing_test::traced_test;

    use crate::{RenderContext, Theme, changed_header, render};

    fn yaml_source(yaml: &'static str) -> YamlSource {
        let mut docs =
            read_doc(yaml, &camino::Utf8PathBuf::new()).expect("to have parsed properly");
        docs.remove(0)
    }

    #[test]
    fn render_changed_header() {
        let l = (
            Utf8PathBuf::from_str("left/examples/deployment.flux-engine-steam.yaml").unwrap(),
            0usize,
        );
        let r = (
            Utf8PathBuf::from_str("right/examples/deployment.flux-engine-steam.yaml").unwrap(),
            0usize,
        );
        let fields = Fields::default();
        let width = 160;
        let lines = changed_header(&l, &r, fields, width);

        let actual = lines.join("\n");
        expect![[r#"
            ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
            [4m[1mChanged document[0m[0m                                                                                                                                               ┃
            left/examples/deployment.flux-engine-steam.yaml:0                              right/examples/deployment.flux-engine-steam.yaml:0                              ┃
            ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛"#]].assert_eq(&actual);
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
            │   3 │     port: 8080                            │   3 │     port: 8080                            
            │   4 │   - host: server2.example.com             │   4 │   - host: server2.example.com             
            │   5 │     port: 9090                            │   5 │     port: 9091                            


        "#]]
        .assert_eq(&complete);
    }
}
