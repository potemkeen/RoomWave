use crate::audio::AudioSession;
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

const SESSION_DURATION: Duration = Duration::from_secs(10 * 60);
const LIMIT_BYTES: u64 = 64 * 1024 * 1024;
fn session_active(stopped: bool, elapsed: Duration) -> bool {
    !stopped && elapsed < SESSION_DURATION
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogState {
    pub path: Option<String>,
    pub recording: bool,
    pub samples: u64,
    pub error: Option<String>,
    pub ends_at_unix_ms: Option<u128>,
}
pub struct DiagnosticLog {
    state: Arc<Mutex<LogState>>,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}
fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
fn write_record(writer: &mut impl Write, value: &Value) -> std::io::Result<u64> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    writer.write_all(&bytes)?;
    writer.flush()?;
    Ok(bytes.len() as u64)
}
impl DiagnosticLog {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(LogState::default())),
            stop: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
        }
    }
    pub fn start(&self, app: tauri::AppHandle) -> Result<(), String> {
        let mut worker = self.worker.lock().unwrap();
        if worker.as_ref().is_some_and(|w| !w.is_finished()) {
            return Ok(());
        }
        if let Some(previous) = worker.take() { let _ = previous.join(); }
        self.stop.store(false, Ordering::Relaxed);
        *self.state.lock().unwrap() = LogState {
            recording: true,
            ends_at_unix_ms: Some(unix_ms() + SESSION_DURATION.as_millis()),
            ..LogState::default()
        };
        let state = self.state.clone();
        let stop = self.stop.clone();
        match thread::Builder::new().name("RoomWave-Diagnostics".into()).spawn(move || {
            let result = record(app, &state, &stop);
            let mut status = state.lock().unwrap();
            status.recording = false;
            status.ends_at_unix_ms = None;
            if let Err(error) = result { status.error = Some(error.to_string()); }
        }) {
            Ok(handle) => { *worker = Some(handle); Ok(()) }
            Err(error) => {
                let mut status = self.state.lock().unwrap();
                status.recording = false;
                status.ends_at_unix_ms = None;
                status.error = Some(error.to_string());
                Err(error.to_string())
            }
        }
    }
    pub fn snapshot(&self) -> LogState {
        self.state.lock().unwrap().clone()
    }
    pub fn stop(&self) {
        let mut handle = self.worker.lock().unwrap();
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = handle.take() {
            let _ = worker.join();
        }
    }
}
fn record(
    app: tauri::AppHandle,
    state: &Mutex<LogState>,
    stop: &AtomicBool,
) -> Result<(), Box<dyn std::error::Error>> {
    let directory = crate::platform::log_dir()?;
    fs::create_dir_all(&directory)?;
    let started = unix_ms();
    let path = directory.join(format!("session-{started}-{}.jsonl", std::process::id()));
    let file: File = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let mut writer = BufWriter::new(file);
    let mut size = write_record(
        &mut writer,
        &json!({"type":"session_start", "schemaVersion":1,
        "unixMs":started, "version":env!("CARGO_PKG_VERSION"), "sampleIntervalMs":500,
        "pcmRecorded":false, "counterSemantics":"cumulative per connection; counters may reset on reconnect",
        "latencyScope":"WASAPI read to reported presentation; excludes pre-capture and acoustic latency"}),
    )?;
    {
        let mut status = state.lock().unwrap();
        status.path = Some(path.to_string_lossy().into_owned());
        status.recording = true;
    }
    let clock = Instant::now();
    let mut last_mode_revision = None;
    while session_active(stop.load(Ordering::Relaxed), clock.elapsed()) {
        // Copy state under short existing locks; serialize and write only after releasing them.
        // This worker runs even with the window minimized and never handles PCM buffers.
        let snapshot = app.state::<AudioSession>().snapshot();
        let record = match snapshot {
            Ok(audio) => {
                if last_mode_revision != Some(audio.mode_revision) {
                    size += write_record(
                        &mut writer,
                        &json!({"type":"mode_changed","unixMs":unix_ms(),"elapsedMs":clock.elapsed().as_millis(),
                        "mode":audio.stream_mode,"modeRevision":audio.mode_revision,"changedUnixMs":audio.mode_changed_unix_ms,
                        "initial":last_mode_revision.is_none(),"receivers":audio.receivers}),
                    )?;
                    last_mode_revision = Some(audio.mode_revision);
                }
                json!({"type":"sample", "unixMs":unix_ms(), "elapsedMs":clock.elapsed().as_millis(), "audio":audio,
                    "virtualAudio":app.state::<crate::default_output::DefaultOutput>().snapshot()})
            }
            Err(error) => {
                json!({"type":"sample_error", "unixMs":unix_ms(), "elapsedMs":clock.elapsed().as_millis(), "error":error})
            }
        };
        size += write_record(&mut writer, &record)?;
        state.lock().unwrap().samples += 1;
        if size >= LIMIT_BYTES {
            write_record(
                &mut writer,
                &json!({"type":"session_end","unixMs":unix_ms(),"reason":"size_limit"}),
            )?;
            return Err("Лог достиг лимита 64 МБ. Запустите новый сеанс при необходимости.".into());
        }
        thread::sleep(Duration::from_millis(500));
    }
    write_record(
        &mut writer,
        &json!({"type":"session_end","unixMs":unix_ms(),"elapsedMs":clock.elapsed().as_millis(),"reason":if stop.load(Ordering::Relaxed) { "stopped" } else { "time_limit" }}),
    )?;
    state.lock().unwrap().recording = false;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_stops_at_deadline_or_manual_stop() {
        assert!(session_active(false, SESSION_DURATION - Duration::from_millis(1)));
        assert!(!session_active(false, SESSION_DURATION));
        assert!(!session_active(false, SESSION_DURATION + Duration::from_secs(1)));
        assert!(!session_active(true, Duration::ZERO));
    }
    #[test]
    fn logging_is_idle_until_requested() {
        let log = DiagnosticLog::new();
        assert!(!log.snapshot().recording);
        assert!(log.snapshot().path.is_none());
        assert!(log.worker.lock().unwrap().is_none());
        log.stop();
        log.stop();
    }
    #[test]
    fn records_are_independently_readable_json_lines() {
        let mut bytes = Vec::new();
        let first = json!({"type":"sample","error":"line one\nline two","audio":{"receivers":[]}});
        let second = json!({"type":"session_end","reason":"application_exit"});
        let size =
            write_record(&mut bytes, &first).unwrap() + write_record(&mut bytes, &second).unwrap();
        assert_eq!(size, bytes.len() as u64);
        let text = String::from_utf8(bytes).unwrap();
        let records: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records, vec![first, second]);
        assert!(text.ends_with('\n'));
    }
}
