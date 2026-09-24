//! OS services. No network protocol or channel-routing policy belongs here.
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::*;
#[cfg(not(target_os = "windows"))]
compile_error!("RoomWave: implement the macOS platform and audio_backend described in docs/macos-host-plan.md before building this target");
