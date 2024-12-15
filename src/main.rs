use clap::{Parser, ValueEnum};
use diff::Difference;
use multidoc::{AdditionalDoc, DocDifference, MissingDoc};
use notify::{RecursiveMode, Watcher};

mod diff;
mod identifier;
mod multidoc;

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

    /// Watch the `left` and `right` files for changes and re-run
    #[arg(short = 'w', long, default_value = "false")]
    watch: bool,

    #[clap(short, long, value_delimiter = ' ', num_args = 1..)]
    left: Vec<camino::Utf8PathBuf>,
    #[clap(short, long, value_delimiter = ' ', num_args = 1..)]
    right: Vec<camino::Utf8PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let left = read_all_docs(&args.left)?;
    let right = read_all_docs(&args.right)?;
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

    render_multidoc_diff(diffs);

    if args.watch {
        let (tx, rx) = std::sync::mpsc::channel();

        let mut watcher = notify::recommended_watcher(tx)?;
        for p in args.left.clone().into_iter().chain(args.right.clone()) {
            watcher.watch(p.as_std_path(), RecursiveMode::NonRecursive)?;
        }

        for event in rx {
            let _event = event?;
            print!("{esc}[2J{esc}[1;1H", esc = 27 as char);
            let left = read_all_docs(&args.left)?;
            let right = read_all_docs(&args.right)?;

            let diffs = multidoc::diff(&ctx, &left, &right);

            render_multidoc_diff(diffs);
        }
    }

    Ok(())
}

fn read_all_docs(paths: &[camino::Utf8PathBuf]) -> anyhow::Result<Vec<serde_yaml::Value>> {
    use serde::Deserialize;

    let mut docs = Vec::new();
    for p in paths {
        let f = std::fs::File::open(p)?;
        for document in serde_yaml::Deserializer::from_reader(f) {
            let v = serde_yaml::Value::deserialize(document)?;
            docs.push(v);
        }
    }

    Ok(docs)
}

pub fn render_multidoc_diff(differences: Vec<DocDifference>) {
    if differences.is_empty() {
        println!("No differences found")
    }
    for d in differences {
        match d {
            DocDifference::Addition(AdditionalDoc { key, .. }) => {
                let key = indent::indent_all_by(4, key.to_string());
                println!("Additional document:");
                println!("{key}");
            }
            DocDifference::Missing(MissingDoc { key, .. }) => {
                let key = indent::indent_all_by(4, key.to_string());
                println!("Additional document:");
                println!("{key}");
            }
            DocDifference::Changed {
                key, differences, ..
            } => {
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
                println!("Added: {p}:", p = path.jq_like());
                let added_yaml = indent::indent_all_by(4, serde_yaml::to_string(&value).unwrap());

                println!("{a}", a = added_yaml.green());
            }
            Difference::Removed { path, value } => {
                println!("Removed: {p}:", p = path.jq_like());
                let removed_yaml = indent::indent_all_by(4, serde_yaml::to_string(&value).unwrap());
                println!("{r}", r = removed_yaml.red());
            }
            Difference::Changed { path, left, right } => {
                println!("Changed: {p}:", p = path.jq_like());
                let left = indent::indent_all_by(4, serde_yaml::to_string(&left).unwrap());
                let right = indent::indent_all_by(4, serde_yaml::to_string(&right).unwrap());

                print!("{r}", r = left.green());
                print!("{r}", r = right.red());
            }
        }
        println!()
    }
}
