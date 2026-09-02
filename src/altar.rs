use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use tracing::{debug, info, warn};

use crate::adb::Adb;
use crate::matcher::Anchor;

pub const NESTS_PER_BUY: u32 = 50;

const SETTLE: Duration = Duration::from_millis(1500);
const AFTER_OPEN: Duration = Duration::from_secs(1);
const AFTER_MAX: Duration = Duration::from_millis(500);
const AFTER_BUY: Duration = Duration::from_secs(2);
const AFTER_CLOSE: Duration = Duration::from_secs(1);

/// Tap targets in the 1920x1080 frame, measured on 2026-09-02.
pub struct Geometry;

impl Geometry {
    pub const BUY: (u32, u32) = (298, 805);
    pub const MAX: (u32, u32) = (1360, 709);
    pub const CANCEL: (u32, u32) = (703, 884);
    pub const CONFIRM: (u32, u32) = (1120, 884);
    pub const CLOSE: (u32, u32) = (958, 792);
}

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
    pub tap_sleep: Duration,
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
            self.live()?;
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

    /// Open the modal, force 50/50, confirm, close the reward popup; one buy per pass.
    fn live(&mut self) -> Result<()> {
        while self.done < self.cfg.buys && !self.stopped() {
            if !self.anchors.altar.visible(&self.adb.screencap()?) {
                return self.left_altar();
            }
            self.tap(Geometry::BUY)?;
            sleep(AFTER_OPEN);
            if !self.ensure_max()? {
                return Ok(());
            }
            self.tap(Geometry::CONFIRM)?;
            sleep(AFTER_BUY);
            if !self.anchors.popup.visible(&self.adb.screencap()?) {
                return self.popup_missing();
            }
            self.tap(Geometry::CLOSE)?;
            sleep(AFTER_CLOSE);
            self.done += 1;
            info!(
                "[{}/{}] bought {} penguin nests",
                self.done, self.cfg.buys, NESTS_PER_BUY
            );
        }
        Ok(())
    }

    /// The modal remembers the last quantity bought, so Max is only tapped when 50/50 is
    /// not already shown. Returns false after cancelling when 50/50 never appears.
    fn ensure_max(&mut self) -> Result<bool> {
        if self.anchors.max.visible(&self.adb.screencap()?) {
            return Ok(true);
        }
        debug!("quantity is not 50/50, tapping Max");
        self.tap(Geometry::MAX)?;
        sleep(AFTER_MAX);
        if self.anchors.max.visible(&self.adb.screencap()?) {
            return Ok(true);
        }
        warn!("could not set quantity to 50/50, cancelling and stopping");
        self.tap(Geometry::CANCEL)?;
        self.stop.store(true, Ordering::SeqCst);
        Ok(false)
    }

    /// The penguin Buy button is gone: never tap into whatever screen replaced the altar.
    fn left_altar(&mut self) -> Result<()> {
        if self.done == 0 {
            bail!("Growth Altar not detected: open it in landscape and retry");
        }
        warn!("Growth Altar no longer visible, stopping");
        self.stop.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// No reward popup after confirming: the buy is not counted and the run stops.
    fn popup_missing(&mut self) -> Result<()> {
        warn!("no reward popup after buying, stopping (out of leaves?)");
        self.stop.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn tap(&self, (x, y): (u32, u32)) -> Result<()> {
        self.adb.tap(x, y)?;
        sleep(self.cfg.tap_sleep);
        Ok(())
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
