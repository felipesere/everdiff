use std::fmt;

use clap::{Parser, ValueEnum};
use config::config_from_env;
use diff::Difference;
use multidoc::{AdditionalDoc, DocDifference, MissingDoc};
use notify::{RecursiveMode, Watcher};
use owo_colors::{OwoColorize, Style};
use path::{IgnorePath, Path};

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

struct YamlSource {
    file: camino::Utf8PathBuf,
    yaml: serde_yaml::Value,
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

    render_multidoc_diff(diffs, args.ignore_moved, &args.ignore_changes);

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

            render_multidoc_diff(diffs, args.ignore_moved, &args.ignore_changes);
        }
    }

    Ok(())
}

fn read_and_patch(
    paths: &[camino::Utf8PathBuf],
    patches: &[prepatch::PrePatch],
) -> anyhow::Result<Vec<YamlSource>> {
    use serde::Deserialize;

    let mut docs = Vec::new();
    for p in paths {
        let f = std::fs::File::open(p)?;
        for document in serde_yaml::Deserializer::from_reader(f) {
            let v = serde_yaml::Value::deserialize(document)?;
            docs.push(YamlSource {
                file: p.clone(),
                yaml: v,
            });
        }
    }
    for patch in patches {
        let _err = patch.apply_to(&mut docs);
    }

    Ok(docs)
}

pub fn render_multidoc_diff(
    differences: Vec<DocDifference>,
    ignore_moved: bool,
    ignore: &[IgnorePath],
) {
    use owo_colors::OwoColorize;

    if differences.is_empty() {
        println!("No differences found")
    }

    for d in differences {
        match d {
            DocDifference::Addition(AdditionalDoc { key, .. }) => {
                let key = indent::indent_all_by(4, key.to_string());
                println!("{m}", m = "Additional document:".green());
                println!("{key}");
            }
            DocDifference::Missing(MissingDoc { key, .. }) => {
                let key = indent::indent_all_by(4, key.to_string());
                println!("{m}", m = "Missing document:".red());
                println!("{key}");
            }
            DocDifference::Changed {
                key, differences, ..
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

                let key = indent::indent_all_by(4, key.to_string());
                println!("Changed document:");
                println!("{key}");
                render(differences);
            }
        }
    }
}

pub fn render(differences: Vec<Difference>) {
    use owo_colors::OwoColorize;
    for d in differences {
        match d {
            Difference::Added { path, value } => {
                println!("Added: {p}:", p = path.jq_like().bold());
                let added_yaml = indent::indent_all_by(4, serde_yaml::to_string(&value).unwrap());

                println!("{a}", a = added_yaml.green());
            }
            Difference::Removed { path, value } => {
                println!("Removed: {p}:", p = path.jq_like().bold());
                let removed_yaml = indent::indent_all_by(4, serde_yaml::to_string(&value).unwrap());
                println!("{r}", r = removed_yaml.red());
            }
            Difference::Changed { path, left, right } => {
                println!("Changed: {p}:", p = path.jq_like().bold());

                match (left, right) {
                    (serde_yaml::Value::String(left), serde_yaml::Value::String(right)) => {
                        render_string_diff(&left, &right)
                    }
                    (left, right) => {
                        let left = indent::indent_all_by(4, serde_yaml::to_string(&left).unwrap());
                        let right =
                            indent::indent_all_by(4, serde_yaml::to_string(&right).unwrap());

                        print!("{r}", r = left.green());
                        print!("{r}", r = right.red());
                    }
                }
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
