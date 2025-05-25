use clap::Parser;
use saphyr::MarkedYamlOwned;

use everdiff::{YamlSource, read_and_patch};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[clap(short, long)]
    left: camino::Utf8PathBuf,
}

fn print_node_spans(node: &MarkedYamlOwned, depth: usize) {
    let indent = "  ".repeat(depth);
    let span = &node.span;

    match &node.data {
        saphyr::YamlDataOwned::Value(saphyr::ScalarOwned::Null) => {
            println!(
                "{}Null: span {}:{}-{}:{}",
                indent,
                span.start.line(),
                span.start.col(),
                span.end.line(),
                span.end.col()
            );
        }
        saphyr::YamlDataOwned::Value(saphyr::ScalarOwned::Boolean(b)) => {
            println!(
                "{}Boolean({}): span {}:{}-{}:{}",
                indent,
                b,
                span.start.line(),
                span.start.col(),
                span.end.line(),
                span.end.col()
            );
        }
        saphyr::YamlDataOwned::Value(saphyr::ScalarOwned::Integer(i)) => {
            println!(
                "{}Integer({}): span {}:{}-{}:{}",
                indent,
                i,
                span.start.line(),
                span.start.col(),
                span.end.line(),
                span.end.col()
            );
        }
        saphyr::YamlDataOwned::Value(saphyr::ScalarOwned::FloatingPoint(r)) => {
            println!(
                "{}Real({}): span {}:{}-{}:{}",
                indent,
                r,
                span.start.line(),
                span.start.col(),
                span.end.line(),
                span.end.col()
            );
        }
        saphyr::YamlDataOwned::Value(saphyr::ScalarOwned::String(s)) => {
            println!(
                "{}String(\"{}\"): span {}:{}-{}:{}",
                indent,
                s,
                span.start.line(),
                span.start.col(),
                span.end.line(),
                span.end.col()
            );
        }
        saphyr::YamlDataOwned::Sequence(seq) => {
            println!(
                "{}Sequence: span {}:{}-{}:{}",
                indent,
                span.start.line(),
                span.start.col(),
                span.end.line(),
                span.end.col()
            );
            for item in seq {
                print_node_spans(item, depth + 1);
            }
        }
        saphyr::YamlDataOwned::Mapping(map) => {
            println!(
                "{}Mapping: span {}:{}-{}:{}",
                indent,
                span.start.line(),
                span.start.col(),
                span.end.line(),
                span.end.col()
            );
            for (key, value) in map {
                println!("{}  Key:", indent);
                print_node_spans(key, depth + 2);
                println!("{}  Value:", indent);
                print_node_spans(value, depth + 2);
            }
        }
        saphyr::YamlDataOwned::Representation(repr, style, tag) => {
            println!(
                "{}Representation(\"{}\", {:?}, {:?}): span {}:{}-{}:{}",
                indent,
                repr,
                style,
                tag,
                span.start.line(),
                span.start.col(),
                span.end.line(),
                span.end.col()
            );
        }
        saphyr::YamlDataOwned::Alias(alias_id) => {
            println!(
                "{}Alias({}): span {}:{}-{}:{}",
                indent,
                alias_id,
                span.start.line(),
                span.start.col(),
                span.end.line(),
                span.end.col()
            );
        }
        saphyr::YamlDataOwned::BadValue => {
            println!(
                "{}BadValue: span {}:{}-{}:{}",
                indent,
                span.start.line(),
                span.start.col(),
                span.end.line(),
                span.end.col()
            );
        }
    }
}

fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();

    let sources = read_and_patch(&[args.left], &[])?;

    for source in sources {
        println!("File: {}", source.file);
        println!("Document index: {}", source.index);
        println!("YAML tree structure with spans:");
        print_node_spans(&source.yaml, 0);
    }

    Ok(())
}
