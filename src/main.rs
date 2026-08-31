mod adb;
mod cli;
mod display;
mod history;
mod item;
mod matcher;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    match cli.command {
        cli::Command::Run(args) => println!("run {args:?}"),
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
    }
    Ok(())
}
