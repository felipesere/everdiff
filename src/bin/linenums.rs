use clap::Parser;
use saphyr::{MarkedYamlOwned, Marker};

use everdiff::read_and_patch;

struct Span {
    start: Marker,
    end: Marker,
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[clap(short, long)]
    left: camino::Utf8PathBuf,
}

fn calculate_max_line_width(node: &MarkedYamlOwned) -> usize {
    let span = &node.span;
    let line_str = format!("{}-{}", span.start.line(), span.end.line());
    let mut max_width = line_str.len();

    match &node.data {
        saphyr::YamlDataOwned::Sequence(seq) => {
            for item in seq {
                max_width = max_width.max(calculate_max_line_width(item));
            }
        }
        saphyr::YamlDataOwned::Mapping(map) => {
            for (key, value) in map {
                max_width = max_width.max(calculate_max_line_width(key));
                max_width = max_width.max(calculate_max_line_width(value));
            }
        }
        _ => {}
    }

    max_width
}

fn extract_original_text(content: &str, span: &Span) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let start_line = span.start.line() - 1; // Convert to 0-based indexing
    let end_line = span.end.line() - 1;

    if start_line == end_line {
        // Single line span
        if let Some(line) = lines.get(start_line) {
            let start_col = span.start.col();
            let end_col = span.end.col();
            if start_col < line.len() && end_col <= line.len() {
                return line[start_col..end_col].to_string();
            }
        }
    } else {
        // Multi-line span
        let mut result = String::new();
        for line_idx in start_line..=end_line.min(lines.len() - 1) {
            if let Some(line) = lines.get(line_idx) {
                if line_idx == start_line {
                    // First line - from start column to end
                    let start_col = span.start.col();
                    if start_col < line.len() {
                        result.push_str(&line[start_col..]);
                    }
                } else if line_idx == end_line {
                    // Last line - from beginning to end column
                    let end_col = span.end.col();
                    if end_col <= line.len() {
                        result.push_str(&line[..end_col]);
                    }
                } else {
                    // Middle lines - entire line
                    result.push_str(line);
                }
                if line_idx < end_line {
                    result.push('\n');
                }
            }
        }
        return result;
    }

    // Fallback if extraction fails
    String::new()
}

fn print_node_spans(
    node: &MarkedYamlOwned,
    depth: usize,
    line_width: usize,
    original_content: &str,
) {
    let indent = "  ".repeat(depth);
    let span = Span {
        start: node.span.start,
        end: node.span.end,
    };
    let line_str = format!("{}-{}", span.start.line(), span.end.line());
    let original_text = extract_original_text(original_content, &span);

    match &node.data {
        saphyr::YamlDataOwned::Value(saphyr::ScalarOwned::Null) => {
            println!(
                "{:<width$} {}Null: {}",
                line_str,
                indent,
                original_text.trim(),
                width = line_width
            );
        }
        saphyr::YamlDataOwned::Value(saphyr::ScalarOwned::Boolean(b)) => {
            println!(
                "{:<width$} {}Boolean({}): {}",
                line_str,
                indent,
                b,
                original_text.trim(),
                width = line_width
            );
        }
        saphyr::YamlDataOwned::Value(saphyr::ScalarOwned::Integer(i)) => {
            println!(
                "{:<width$} {}Integer({}): {}",
                line_str,
                indent,
                i,
                original_text.trim(),
                width = line_width
            );
        }
        saphyr::YamlDataOwned::Value(saphyr::ScalarOwned::FloatingPoint(r)) => {
            println!(
                "{line_str:<line_width$} {indent}Real({r}): {}",
                original_text.trim()
            );
        }
        saphyr::YamlDataOwned::Value(saphyr::ScalarOwned::String(s)) => {
            println!(
                "{line_str:<line_width$} {indent}String(\"{s}\"): {}",
                original_text.trim()
            );
        }
        saphyr::YamlDataOwned::Sequence(seq) => {
            println!(
                "{line_str:<width$} {indent}Sequence: {}",
                original_text.lines().next().unwrap_or("").trim(),
                width = line_width
            );
            for item in seq {
                print_node_spans(item, depth + 1, line_width, original_content);
            }
        }
        saphyr::YamlDataOwned::Mapping(map) => {
            println!(
                "{:<width$} {}Mapping: {}",
                line_str,
                indent,
                original_text.lines().next().unwrap_or("").trim(),
                width = line_width
            );
            for (key, value) in map {
                println!("{:<width$} {}  Key:", "", indent, width = line_width);
                print_node_spans(key, depth + 2, line_width, original_content);
                println!("{:<width$} {}  Value:", "", indent, width = line_width);
                print_node_spans(value, depth + 2, line_width, original_content);
            }
        }
        saphyr::YamlDataOwned::Representation(repr, style, tag) => {
            println!(
                "{:<width$} {}Representation(\"{}\", {:?}, {:?}): {}",
                line_str,
                indent,
                repr,
                style,
                tag,
                original_text.trim(),
                width = line_width
            );
        }
        saphyr::YamlDataOwned::Alias(alias_id) => {
            println!(
                "{:<width$} {}Alias({}): {}",
                line_str,
                indent,
                alias_id,
                original_text.trim(),
                width = line_width
            );
        }
        saphyr::YamlDataOwned::BadValue => {
            println!(
                "{:<width$} {}BadValue: {}",
                line_str,
                indent,
                original_text.trim(),
                width = line_width
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

        let max_line_width = calculate_max_line_width(&source.yaml);
        print_node_spans(&source.yaml, 0, max_line_width, &source.content);
    }

    Ok(())
}
