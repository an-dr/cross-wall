//! An optional log file for diagnosing problems in the field. Off by default in
//! release builds, on by default in development, and switchable in Settings.
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use bones_kernel::logging::{Level, LogSink};

/// Past this the old log is set aside, so logging can be left on.
const MAX_BYTES: u64 = 5 * 1024 * 1024;

static ENABLED: AtomicBool = AtomicBool::new(false);
static WRITER: Mutex<()> = Mutex::new(());

pub fn default_enabled() -> bool {
    cfg!(debug_assertions)
}

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn path() -> PathBuf {
    crate::settings::data_dir().join("app.log")
}

pub fn info(category: &str, message: &str) {
    write(&path(), "INFO", category, message);
}

fn write(file: &std::path::Path, level: &str, category: &str, message: &str) {
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let Ok(_guard) = WRITER.lock() else { return };
    if std::fs::metadata(file).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(file, file.with_extension("old.log"));
    }
    if let Some(folder) = file.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    if let Ok(mut log) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        let _ = writeln!(log, "{} {level} [{category}] {message}", crate::librarian::now());
    }
}

/// Sends the engine's own log lines to the same file.
pub struct FileSink;

impl LogSink for FileSink {
    fn log(&self, level: Level, category: &str, message: &str) {
        write(&path(), &format!("{level:?}").to_uppercase(), category, message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_only_when_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.log");
        set_enabled(false);
        write(&file, "INFO", "x", "hidden");
        assert!(!file.exists());
        set_enabled(true);
        write(&file, "INFO", "x", "shown");
        set_enabled(false);
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("[x] shown") && !text.contains("hidden"));
    }
}
