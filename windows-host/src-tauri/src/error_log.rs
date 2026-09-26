//! Bounded, best-effort error journal; audio threads never wait for disk I/O.
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    sync::mpsc::{sync_channel, SyncSender},
    time::Instant,
};
const LIMIT: u64 = 512 * 1024;

struct ErrorLogger(SyncSender<String>);
impl log::Log for ErrorLogger {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        meta.level() <= log::Level::Error
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let text = format!("{}: {}", record.target(), record.args());
            let _ = self.0.try_send(text.chars().take(2048).collect());
        }
    }
    fn flush(&self) {}
}

fn append(directory: &Path, line: &str) -> std::io::Result<()> {
    fs::create_dir_all(directory)?;
    let path = directory.join("errors.log");
    if fs::metadata(&path).map(|m| m.len()).unwrap_or(0) + line.len() as u64 > LIMIT {
        let previous = directory.join("errors.previous.log");
        if previous.exists() {
            fs::remove_file(&previous)?;
        }
        fs::rename(&path, previous)?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(line.as_bytes())
}

pub fn init() {
    if cfg!(debug_assertions) {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
        return;
    }
    let (tx, rx) = sync_channel::<String>(64);
    let _ = std::thread::Builder::new()
        .name("RoomWave-Errors".into())
        .spawn(move || {
            let Ok(directory) = crate::platform::log_dir() else {
                return;
            };
            let mut previous = String::new();
            let mut written = Instant::now();
            while let Ok(message) = rx.recv() {
                // Avoid filling the journal with a repeating error during a device failure.
                if message == previous && written.elapsed().as_secs() < 30 {
                    continue;
                }
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let line =
                    serde_json::json!({"unixMs":timestamp,"error":message}).to_string() + "\n";
                if append(&directory, &line).is_err() {
                    return;
                }
                previous = message;
                written = Instant::now();
            }
        });
    if log::set_boxed_logger(Box::new(ErrorLogger(tx))).is_ok() {
        log::set_max_level(log::LevelFilter::Error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn journal_rotates_and_keeps_only_one_previous_file() {
        let dir = std::env::temp_dir().join(format!(
            "roomwave-error-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let full = "x".repeat(LIMIT as usize);
        append(&dir, &full).unwrap();
        append(&dir, "next\n").unwrap();
        assert_eq!(
            fs::metadata(dir.join("errors.previous.log")).unwrap().len(),
            LIMIT
        );
        assert_eq!(
            fs::read_to_string(dir.join("errors.log")).unwrap(),
            "next\n"
        );
        append(&dir, &full).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("errors.previous.log")).unwrap(),
            "next\n"
        );
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        for name in ["errors.log", "errors.previous.log"] {
            fs::remove_file(dir.join(name)).unwrap();
        }
        fs::remove_dir(dir).unwrap();
    }
}
