use clap::{Parser, Subcommand, ValueEnum};

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
        #[clap(short, long, default_value = "index")]
        compare_docs_by: CompareDocsBy,
        #[clap(short, long, value_delimiter = ' ', num_args = 1..)]
        left: Vec<camino::Utf8PathBuf>,
        #[clap(short, long, value_delimiter = ' ', num_args = 1..)]
        right: Vec<camino::Utf8PathBuf>,
    },
}

#[derive(Default, ValueEnum, Clone, Debug)]
enum CompareDocsBy {
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

            dbg!(diffs);
        }

        Commands::MultiDoc {
            left,
            right,
            compare_docs_by,
        } => {
            let left = read_all_docs(&left)?;
            let right = read_all_docs(&right)?;

            let id = match compare_docs_by {
                CompareDocsBy::Index => identifier::by_index(),
                CompareDocsBy::Kubernetes => identifier::kubernetes::metadata_name(),
            };

            let ctx = multidoc::Context::new_with_doc_identifier(id);

            let diffs = multidoc::diff(ctx, &left, &right);

            dbg!(diffs);
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
