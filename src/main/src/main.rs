use bpaf::{Parser, construct, short};
use everdiff_diff::{IgnorePath, read_and_patch};
use everdiff_multidoc as multidoc;
use everdiff_snippet::render_multidoc_diff;
use notify::{RecursiveMode, Watcher};
use owo_colors::OwoColorize;

mod config;
mod identifier;

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

    setup_logging(args.verbosity)?;

    log::debug!("Starting everdiff with args: {:?}", args);

    let _config = config::config_from_env();
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

fn setup_logging(verbosity: usize) -> Result<(), anyhow::Error> {
    let mut base_config = fern::Dispatch::new().format(move |out, message, record| {
        let level = match record.level() {
            log::Level::Error => "ERROR".red().to_string(),
            log::Level::Warn => "WARN".yellow().to_string(),
            log::Level::Info => "INFO".blue().to_string(),
            log::Level::Debug => "DEBUG".green().to_string(),
            log::Level::Trace => "TRACE".magenta().to_string(),
        };

        let module = record.module_path().unwrap_or("unknown");

        out.finish(format_args!("{level}:{module}: {message}",))
    });

    // Adjust log levels for moudles as needed
    //    1 => base_config
    //        .level(log::LevelFilter::Debug)
    //        .level_for("rustls", log::LevelFilter::Warn)
    //        .level_for("ureq", log::LevelFilter::Warn)
    //        .level_for("ureq_proto", log::LevelFilter::Warn),
    base_config = match verbosity {
        0 => base_config.level(log::LevelFilter::Warn),
        1 => base_config.level(log::LevelFilter::Debug),
        2 => base_config.level(log::LevelFilter::Trace),
        _ => unreachable!("verbosity > 3"),
    };
    base_config.chain(std::io::stderr()).apply()?;

    Ok(())
}
