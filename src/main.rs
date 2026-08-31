use clap::Parser;

#[derive(Parser)]
#[command(name = "e7", version, about)]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
}
