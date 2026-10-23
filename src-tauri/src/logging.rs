use chrono::Local;
use log::{Level, LevelFilter, Log, Metadata, Record};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::config::config_dir;

const FILE_NAME: &str = "agent-backup.log";
const ROTATED_NAME: &str = "agent-backup.1.log";
const MAX_BYTES: u64 = 1024 * 1024;
const OWN_TARGET: &str = "agent_backup_lib";

pub fn log_dir() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("logs"))
}

/// Appends to `logs/agent-backup.log`, keeping one rotated file. Release builds have
/// no console, so this file is the only record of background failures.
struct FileLogger {
    dir: PathBuf,
    max_bytes: u64,
    file: Mutex<Option<File>>,
}

impl FileLogger {
    fn new(dir: PathBuf, max_bytes: u64) -> Self {
        Self {
            dir,
            max_bytes,
            file: Mutex::new(None),
        }
    }

    fn write_line(&self, line: &str) {
        let mut slot = self.file.lock().unwrap_or_else(|p| p.into_inner());
        let full = slot
            .as_ref()
            .and_then(|f| f.metadata().ok())
            .is_some_and(|m| m.len() >= self.max_bytes);
        if full {
            *slot = None;
            let _ = fs::rename(self.dir.join(FILE_NAME), self.dir.join(ROTATED_NAME));
        }
        if slot.is_none() {
            *slot = open_append(&self.dir);
        }
        if let Some(file) = slot.as_mut() {
            let _ = file.write_all(line.as_bytes());
        }
    }
}

fn open_append(dir: &Path) -> Option<File> {
    fs::create_dir_all(dir).ok()?;
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(FILE_NAME))
        .ok()
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        let max = if metadata.target().starts_with(OWN_TARGET) {
            Level::Info
        } else {
            Level::Warn
        };
        metadata.level() <= max
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let target = record.target();
        let module = target.strip_prefix(OWN_TARGET).map_or(target, |t| t.trim_start_matches("::"));
        let line = format!(
            "{} {:<5} [{}] {}\n",
            Local::now().format("%Y-%m-%d %H:%M:%S"),
            record.level(),
            if module.is_empty() { "app" } else { module },
            record.args()
        );
        if cfg!(debug_assertions) {
            eprint!("{line}");
        }
        self.write_line(&line);
    }

    fn flush(&self) {
        if let Some(file) = self.file.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
            let _ = file.flush();
        }
    }
}

/// Installs the file logger and routes panics into it.
pub fn init() {
    let Ok(dir) = log_dir() else {
        return;
    };
    if log::set_boxed_logger(Box::new(FileLogger::new(dir, MAX_BYTES))).is_err() {
        return;
    }
    log::set_max_level(LevelFilter::Info);
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("panic: {info}");
        default_hook(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_into_a_single_backup_file() {
        let dir = std::env::temp_dir().join(format!("agent-backup-log-{}", uuid::Uuid::new_v4()));
        let logger = FileLogger::new(dir.clone(), 100);
        for i in 0..30 {
            logger.write_line(&format!("line {i:02}\n"));
        }
        let current = fs::read_to_string(dir.join(FILE_NAME)).unwrap();
        let rotated = fs::read_to_string(dir.join(ROTATED_NAME)).unwrap();
        assert!(current.ends_with("line 29\n"), "{current}");
        assert!(rotated.len() as u64 >= 100 && rotated.len() < 120, "{rotated}");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        let _ = fs::remove_dir_all(&dir);
    }
}
