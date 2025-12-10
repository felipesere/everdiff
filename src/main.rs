use bpaf::{Parser, construct, short};
use everdiff::{
    config::config_from_env, identifier, multidoc, path::IgnorePath, read_and_patch,
    render_multidoc_diff,
};
use log::LevelFilter;
use notify::{RecursiveMode, Watcher};

#[derive(Debug)]
struct Args {
    side_by_side: bool,
    kubernetes: bool,
    ignore_moved: bool,
    ignore_changes: Vec<IgnorePath>,
    watch: bool,
    verbosity: usize,
    left: Vec<camino::Utf8PathBuf>,
    right: Vec<camino::Utf8PathBuf>,
}

fn args() -> impl Parser<Args> {
    let side_by_side = short('s')
        .long("side-by-side")
        .help("Render differences side-by-side")
        .switch();

    let kubernetes = short('k')
        .long("kubernetes")
        .help("Use Kubernetes comparison")
        .switch();

    let ignore_moved = short('m')
        .long("ignore-moved")
        .help("Don't show changes for moved elements")
        .switch();

    let ignore_changes = short('i')
        .long("ignore-changes")
        .help("Paths to ignore when comparing")
        .argument::<IgnorePath>("PATH")
        .many();

    let watch = short('w')
        .long("watch")
        .help("Watch the `left` and `right` files for changes and re-run")
        .switch();

    let verbosity = short('v')
        .long("verbose")
        .help("Increase verbosity level (can be repeated)")
        .req_flag(())
        .many()
        .map(|v| v.len());

    let left = short('l')
        .long("left")
        .help("Left file(s) to compare")
        .argument::<camino::Utf8PathBuf>("PATH")
        .some("need at least one left path");

    let right = short('r')
        .long("right")
        .help("Right file(s) to compare")
        .argument::<camino::Utf8PathBuf>("PATH")
        .some("need at least one right path");

    construct!(Args {
        side_by_side,
        kubernetes,
        ignore_moved,
        ignore_changes,
        watch,
        verbosity,
        left,
        right,
    })
}

fn main() -> anyhow::Result<()> {
    let args = args()
        .to_options()
        .descr("Difference between YAML documents")
        .run();

    let level = match args.verbosity {
        0 => LevelFilter::Warn,
        1 => LevelFilter::Info,
        2 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    };

    env_logger::Builder::new()
        .filter_level(level)
        .format_timestamp(None)
        .init();

    log::debug!("Starting everdiff with args: {:?}", args);

    let _config = config_from_env();
    let left = read_and_patch(&args.left)?;
    let right = read_and_patch(&args.right)?;

    let id = if args.kubernetes {
        identifier::kubernetes::gvk()
    } else {
        identifier::by_index()
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
            let left = read_and_patch(&args.left)?;
            let right = read_and_patch(&args.right)?;

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
