use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use crate::item::Item;

const HEADER: [&str; 6] = ["timestamp", "run_id", "refresh", "event", "item", "gold"];

pub struct History {
    writer: csv::Writer<File>,
    run_id: String,
}

pub fn default_path() -> Result<PathBuf> {
    Ok(dirs::data_dir()
        .ok_or_else(|| anyhow!("no data directory"))?
        .join("e7")
        .join("history.csv"))
}

pub fn new_run_id() -> String {
    chrono::Local::now().format("%Y%m%d-%H%M%S").to_string()
}

impl History {
    pub fn open(run_id: &str) -> Result<Self> {
        Self::open_at(&default_path()?, run_id)
    }

    pub fn open_at(path: &Path, run_id: &str) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        let fresh = std::fs::metadata(path)
            .map(|m| m.len() == 0)
            .unwrap_or(true);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("open {}", path.display()))?;
        let mut writer = csv::WriterBuilder::new()
            .has_headers(false)
            .from_writer(file);
        if fresh {
            writer.write_record(HEADER)?;
            writer.flush()?;
        }
        Ok(Self {
            writer,
            run_id: run_id.to_string(),
        })
    }

    pub fn bought(&mut self, refresh: u32, item: Item) -> Result<()> {
        self.write(refresh, "bought", item.key(), u64::from(item.gold()))
    }

    pub fn run_end(&mut self, refresh: u32, gold: u64) -> Result<()> {
        self.write(refresh, "run_end", "", gold)
    }

    fn write(&mut self, refresh: u32, event: &str, item: &str, gold: u64) -> Result<()> {
        let ts = chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
        self.writer.write_record([
            ts.as_str(),
            &self.run_id,
            &refresh.to_string(),
            event,
            item,
            &gold.to_string(),
        ])?;
        self.writer.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_header_once_and_rows_incrementally() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.csv");
        {
            let mut h = History::open_at(&path, "run1").unwrap();
            h.bought(3, Item::Cov).unwrap();
        }
        {
            let mut h = History::open_at(&path, "run2").unwrap();
            h.run_end(10, 184_000).unwrap();
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "timestamp,run_id,refresh,event,item,gold");
        assert_eq!(lines.len(), 3);
        assert!(lines[1].ends_with(",run1,3,bought,covenant,184000"), "{}", lines[1]);
        assert!(lines[2].ends_with(",run2,10,run_end,,184000"), "{}", lines[2]);
    }
}
