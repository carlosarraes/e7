use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use rand::Rng;
use tracing::{debug, info, warn};

use crate::adb::Adb;
use crate::history::History;
use crate::item::Item;
use crate::matcher::{Anchor, Hit, Matcher, SCREEN_H, SCREEN_W};

const SETTLE: Duration = Duration::from_millis(1500);
const AFTER_SWIPE: Duration = Duration::from_secs(1);
const AFTER_BUY: Duration = Duration::from_secs(1);

#[derive(Clone, Copy)]
pub struct Frac(pub f64, pub f64);

impl Frac {
    pub fn px(self) -> (u32, u32) {
        (
            (self.0 * SCREEN_W as f64) as u32,
            (self.1 * SCREEN_H as f64) as u32,
        )
    }
}

/// Tap targets as fractions of 1920x1080, carried over from the Python script.
pub struct Geometry;

impl Geometry {
    pub const BUY_OFFSET: Frac = Frac(0.4718, 0.1000);
    pub const BUY_CONFIRM: Frac = Frac(0.5677, 0.7037);
    pub const REFRESH: Frac = Frac(0.1698, 0.9138);
    pub const REFRESH_CONFIRM: Frac = Frac(0.5828, 0.6411);
    pub const SWIPE: (Frac, Frac) = (Frac(0.6250, 0.7481), Frac(0.6250, 0.3629));

    pub fn buy_point(hit: &Hit) -> (u32, u32) {
        let (dx, dy) = Self::BUY_OFFSET.px();
        (hit.x + dx, hit.y + dy)
    }
}

pub fn jitter((x, y): (u32, u32)) -> (u32, u32) {
    let mut rng = rand::rng();
    let dx: i32 = rng.random_range(-75..=75);
    let dy: i32 = rng.random_range(-25..=25);
    ((x as i32 + dx).max(0) as u32, (y as i32 + dy).max(0) as u32)
}

pub struct Config {
    pub refreshes: u32,
    pub items: Vec<Item>,
    pub dry_run: bool,
    pub tap_sleep: Duration,
    pub jitter: bool,
}

pub struct Summary {
    pub refreshes: u32,
    pub counts: Vec<(Item, u32)>,
    pub gold: u64,
    pub duration: Duration,
    pub stopped: bool,
}

pub struct Runner {
    adb: Adb,
    matcher: Matcher,
    anchor: Anchor,
    cfg: Config,
    stop: Arc<AtomicBool>,
    history: Option<History>,
    counts: Vec<(Item, u32)>,
    done: u32,
}

impl Runner {
    pub fn new(
        adb: Adb,
        matcher: Matcher,
        anchor: Anchor,
        cfg: Config,
        stop: Arc<AtomicBool>,
        history: Option<History>,
    ) -> Self {
        let counts = cfg.items.iter().map(|&i| (i, 0)).collect();
        Self {
            adb,
            matcher,
            anchor,
            cfg,
            stop,
            history,
            counts,
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
        let gold = self
            .counts
            .iter()
            .map(|(i, n)| u64::from(i.gold()) * u64::from(*n))
            .sum();
        if let Some(h) = &mut self.history {
            h.run_end(self.done, gold)?;
        }
        Ok(Summary {
            refreshes: self.done,
            counts: self.counts,
            gold,
            duration: start.elapsed(),
            stopped: self.stop.load(Ordering::SeqCst),
        })
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Inspect the current page, refresh, repeat: `refreshes` refreshes, `refreshes + 1` inspections.
    fn live(&mut self) -> Result<()> {
        loop {
            if self.stopped() {
                return Ok(());
            }
            sleep(SETTLE);
            let mut bought = HashSet::new();
            if !self.scan_and_buy(&mut bought)? {
                return self.left_shop();
            }
            if self.stopped() {
                return Ok(());
            }
            let (a, b) = Geometry::SWIPE;
            let ((x1, y1), (x2, y2)) = (a.px(), b.px());
            self.adb.swipe(x1, y1, x2, y2)?;
            sleep(AFTER_SWIPE);
            if !self.scan_and_buy(&mut bought)? {
                return self.left_shop();
            }
            if self.stopped() || self.done >= self.cfg.refreshes {
                return Ok(());
            }
            self.tap(Geometry::REFRESH.px())?;
            self.tap(Geometry::REFRESH_CONFIRM.px())?;
            self.done += 1;
            debug!("refresh {}/{}", self.done, self.cfg.refreshes);
        }
    }

    /// The Refresh button is gone: never tap into whatever screen replaced the shop.
    fn left_shop(&mut self) -> Result<()> {
        if self.done == 0 {
            bail!("Secret Shop not detected: open it in landscape and retry");
        }
        warn!("Secret Shop no longer visible, stopping");
        self.stop.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Returns false when the screen is not the Secret Shop (nothing is tapped then).
    fn scan_and_buy(&mut self, bought: &mut HashSet<Item>) -> Result<bool> {
        let screen = self.adb.screencap()?;
        if !self.anchor.visible(&screen) {
            return Ok(false);
        }
        let hits: Vec<Hit> = {
            let scan = self.matcher.scan(&screen);
            self.cfg
                .items
                .iter()
                .filter(|i| !bought.contains(i))
                .filter_map(|&i| scan.find(i))
                .collect()
        };
        for hit in hits {
            if self.stopped() {
                break;
            }
            debug!(
                "{} at ({}, {}) score {:.3}",
                hit.item.name(),
                hit.x,
                hit.y,
                hit.score
            );
            self.tap(Geometry::buy_point(&hit))?;
            self.tap(Geometry::BUY_CONFIRM.px())?;
            sleep(AFTER_BUY);
            bought.insert(hit.item);
            self.record(hit.item)?;
        }
        Ok(true)
    }

    fn record(&mut self, item: Item) -> Result<()> {
        if let Some((_, n)) = self.counts.iter_mut().find(|(i, _)| *i == item) {
            *n += 1;
        }
        info!(
            "[{}/{}] {} → bought",
            self.done,
            self.cfg.refreshes,
            item.name()
        );
        if let Some(h) = &mut self.history {
            h.bought(self.done, item)?;
        }
        Ok(())
    }

    fn tap(&self, point: (u32, u32)) -> Result<()> {
        let (x, y) = if self.cfg.jitter {
            jitter(point)
        } else {
            point
        };
        self.adb.tap(x, y)?;
        sleep(self.cfg.tap_sleep);
        Ok(())
    }

    /// Screenshot + match + log, no input at all. `refreshes` = number of screenshots.
    fn dry_run(&mut self) -> Result<()> {
        while self.done < self.cfg.refreshes && !self.stopped() {
            let screen = self.adb.screencap()?;
            self.done += 1;
            if !self.anchor.visible(&screen) {
                warn!(
                    "[{}/{}] Secret Shop not visible (refresh button score {:.3})",
                    self.done,
                    self.cfg.refreshes,
                    self.anchor.score(&screen)
                );
            }
            let bests: Vec<Hit> = {
                let scan = self.matcher.scan(&screen);
                self.cfg
                    .items
                    .iter()
                    .filter_map(|&i| scan.best(i))
                    .collect()
            };
            for best in bests {
                if best.score >= self.matcher.threshold() {
                    info!(
                        "[{}/{}] {} found at ({}, {}) score {:.3}",
                        self.done,
                        self.cfg.refreshes,
                        best.item.name(),
                        best.x,
                        best.y,
                        best.score
                    );
                    if let Some((_, n)) = self.counts.iter_mut().find(|(i, _)| *i == best.item) {
                        *n += 1;
                    }
                } else {
                    debug!(
                        "[{}/{}] {} best {:.3} at ({}, {})",
                        self.done,
                        self.cfg.refreshes,
                        best.item.name(),
                        best.score,
                        best.x,
                        best.y
                    );
                }
            }
            sleep(SETTLE);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buy_point_matches_python_offsets() {
        let hit = Hit {
            item: Item::Cov,
            x: 831,
            y: 902,
            score: 1.0,
        };
        assert_eq!(Geometry::buy_point(&hit), (831 + 905, 902 + 108));
    }

    #[test]
    fn fixed_points_match_python_fractions() {
        assert_eq!(Geometry::BUY_CONFIRM.px(), (1089, 759));
        assert_eq!(Geometry::REFRESH.px(), (326, 986));
        assert_eq!(Geometry::REFRESH_CONFIRM.px(), (1118, 692));
        assert_eq!(Geometry::SWIPE.0.px(), (1200, 807));
        assert_eq!(Geometry::SWIPE.1.px(), (1200, 391));
    }

    #[test]
    fn jitter_stays_within_bounds() {
        for _ in 0..1000 {
            let (x, y) = jitter((500, 500));
            assert!((425..=575).contains(&x) && (475..=525).contains(&y));
        }
    }
}
