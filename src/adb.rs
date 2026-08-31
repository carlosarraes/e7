use std::path::PathBuf;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail, ensure};
use image::RgbImage;
use tracing::{trace, warn};

const ATTEMPTS: u32 = 3;
const BACKOFF: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct Adb {
    path: PathBuf,
    serial: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub serial: String,
    pub state: String,
}

impl Adb {
    pub fn new(serial: Option<String>) -> Result<Self> {
        let path = std::env::var_os("PATH")
            .and_then(|p| {
                std::env::split_paths(&p)
                    .map(|d| d.join("adb"))
                    .find(|c| c.is_file())
            })
            .ok_or_else(|| anyhow!("adb not found on PATH (install android-tools)"))?;
        Ok(Self { path, serial })
    }

    /// Connect to the only online device, or to `serial` when given.
    pub fn pick(serial: Option<String>) -> Result<Self> {
        let adb = Self::new(None)?;
        let devices = adb.devices()?;
        let chosen = choose(&devices, serial)?;
        Ok(Self {
            serial: Some(chosen),
            ..adb
        })
    }

    pub fn serial(&self) -> Option<&str> {
        self.serial.as_deref()
    }

    pub fn devices(&self) -> Result<Vec<Device>> {
        let out = self.run(&["devices"])?;
        Ok(parse_devices(&String::from_utf8_lossy(&out)))
    }

    pub fn screencap(&self) -> Result<RgbImage> {
        retry("screencap", || {
            let png = self.run_once(&["exec-out", "screencap", "-p"])?;
            ensure!(!png.is_empty(), "screencap returned no data");
            Ok(image::load_from_memory(&png)
                .context("decode screencap png")?
                .to_rgb8())
        })
    }

    pub fn tap(&self, x: u32, y: u32) -> Result<()> {
        self.run(&["shell", "input", "tap", &x.to_string(), &y.to_string()])
            .map(drop)
    }

    pub fn swipe(&self, x1: u32, y1: u32, x2: u32, y2: u32) -> Result<()> {
        self.run(&[
            "shell",
            "input",
            "swipe",
            &x1.to_string(),
            &y1.to_string(),
            &x2.to_string(),
            &y2.to_string(),
        ])
        .map(drop)
    }

    pub fn wm_size_set(&self, w: u32, h: u32) -> Result<()> {
        self.run(&["shell", "wm", "size", &format!("{w}x{h}")])
            .map(drop)
    }

    pub fn wm_size_reset(&self) -> Result<()> {
        self.run(&["shell", "wm", "size", "reset"]).map(drop)
    }

    fn run(&self, args: &[&str]) -> Result<Vec<u8>> {
        retry(args.join(" ").as_str(), || self.run_once(args))
    }

    fn run_once(&self, args: &[&str]) -> Result<Vec<u8>> {
        let mut cmd = Command::new(&self.path);
        if let Some(s) = &self.serial {
            cmd.args(["-s", s]);
        }
        cmd.args(args);
        trace!(?cmd, "adb");
        let out = cmd
            .output()
            .with_context(|| format!("spawn {}", self.path.display()))?;
        if !out.status.success() {
            bail!(
                "adb {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(out.stdout)
    }
}

fn retry<T>(what: &str, mut op: impl FnMut() -> Result<T>) -> Result<T> {
    let mut last = None;
    for attempt in 1..=ATTEMPTS {
        match op() {
            Ok(v) => return Ok(v),
            Err(e) => {
                warn!("adb {what}: attempt {attempt}/{ATTEMPTS} failed: {e:#}");
                last = Some(e);
                if attempt < ATTEMPTS {
                    sleep(BACKOFF);
                }
            }
        }
    }
    Err(last.expect("at least one attempt"))
        .context(format!("adb {what} failed after {ATTEMPTS} attempts"))
}

pub fn parse_devices(out: &str) -> Vec<Device> {
    out.lines()
        .skip(1)
        .filter_map(|l| {
            let (serial, state) = l.split_once('\t')?;
            Some(Device {
                serial: serial.trim().to_string(),
                state: state.trim().to_string(),
            })
        })
        .collect()
}

pub fn choose(devices: &[Device], serial: Option<String>) -> Result<String> {
    let listing = || {
        devices
            .iter()
            .map(|d| format!("  {}\t{}", d.serial, d.state))
            .collect::<Vec<_>>()
            .join("\n")
    };
    if let Some(s) = serial {
        let d = devices
            .iter()
            .find(|d| d.serial == s)
            .ok_or_else(|| anyhow!("device {s} not found:\n{}", listing()))?;
        ensure!(d.state == "device", "device {s} is {}", d.state);
        return Ok(s);
    }
    let online: Vec<&Device> = devices.iter().filter(|d| d.state == "device").collect();
    match online.as_slice() {
        [d] => Ok(d.serial.clone()),
        [] => bail!("no online adb device (adb devices):\n{}", listing()),
        _ => bail!("several devices attached, pass --device:\n{}", listing()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_adb_devices_output() {
        let out =
            "List of devices attached\n192.168.15.14:5555\tdevice\nemulator-5554\toffline\n\n";
        let d = parse_devices(out);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].serial, "192.168.15.14:5555");
        assert_eq!(d[0].state, "device");
        assert_eq!(d[1].state, "offline");
    }

    #[test]
    fn parses_empty_list() {
        assert!(parse_devices("List of devices attached\n\n").is_empty());
    }

    fn dev(serial: &str, state: &str) -> Device {
        Device {
            serial: serial.into(),
            state: state.into(),
        }
    }

    #[test]
    fn choose_single_online_device() {
        assert_eq!(choose(&[dev("a", "device")], None).unwrap(), "a");
    }

    #[test]
    fn choose_requires_serial_when_ambiguous() {
        let d = vec![dev("a", "device"), dev("b", "device")];
        assert!(choose(&d, None).is_err());
        assert_eq!(choose(&d, Some("b".into())).unwrap(), "b");
        assert!(choose(&d, Some("zzz".into())).is_err());
    }

    #[test]
    fn choose_rejects_offline() {
        assert!(choose(&[dev("a", "offline")], None).is_err());
    }
}
