use std::path::PathBuf;

use anyhow::{Result, ensure};
use clap::{ArgGroup, Args, Parser, Subcommand};

use crate::item::Item;

pub const SKYSTONES_PER_REFRESH: u32 = 3;

#[derive(Parser, Debug)]
#[command(name = "e7", version, about)]
pub struct Cli {
    /// -v: match scores per screenshot, -vv: adb commands
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Refresh the secret shop and buy items (be on the Secret Shop screen first)
    Run(RunArgs),
    /// List adb devices
    Devices,
    /// Save a 1920x1080 screenshot (useful for cropping new templates)
    Screenshot(ScreenshotArgs),
}

#[derive(Args, Debug)]
#[command(group(ArgGroup::new("limit").required(true)))]
pub struct RunArgs {
    /// Number of refreshes (3 skystones each)
    #[arg(long, group = "limit", value_name = "N")]
    pub refreshes: Option<u32>,
    /// Skystone budget (converted to refreshes)
    #[arg(long, group = "limit", value_name = "N")]
    pub skystones: Option<u32>,
    /// Items to buy: cov, mys, fb (or covenant, mystic, friendship)
    #[arg(long, value_delimiter = ',', default_values = ["cov", "mys"])]
    pub buy: Vec<Item>,
    /// Detect and log only; never tap. --refreshes counts screenshots.
    #[arg(long)]
    pub dry_run: bool,
    /// adb serial (required when more than one device is attached)
    #[arg(long, value_name = "SERIAL")]
    pub device: Option<String>,
    /// Pause between taps, in seconds
    #[arg(long, default_value_t = 0.3, value_name = "SECS")]
    pub tap_sleep: f64,
    #[arg(long, hide = true)]
    pub jitter: bool,
    #[arg(long, hide = true, default_value_t = 0.75)]
    pub threshold: f32,
    #[arg(long, hide = true)]
    pub no_display_override: bool,
    #[arg(long, hide = true, value_name = "DIR")]
    pub templates_dir: Option<PathBuf>,
}

impl RunArgs {
    pub fn refreshes(&self) -> Result<u32> {
        if let Some(n) = self.refreshes {
            return Ok(n);
        }
        let skystones = self.skystones.expect("clap enforces the limit group");
        let n = skystones / SKYSTONES_PER_REFRESH;
        ensure!(
            n > 0,
            "--skystones must be at least {SKYSTONES_PER_REFRESH}"
        );
        Ok(n)
    }
}

#[derive(Args, Debug)]
pub struct ScreenshotArgs {
    /// Output path (default: e7-<timestamp>.png)
    #[arg(long, value_name = "PATH")]
    pub out: Option<PathBuf>,
    #[arg(long, value_name = "SERIAL")]
    pub device: Option<String>,
    #[arg(long, hide = true)]
    pub no_display_override: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str]) -> RunArgs {
        let mut argv = vec!["e7", "run"];
        argv.extend(args);
        match Cli::try_parse_from(argv).unwrap().command {
            Command::Run(r) => r,
            _ => panic!("expected run"),
        }
    }

    #[test]
    fn skystones_divide_by_three() {
        assert_eq!(run(&["--skystones", "300"]).refreshes().unwrap(), 100);
        assert_eq!(run(&["--skystones", "301"]).refreshes().unwrap(), 100);
    }

    #[test]
    fn refreshes_pass_through() {
        assert_eq!(run(&["--refreshes", "7"]).refreshes().unwrap(), 7);
    }

    #[test]
    fn too_few_skystones_is_error() {
        assert!(run(&["--skystones", "2"]).refreshes().is_err());
    }

    #[test]
    fn limit_flags_are_exclusive_and_required() {
        assert!(
            Cli::try_parse_from(["e7", "run", "--refreshes", "1", "--skystones", "3"]).is_err()
        );
        assert!(Cli::try_parse_from(["e7", "run"]).is_err());
    }

    #[test]
    fn buy_defaults_and_aliases() {
        assert_eq!(run(&["--refreshes", "1"]).buy, vec![Item::Cov, Item::Mys]);
        assert_eq!(
            run(&["--refreshes", "1", "--buy", "covenant,fb"]).buy,
            vec![Item::Cov, Item::Fb]
        );
    }
}
