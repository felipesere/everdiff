use std::{fmt::Write, io::Read};

// TODO: Replace anyhow with structured error types for better error handling and user experience
use camino::Utf8PathBuf;
use diff::Difference;
use multidoc::{AdditionalDoc, DocDifference, MissingDoc};
use owo_colors::{OwoColorize, Style};
use path::IgnorePath;
use saphyr::LoadableYamlNode;
use snippet::{
    Color, Line, LineWidget, RenderContext, render_added, render_difference, render_removal,
};
// used in the linenums binary
pub use source::YamlSource;

pub mod config;
pub mod diff;
pub mod identifier;
pub mod multidoc;
pub mod node;
pub mod path;
pub mod prepatch;
pub mod snippet;
pub mod source;

// TODO: Optimize memory usage for large files - consider streaming approach instead of loading all into memory
pub fn read_and_patch(
    paths: &[camino::Utf8PathBuf],
    patches: &[prepatch::PrePatch],
) -> anyhow::Result<Vec<YamlSource>> {
    let mut docs = Vec::new();
    for p in paths {
        let mut f = std::fs::File::open(p)?;
        let mut content = String::new();
        f.read_to_string(&mut content)?;

        let n = read_doc(content, p.clone())?;

        docs.extend(n.into_iter());
    }
    for patch in patches {
        let _err = patch.apply_to(&mut docs);
    }

    Ok(docs)
}

pub fn read_doc(content: impl Into<String>, path: Utf8PathBuf) -> anyhow::Result<Vec<YamlSource>> {
    let content = content.into();
    let mut docs = Vec::new();
    let raw_docs: Vec<_> = content
        .clone()
        .split("---")
        .filter(|doc| !doc.is_empty())
        .map(|doc| doc.trim_start().to_string())
        .collect();

    let parsed_docs = saphyr::MarkedYamlOwned::load_from_str(&content)?;

    for (index, (document, content)) in parsed_docs.into_iter().zip(raw_docs).enumerate() {
        let start = document.span.start.line();
        let end = document.span.end.line();

        log::debug!("start: {start} and end {end}");

        let first_line = Line::one();
        // the span ends when the indenation no longer matches, which is the line _after_ the the
        // last properly indented line
        let last_line = Line::new(end - start).unwrap();

        docs.push(YamlSource {
            file: path.clone(),
            yaml: document,
            start,
            end,
            first_line,
            last_line,
            content,
            index,
        });
    }
    Ok(docs)
}

// TODO: Add more output format options (JSON, machine-readable formats, colored HTML output)
pub fn render_multidoc_diff(
    (left, right): (Vec<YamlSource>, Vec<YamlSource>),
    mut differences: Vec<DocDifference>,
    ignore_moved: bool,
    ignore: &[IgnorePath],
    side_by_side: bool,
) {
    use owo_colors::OwoColorize;

    if differences.is_empty() {
        println!("No differences found")
    }

    differences.sort();

    for d in differences {
        match d {
            DocDifference::Addition(AdditionalDoc { key, .. }) => {
                let key = indent::indent_all_by(4, key.pretty_print());
                println!("{m}", m = "Additional document:".green());
                println!("{key}");
            }
            DocDifference::Missing(MissingDoc { key, .. }) => {
                let key = indent::indent_all_by(4, key.pretty_print());
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

                let key = indent::indent_all_by(4, key.pretty_print());
                println!("Changed document:");
                println!("{key}");
                let actual_left_doc = &left[left_doc_idx];
                let actual_right_doc = &right[right_doc_idx];

                let max_width = termsize::get().unwrap().cols;
                let ctx = RenderContext::new(max_width, snippet::Color::Enabled);
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

//fn stringify(yaml: &MarkedYamlOwned) -> String {
//    let mut out_str = String::new();
//    let mut emitter = saphyr::YamlEmitter::new(&mut out_str);
//    emitter.dump(&yaml).expect("failed to write YAML to buffer");
//    match out_str.find('\n') {
//        Some(pos) => out_str[pos + 1..].to_string(),
//        None => out_str,
//    }
//}

pub fn render(
    ctx: RenderContext,
    left_doc: &YamlSource,
    right_doc: &YamlSource,
    differences: Vec<Difference>,
    _side_by_side: bool,
) -> String {
    use owo_colors::OwoColorize;
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

#[allow(dead_code)]
fn render_string_diff(left: &str, right: &str) {
    let diff = similar::TextDiff::from_lines(left, right);

    for (idx, group) in diff.grouped_ops(2).iter().enumerate() {
        if idx > 0 {
            println!("{:┈^1$}", "┈", 80);
        }
        for op in group {
            for change in diff.iter_inline_changes(op) {
                let (sign, emphasis_style) = match change.tag() {
                    similar::ChangeTag::Delete => ("-", Style::new().red()),
                    similar::ChangeTag::Insert => ("+", Style::new().green()),
                    similar::ChangeTag::Equal => (" ", Style::new().dimmed()),
                };
                print!(
                    "{}{} {}│  ",
                    LineWidget(change.old_index()).to_string().dimmed(),
                    LineWidget(change.new_index()).to_string().dimmed(),
                    sign.style(emphasis_style).bold(),
                );
                for (emphasized, value) in change.iter_strings_lossy() {
                    if emphasized {
                        print!("{}", value.style(emphasis_style.underline()));
                    } else {
                        print!("{value}");
                    }
                }
                if change.missing_newline() {
                    println!();
                }
            }
        }
    }
}
