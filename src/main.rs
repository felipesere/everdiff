use clap::{Parser, ValueEnum};
use everdiff::{
    config::config_from_env, identifier, multidoc, path::IgnorePath, read_and_patch,
    render_multidoc_diff,
};
use notify::{RecursiveMode, Watcher};

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

    /// Enable verbose logging
    #[arg(short = 'v', long, default_value = "false")]
    verbose: bool,

    #[clap(short, long, value_delimiter = ' ', num_args = 1..)]
    left: Vec<camino::Utf8PathBuf>,
    #[clap(short, long, value_delimiter = ' ', num_args = 1..)]
    right: Vec<camino::Utf8PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize logging with colors
    if args.verbose {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("error"))
            .format(|buf, record| {
                use owo_colors::OwoColorize;
                use std::io::Write;

                let level_color = match record.level() {
                    log::Level::Error => record.level().to_string().red().to_string(),
                    log::Level::Warn => record.level().to_string().yellow().to_string(),
                    log::Level::Info => record.level().to_string().green().to_string(),
                    log::Level::Debug => record.level().to_string().blue().to_string(),
                    log::Level::Trace => record.level().to_string().purple().to_string(),
                };

                writeln!(buf, "[{}] {}", level_color, record.args())
            })
            .init();
    } else {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("off")).init();
    }

    log::debug!("Starting everdiff with args: {:?}", args);

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

    log::error!("this went wrong!");

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
