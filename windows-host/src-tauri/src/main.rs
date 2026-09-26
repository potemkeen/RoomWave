// Release builds are desktop applications; diagnostic modes still support redirected stdout.
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod audio_types;
mod platform;
use platform::{default_output, local_test, virtual_probe, windows_audio};
mod audio;

mod diagnostics;
mod discovery;
mod error_log;
mod fec;
mod layout;

mod timing;
mod transport_stats;

use audio::{AudioSession, AudioState};
use discovery::{Discovery, DiscoverySnapshot};
use tauri::Manager;

#[tauri::command]
fn get_virtual_audio_state(
    output: tauri::State<'_, default_output::DefaultOutput>,
) -> default_output::Status {
    output.snapshot()
}

#[tauri::command]
fn get_diagnostic_log_state(
    log: tauri::State<'_, diagnostics::DiagnosticLog>,
) -> diagnostics::LogState {
    log.snapshot()
}

#[tauri::command]
fn start_diagnostic_log(
    app: tauri::AppHandle,
    log: tauri::State<'_, diagnostics::DiagnosticLog>,
) -> Result<(), String> {
    log.start(app)
}
#[tauri::command]
fn stop_diagnostic_log(log: tauri::State<'_, diagnostics::DiagnosticLog>) {
    log.stop();
}
#[tauri::command]
fn reveal_diagnostic_log(log: tauri::State<'_, diagnostics::DiagnosticLog>) -> Result<(), String> {
    let path = log
        .snapshot()
        .path
        .ok_or("Сначала запишите диагностический сеанс")?;
    platform::reveal_file(std::path::Path::new(&path)).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_audio_state(audio: tauri::State<'_, AudioSession>) -> Result<AudioState, String> {
    audio.snapshot()
}
#[tauri::command]
fn set_channel(
    device_id: String,
    speaker: Option<u32>,
    audio: tauri::State<'_, AudioSession>,
) -> Result<(), String> {
    audio.set_channel(device_id, speaker)
}
#[tauri::command]
async fn test_speaker(speaker: u32, audio: tauri::State<'_, AudioSession>) -> Result<(), String> {
    let routed = audio.routed_local_test();
    let guard = audio.local_test_guard(speaker)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        if routed {
            std::thread::sleep(std::time::Duration::from_millis(750));
            Ok(())
        } else {
            local_test::play(speaker)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn set_local_output(
    config: audio::LocalOutputConfig,
    audio: tauri::State<'_, AudioSession>,
) -> Result<(), String> {
    audio.set_local_output(config)
}

#[tauri::command]
async fn connect_device(
    device_id: String,
    discovery: tauri::State<'_, Discovery>,
    audio: tauri::State<'_, AudioSession>,
) -> Result<(), String> {
    let device = discovery
        .snapshot()?
        .devices
        .into_iter()
        .find(|d| d.device_id == device_id)
        .ok_or("Device is no longer available")?;
    audio.connect(device)
}

#[tauri::command]
async fn disconnect_audio(
    device_id: Option<String>,
    audio: tauri::State<'_, AudioSession>,
) -> Result<(), String> {
    audio.disconnect(device_id)
}

#[tauri::command]
fn get_discovery_state(
    discovery: tauri::State<'_, Discovery>,
) -> Result<DiscoverySnapshot, String> {
    discovery.snapshot()
}

#[tauri::command]
fn refresh_discovery(discovery: tauri::State<'_, Discovery>) -> Result<(), String> {
    discovery.refresh()
}

fn main() {
    if default_output::guardian_entry() {
        return;
    }
    error_log::init();
    let default_output = default_output::DefaultOutput::prepare();
    let audio = AudioSession::new(&default_output.snapshot());
    if default_output.snapshot().endpoint_id.is_some() {
        if audio.wait_for_capture() {
            if let Err(error) = default_output.activate() {
                log::error!("Default output: {error}");
            }
        } else {
            log::error!("Virtual audio capture is not ready; keeping the Windows default output");
            default_output.cancel("Не удалось подготовить захват RoomWave Virtual Speakers. Устройство Windows не переключено.");
        }
    }
    let discovery = Discovery::start().expect("Could not start discovery worker");
    tauri::Builder::default()
        .manage(discovery)
        .manage(audio)
        .manage(default_output)
        .setup(|app| {
            app.manage(diagnostics::DiagnosticLog::new());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_virtual_audio_state,
            get_diagnostic_log_state,
            start_diagnostic_log,
            stop_diagnostic_log,
            reveal_diagnostic_log,
            get_discovery_state,
            refresh_discovery,
            get_audio_state,
            connect_device,
            disconnect_audio,
            set_channel,
            test_speaker,
            set_local_output
        ])
        .build(tauri::generate_context!())
        .expect("Could not run RoomWave Host")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                app.state::<default_output::DefaultOutput>().stop();
                app.state::<diagnostics::DiagnosticLog>().stop();
            }
        });
}
