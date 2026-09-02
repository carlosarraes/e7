mod adb;
mod altar;
mod cli;
mod display;
mod history;
mod item;
mod matcher;
mod shop;

use std::io::IsTerminal;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use tracing::{info, warn};
use tracing_subscriber::fmt::time::ChronoLocal;

fn main() {
    let cli = cli::Cli::parse();
    init_tracing(cli.verbose);
    if let Err(e) = dispatch(cli.command) {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn init_tracing(verbose: u8) {
    let level = match verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    tracing_subscriber::fmt()
        .with_env_filter(format!("e7={level}"))
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .with_target(false)
        .with_timer(ChronoLocal::new("%H:%M:%S".into()))
        .compact()
        .init();
}

fn dispatch(command: cli::Command) -> Result<()> {
    match command {
        cli::Command::Devices => {
            for d in adb::Adb::new(None)?.devices()? {
                println!("{}\t{}", d.serial, d.state);
            }
        }
        cli::Command::Screenshot(args) => {
            let adb = adb::Adb::pick(args.device)?;
            let _guard = display::DisplayGuard::acquire(&adb, !args.no_display_override)?;
            let out = args.out.unwrap_or_else(|| {
                std::path::PathBuf::from(format!(
                    "e7-{}.png",
                    chrono::Local::now().format("%Y%m%d-%H%M%S")
                ))
            });
            adb.screencap()?.save(&out)?;
            println!("{}", out.display());
        }
        cli::Command::Run(args) => run(args)?,
        cli::Command::Altar(args) => altar(args)?,
    }
    Ok(())
}

fn run(args: cli::RunArgs) -> Result<()> {
    let refreshes = args.refreshes()?;
    let adb = adb::Adb::pick(args.device.clone())?;
    let _guard = display::DisplayGuard::acquire(&adb, !args.no_display_override)?;

    let templates = matcher::load_templates(&args.buy, args.templates_dir.as_deref())?;
    let matcher = matcher::Matcher::new(templates, args.threshold, matcher::default_column());

    let anchor = matcher::Anchor::refresh_button()?;

    let stop = Arc::new(AtomicBool::new(false));
    install_ctrlc(stop.clone(), adb.clone());

    let history = if args.dry_run {
        None
    } else {
        Some(history::History::open(&history::new_run_id())?)
    };
    let items: Vec<&str> = args.buy.iter().map(|i| i.key()).collect();
    info!(
        "device {} · {} {} · buying {} · Ctrl+C to stop",
        adb.serial().unwrap_or("?"),
        refreshes,
        if args.dry_run {
            "screenshots (dry run)"
        } else {
            "refreshes"
        },
        items.join(",")
    );

    let cfg = shop::Config {
        refreshes,
        items: args.buy.clone(),
        dry_run: args.dry_run,
        tap_sleep: Duration::from_secs_f64(args.tap_sleep),
        jitter: args.jitter,
    };
    let summary = shop::Runner::new(adb, matcher, anchor, cfg, stop, history).run()?;

    let secs = summary.duration.as_secs();
    let what = if args.dry_run { "detected" } else { "bought" };
    info!(
        "{} {} in {}m{:02}s{}",
        if args.dry_run {
            "screenshots:"
        } else {
            "refreshes:"
        },
        summary.refreshes,
        secs / 60,
        secs % 60,
        if summary.stopped { " (stopped)" } else { "" }
    );
    for (item, n) in &summary.counts {
        info!("{what} {n} × {}", item.name());
    }
    if !args.dry_run {
        info!(
            "skystones {} · gold {}",
            summary.refreshes * cli::SKYSTONES_PER_REFRESH,
            summary.gold
        );
    }
    Ok(())
}

fn altar(args: cli::AltarArgs) -> Result<()> {
    let buys = args.buys()?;
    let adb = adb::Adb::pick(args.device.clone())?;
    let _guard = display::DisplayGuard::acquire(&adb, !args.no_display_override)?;
    let anchors = altar::Anchors::load()?;

    let stop = Arc::new(AtomicBool::new(false));
    install_ctrlc(stop.clone(), adb.clone());

    info!(
        "device {} · {} {} · Ctrl+C to stop",
        adb.serial().unwrap_or("?"),
        buys,
        if args.dry_run {
            "screenshots (dry run)"
        } else {
            "penguin buys"
        }
    );

    let cfg = altar::Config {
        buys,
        dry_run: args.dry_run,
    };
    let summary = altar::Runner::new(adb, anchors, cfg, stop).run()?;

    let secs = summary.duration.as_secs();
    info!(
        "{} {} in {}m{:02}s{}",
        if args.dry_run {
            "screenshots:"
        } else {
            "buys:"
        },
        summary.buys,
        secs / 60,
        secs % 60,
        if summary.stopped { " (stopped)" } else { "" }
    );
    if !args.dry_run {
        info!(
            "penguin nests {} · leaves {}",
            summary.buys * altar::NESTS_PER_BUY,
            summary.buys * cli::LEAVES_PER_BUY
        );
    }
    Ok(())
}

/// First Ctrl+C stops after the current step; the second aborts immediately
/// (still resetting the display).
fn install_ctrlc(stop: Arc<AtomicBool>, adb: adb::Adb) {
    if let Err(e) = ctrlc::set_handler(move || {
        if stop.swap(true, Ordering::SeqCst) {
            eprintln!("\naborting");
            let _ = adb.wm_size_reset();
            std::process::exit(130);
        }
        eprintln!("\nstopping after the current step (Ctrl+C again to abort)");
    }) {
        warn!("Ctrl+C handler not installed: {e}");
    }
}
