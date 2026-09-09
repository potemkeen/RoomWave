mod audio;
mod discovery;
mod layout;
mod local_test;
mod timing;

use audio::{AudioSession, AudioState};
use discovery::{Discovery, DiscoverySnapshot};

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
    // Keep the console in Phase 1, including release builds, for useful LAN diagnostics.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let discovery = Discovery::start().expect("Could not start discovery worker");
    tauri::Builder::default()
        .manage(discovery)
        .manage(AudioSession::new())
        .invoke_handler(tauri::generate_handler![
            get_discovery_state,
            refresh_discovery,
            get_audio_state,
            connect_device,
            disconnect_audio,
            set_channel,
            test_speaker,
            set_local_output
        ])
        .run(tauri::generate_context!())
        .expect("Could not run RoomWave Host");
}
