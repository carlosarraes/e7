use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use tracing::info;

use crate::adb::Adb;
use crate::matcher::Anchor;

pub const NESTS_PER_BUY: u32 = 50;

const SETTLE: Duration = Duration::from_millis(1500);

pub struct Anchors {
    pub altar: Anchor,
    pub max: Anchor,
    pub popup: Anchor,
}

impl Anchors {
    pub fn load() -> Result<Self> {
        Ok(Self {
            altar: Anchor::altar_buy()?,
            max: Anchor::altar_max()?,
            popup: Anchor::altar_close()?,
        })
    }
}

pub struct Config {
    pub buys: u32,
    pub dry_run: bool,
}

pub struct Summary {
    pub buys: u32,
    pub duration: Duration,
    pub stopped: bool,
}

pub struct Runner {
    adb: Adb,
    anchors: Anchors,
    cfg: Config,
    stop: Arc<AtomicBool>,
    done: u32,
}

impl Runner {
    pub fn new(adb: Adb, anchors: Anchors, cfg: Config, stop: Arc<AtomicBool>) -> Self {
        Self {
            adb,
            anchors,
            cfg,
            stop,
            done: 0,
        }
    }

    pub fn run(mut self) -> Result<Summary> {
        let start = Instant::now();
        if self.cfg.dry_run {
            self.dry_run()?;
        } else {
            bail!("live altar buying is not implemented yet; use --dry-run");
        }
        Ok(Summary {
            buys: self.done,
            duration: start.elapsed(),
            stopped: self.stop.load(Ordering::SeqCst),
        })
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Screenshot + score the three anchors, no input at all. `buys` = number of screenshots.
    fn dry_run(&mut self) -> Result<()> {
        while self.done < self.cfg.buys && !self.stopped() {
            let screen = self.adb.screencap()?;
            self.done += 1;
            info!(
                "[{}/{}] altar {:.3} · 50/50 {:.3} · popup {:.3}",
                self.done,
                self.cfg.buys,
                self.anchors.altar.score(&screen),
                self.anchors.max.score(&screen),
                self.anchors.popup.score(&screen)
            );
            sleep(SETTLE);
        }
        Ok(())
    }
}
