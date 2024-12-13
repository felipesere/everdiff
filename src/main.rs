use clap::{Parser, Subcommand, ValueEnum};
use diff::Difference;
use multidoc::{AdditionalDoc, DocDifference, MissingDoc};

mod diff;
mod identifier;
mod multidoc;

#[derive(Subcommand, Debug)]
enum Commands {
    Between {
        left: camino::Utf8PathBuf,
        right: camino::Utf8PathBuf,
    },

    MultiDoc {
        /// Use Kubernetes comparison
        #[arg(short = 'k', long, default_value = "false")]
        kubernetes: bool,
        #[clap(short, long, value_delimiter = ' ', num_args = 1..)]
        left: Vec<camino::Utf8PathBuf>,
        #[clap(short, long, value_delimiter = ' ', num_args = 1..)]
        right: Vec<camino::Utf8PathBuf>,
    },
}

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
    #[command(subcommand)]
    commands: Commands,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    match args.commands {
        Commands::Between { left, right } => {
            let left = std::fs::File::open(left).unwrap();
            let left_doc: serde_yaml::Value = serde_yaml::from_reader(left).unwrap();

            let right = std::fs::File::open(right).unwrap();
            let right_doc: serde_yaml::Value = serde_yaml::from_reader(right).unwrap();

            let diffs = diff::diff(diff::Context::new(), &left_doc, &right_doc);

            render(diffs);
        }
        Commands::MultiDoc {
            left,
            right,
            kubernetes,
        } => {
            let left = read_all_docs(&left)?;
            let right = read_all_docs(&right)?;
            let comparator = if kubernetes {
                Comparison::Kubernetes
            } else {
                Comparison::Index
            };

            let id = match comparator {
                Comparison::Index => identifier::by_index(),
                Comparison::Kubernetes => identifier::kubernetes::metadata_name(),
            };

            let ctx = multidoc::Context::new_with_doc_identifier(id);

            let diffs = multidoc::diff(ctx, &left, &right);

            render_multidoc_diff(diffs)
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
    for d in differences {
        match d {
            DocDifference::Addition(AdditionalDoc { key, .. }) => {
                let key = indent::indent_by(4, key.to_string());
                println!("Additional document:");
                println!("{key}");
            }
            DocDifference::Missing(MissingDoc { key, .. }) => {
                let key = indent::indent_by(4, key.to_string());
                println!("Additional document:");
                println!("{key}");
            }
            DocDifference::Changed {
                key, differences, ..
            } => {
                let key = indent::indent_by(4, key.to_string());
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
