mod adb;
mod cli;
mod item;

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
        cli::Command::Screenshot(args) => println!("screenshot {args:?}"),
    }
    Ok(())
}
