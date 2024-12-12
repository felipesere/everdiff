use clap::Parser;

mod diff;
mod identifier;
mod multidoc;

/// Differnece between YAML documents
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    multidoc: bool,
    left: camino::Utf8PathBuf,
    right: camino::Utf8PathBuf,
}

fn main() {
    let args = Args::parse();

    let left = std::fs::File::open(args.left).unwrap();
    let left_doc: serde_yaml::Value = serde_yaml::from_reader(left).unwrap();

    let right = std::fs::File::open(args.right).unwrap();
    let right_doc: serde_yaml::Value = serde_yaml::from_reader(right).unwrap();

    let diffs = diff::diff(diff::Context::new(), &left_doc, &right_doc);

    dbg!(diffs);
}
