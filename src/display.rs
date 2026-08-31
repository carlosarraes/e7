use std::thread::sleep;
use std::time::Duration;

use anyhow::{Result, bail};
use tracing::{info, warn};

use crate::adb::Adb;
use crate::matcher::{SCREEN_H, SCREEN_W};

/// Forces a 1920x1080 logical display for the lifetime of the guard.
pub struct DisplayGuard {
    adb: Adb,
    overridden: bool,
}

impl DisplayGuard {
    pub fn acquire(adb: &Adb, allow_override: bool) -> Result<Self> {
        let mut guard = Self {
            adb: adb.clone(),
            overridden: false,
        };
        let (w, h) = adb.screencap()?.dimensions();
        if (w, h) == (SCREEN_W, SCREEN_H) {
            return Ok(guard);
        }
        if !allow_override {
            bail!(
                "screen is {w}x{h}, need {SCREEN_W}x{SCREEN_H} (drop --no-display-override or set it on the device)"
            );
        }
        info!("screen is {w}x{h}; overriding display size to {SCREEN_W}x{SCREEN_H}");
        adb.wm_size_set(SCREEN_H, SCREEN_W)?;
        guard.overridden = true;
        sleep(Duration::from_secs(2));
        let (w, h) = adb.screencap()?.dimensions();
        if (w, h) != (SCREEN_W, SCREEN_H) {
            guard.reset();
            bail!("display override failed, screen is {w}x{h} (is the game open in landscape?)");
        }
        Ok(guard)
    }

    pub fn reset(&mut self) {
        if !self.overridden {
            return;
        }
        match self.adb.wm_size_reset() {
            Ok(()) => info!("display size restored"),
            Err(e) => warn!("could not restore display size: {e:#} (run: adb shell wm size reset)"),
        }
        self.overridden = false;
    }
}

impl Drop for DisplayGuard {
    fn drop(&mut self) {
        self.reset();
    }
}
