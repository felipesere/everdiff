use std::io::Read;

// TODO: Replace anyhow with structured error types for better error handling and user experience
use camino::Utf8PathBuf;
use diff::Difference;
use multidoc::{AdditionalDoc, DocDifference, MissingDoc};
use owo_colors::{OwoColorize, Style};
use path::IgnorePath;
use saphyr::{LoadableYamlNode, MarkedYamlOwned};
use snippet::{Line, LineWidget, RenderContext, render_added, render_difference, render_removal};

pub mod config;
pub mod diff;
pub mod identifier;
pub mod multidoc;
pub mod path;
pub mod prepatch;
pub mod snippet;

#[derive(Debug)]
pub struct YamlSource {
    pub file: camino::Utf8PathBuf,
    pub yaml: saphyr::MarkedYamlOwned,
    pub content: String,
    pub index: usize,
    pub first_line: Line,
    pub last_line: Line,
}

impl YamlSource {
    pub fn lines(&self) -> Vec<&str> {
        self.content
            .lines()
            .skip_while(|line| *line == "---" || line.is_empty())
            .collect()
    }
}

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
        .map(|c| c.to_string())
        .collect();

    let parsed_docs = saphyr::MarkedYamlOwned::load_from_str(&content)?;

    for (index, (document, content)) in parsed_docs.into_iter().zip(raw_docs).enumerate() {
        let first = first_node(&document).unwrap();

        let first_line = Line::new(first.span.start.line()).unwrap();
        let last_line = Line::new(document.span.end.line()).unwrap();
        // last_line_in_node(&document).unwrap() - 1;

        docs.push(YamlSource {
            file: path.clone(),
            yaml: document,
            first_line,
            last_line,
            content,
            index,
        });
    }
    Ok(docs)
}

// These need a better home
pub fn first_node(doc: &MarkedYamlOwned) -> Option<&MarkedYamlOwned> {
    match &doc.data {
        saphyr::YamlDataOwned::Sequence(vec) => vec.first(),
        saphyr::YamlDataOwned::Mapping(hash) => hash.front().map(|(k, _)| k),
        _ => Some(doc),
    }
}

// These need a better home
pub fn last_line_in_node(node: &MarkedYamlOwned) -> Option<Line> {
    match &node.data {
        saphyr::YamlDataOwned::Sequence(vec) => {
            if !vec.is_empty() {
                vec.last().and_then(last_line_in_node)
            } else {
                Line::new(node.span.end.line())
            }
        }
        saphyr::YamlDataOwned::Mapping(hash) => hash.back().and_then(|(_, v)| last_line_in_node(v)),
        _ => Line::new(node.span.end.line()),
    }
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
                render(actual_left_doc, actual_right_doc, differences, side_by_side);
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
    left_doc: &YamlSource,
    right_doc: &YamlSource,
    differences: Vec<Difference>,
    _side_by_side: bool,
) {
    use owo_colors::OwoColorize;
    let max_width = termsize::get().unwrap().cols;
    let ctx = RenderContext::new(max_width, snippet::Color::Enabled);
    for d in differences {
        match d {
            Difference::Added { path, value } => {
                println!("Added: {p}:", p = path.jq_like().bold());

                let added = render_added(&ctx, path, value, left_doc, right_doc);

                println!("{added}");
            }
            Difference::Removed { path, value } => {
                println!("Removed: {p}:", p = path.jq_like());
                let output = render_removal(&ctx, path, value, left_doc, right_doc);

                println!("{output}");
            }
            Difference::Changed { path, left, right } => {
                let combined = render_difference(&ctx, path, left, left_doc, right, right_doc);
                println!("{combined}");
            }
            Difference::Moved {
                original_path,
                new_path,
            } => {
                println!(
                    "Moved: from {p} to {q}:",
                    p = original_path.jq_like().yellow(),
                    q = new_path.jq_like().yellow()
                );
            }
        }
        println!()
    }
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
