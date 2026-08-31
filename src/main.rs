mod cli;
mod item;

use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();
    match cli.command {
        cli::Command::Run(args) => println!("run {args:?}"),
        cli::Command::Devices => println!("devices"),
        cli::Command::Screenshot(args) => println!("screenshot {args:?}"),
    }
}
