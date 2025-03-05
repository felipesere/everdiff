use std::{
    cmp::min,
    fmt::{self},
    io::Read,
};

use clap::{Parser, ValueEnum};
use config::config_from_env;
use diff::Difference;
use multidoc::{AdditionalDoc, DocDifference, MissingDoc};
use notify::{RecursiveMode, Watcher};
use owo_colors::{OwoColorize, Style};
use path::IgnorePath;
use saphyr::MarkedYaml;

mod config;
mod diff;
mod identifier;
mod multidoc;
mod path;
mod prepatch;

#[derive(Default, ValueEnum, Clone, Debug)]
enum Comparison {
    #[default]
    Index,
    Kubernetes,
}

/// Differnece between YAML documents
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Render differences side-by-side
    #[arg(short = 's', long, default_value = "false")]
    side_by_side: bool,

    /// Use Kubernetes comparison
    #[arg(short = 'k', long, default_value = "false")]
    kubernetes: bool,

    /// Don't show changes for moved elements
    #[arg(short = 'm', long, default_value = "false")]
    ignore_moved: bool,

    /// Don't show changes for moved elements
    #[arg(short, long, value_parser = clap::value_parser!(IgnorePath), value_delimiter = ' ', num_args = 0..)]
    ignore_changes: Vec<IgnorePath>,

    /// Watch the `left` and `right` files for changes and re-run
    #[arg(short = 'w', long, default_value = "false")]
    watch: bool,

    #[clap(short, long, value_delimiter = ' ', num_args = 1..)]
    left: Vec<camino::Utf8PathBuf>,
    #[clap(short, long, value_delimiter = ' ', num_args = 1..)]
    right: Vec<camino::Utf8PathBuf>,
}

pub struct YamlSource {
    pub file: camino::Utf8PathBuf,
    pub yaml: saphyr::MarkedYaml,
    pub content: String,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let maybe_config = config_from_env();
    let patches = maybe_config.map(|c| c.prepatches).unwrap_or_default();

    let left = read_and_patch(&args.left, &patches)?;
    let right = read_and_patch(&args.right, &patches)?;

    let comparator = if args.kubernetes {
        Comparison::Kubernetes
    } else {
        Comparison::Index
    };

    let id = match comparator {
        Comparison::Index => identifier::by_index(),
        Comparison::Kubernetes => identifier::kubernetes::gvk(),
    };

    let ctx = multidoc::Context::new_with_doc_identifier(id);

    let diffs = multidoc::diff(&ctx, &left, &right);

    render_multidoc_diff(
        (left, right),
        diffs,
        args.ignore_moved,
        &args.ignore_changes,
        args.side_by_side,
    );

    if args.watch {
        let (tx, rx) = std::sync::mpsc::channel();

        let mut watcher = notify::recommended_watcher(tx)?;
        for p in args.left.clone().into_iter().chain(args.right.clone()) {
            watcher.watch(p.as_std_path(), RecursiveMode::NonRecursive)?;
        }

        for event in rx {
            let _event = event?;
            print!("{esc}[2J{esc}[1;1H", esc = 27 as char);
            let left = read_and_patch(&args.left, &patches)?;
            let right = read_and_patch(&args.right, &patches)?;

            let diffs = multidoc::diff(&ctx, &left, &right);

            render_multidoc_diff(
                (left, right),
                diffs,
                args.ignore_moved,
                &args.ignore_changes,
                args.side_by_side,
            );
        }
    }

    Ok(())
}

fn read_and_patch(
    paths: &[camino::Utf8PathBuf],
    patches: &[prepatch::PrePatch],
) -> anyhow::Result<Vec<YamlSource>> {
    let mut docs = Vec::new();
    for p in paths {
        let mut f = std::fs::File::open(p)?;
        let mut content = String::new();
        f.read_to_string(&mut content)?;

        let split_docs: Vec<_> = content
            .clone()
            .split("---")
            .skip(1)
            .map(|c| c.to_string())
            .collect();

        let n = saphyr::MarkedYaml::load_from_str(&content)?;
        for (document, content) in n.into_iter().zip(split_docs) {
            docs.push(YamlSource {
                file: p.clone(),
                yaml: document,
                content,
            });
        }
    }
    for patch in patches {
        let _err = patch.apply_to(&mut docs);
    }

    Ok(docs)
}

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

fn stringify(yaml: &MarkedYaml) -> String {
    let mut out_str = String::new();
    let mut emitter = saphyr::YamlEmitter::new(&mut out_str);
    emitter.dump(&yaml).expect("failed to write YAML to buffer");
    match out_str.find('\n') {
        Some(pos) => out_str[pos + 1..].to_string(),
        None => out_str,
    }
}

pub fn render(
    left_doc: &YamlSource,
    right_doc: &YamlSource,
    differences: Vec<Difference>,
    _side_by_side: bool,
) {
    use owo_colors::OwoColorize;
    let max_width = termsize::get().unwrap().cols;
    for d in differences {
        match d {
            Difference::Added { path, value } => {
                println!("Added: {p}:", p = path.jq_like().bold());
                let added_yaml = indent::indent_all_by(4, stringify(&value));

                println!("{a}", a = added_yaml.green());
            }
            Difference::Removed { path, value } => {
                println!("Removed: {p}:", p = path.jq_like().bold());
                let removed_yaml = indent::indent_all_by(4, stringify(&value));
                println!("{r}", r = removed_yaml.red());
            }
            Difference::Changed { path, left, right } => {
                println!("Changed: {p}:", p = path.jq_like().bold());

                let max_left = ((max_width - 24) / 2) as usize; // includes a bit of random padding, do this proper later
                                                                //
                let left = render_diff_widget(max_left, left_doc, left);
                let right = render_diff_widget(max_left, right_doc, right);

                let combined = left
                    .iter()
                    .zip(right)
                    .map(|(l, r)| format!("{l} │ {r}"))
                    .collect::<Vec<_>>()
                    .join("\n");

                println!("{combined}");

                // match (&left.data, &right.data) {
                //     (YamlData::String(left), YamlData::String(right)) => {
                //         render_string_diff(left, right)
                //     }
                //     _ => {
                //         let left = indent::indent_all_by(4, stringify(&left));
                //         let right = indent::indent_all_by(4, stringify(&right));

                //         print!("{r}", r = left.green());
                //         print!("{r}", r = right.red());
                //     }
                // }
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

fn render_diff_widget(
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
                (max_width + 2, line.green().to_string())
            } else {
                (max_width, line.dimmed().to_string())
            };

            format!("{nr:<3}│ {line:<w$}")
        })
        .collect::<Vec<_>>()
}

fn node_in<'y>(yaml: &'y MarkedYaml, path: &path::Path) -> Option<&'y MarkedYaml> {
    let mut n = Some(yaml);
    for p in path.segments() {
        match p {
            path::Segment::Field(f) => {
                let Some(v) = n.and_then(|n| n.get(f)) else {
                    return None;
                };
                n = Some(v);
            }
            path::Segment::Index(nr) => {
                let Some(v) = n.and_then(|n| n.get(nr)) else {
                    return None;
                };
                n = Some(v);
            }
        }
    }
    n
}

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
                    Line(change.old_index()).to_string().dimmed(),
                    Line(change.new_index()).to_string().dimmed(),
                    sign.style(emphasis_style).bold(),
                );
                for (emphasized, value) in change.iter_strings_lossy() {
                    if emphasized {
                        print!("{}", value.style(emphasis_style.underline()));
                    } else {
                        print!("{}", value);
                    }
                }
                if change.missing_newline() {
                    println!();
                }
            }
        }
    }
}

struct Line(Option<usize>);

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            None => write!(f, "   "),
            Some(idx) => write!(f, "{:<3}", idx + 1),
        }
    }
}
