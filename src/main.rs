#![allow(unused)]
use std::io;

use clap::{Parser, ValueEnum};
use config::config_from_env;
use diff::{Difference, Path};
use multidoc::{AdditionalDoc, DocDifference, MissingDoc};
use notify::{RecursiveMode, Watcher};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tui::TuiApp;

mod config;
mod diff;
mod identifier;
mod multidoc;
mod prepatch;
mod tui;

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
    #[arg(long, default_value = "false")]
    interactive: bool,

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

    tracing_subscriber::registry()
        .with(tui_logger::tracing_subscriber_layer())
        .init();

    tui_logger::init_logger(tui_logger::LevelFilter::Trace).expect("Setting up tui_logger");
    tui_logger::set_default_level(tui_logger::LevelFilter::Trace);

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

    if args.interactive {
        let mut terminal = ratatui::init();
        let app_result = TuiApp::new(diffs).run(&mut terminal);
        ratatui::restore();
    } else {
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
                let left = read_and_patch(&args.left, &patches)?;
                let right = read_and_patch(&args.right, &patches)?;

                let diffs = multidoc::diff(&ctx, &left, &right);

                render_multidoc_diff(diffs);
            }
        }
    }

    Ok(())
}

fn read_and_patch(
    paths: &[camino::Utf8PathBuf],
    patches: &[prepatch::PrePatch],
) -> anyhow::Result<Vec<serde_yaml::Value>> {
    use serde::Deserialize;

    let mut docs = Vec::new();
    for p in paths {
        let f = std::fs::File::open(p)?;
        for document in serde_yaml::Deserializer::from_reader(f) {
            let v = serde_yaml::Value::deserialize(document)?;
            docs.push(v);
        }
    }
    for patch in patches {
        let _err = patch.apply_to(&mut docs);
    }

    Ok(docs)
}

pub fn render_multidoc_diff(differences: Vec<DocDifference>) {
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
                let left = indent::indent_all_by(4, serde_yaml::to_string(&left).unwrap());
                let right = indent::indent_all_by(4, serde_yaml::to_string(&right).unwrap());

                print!("{r}", r = left.green());
                print!("{r}", r = right.red());
            }
        }
        println!()
    }
}
