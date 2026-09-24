//! Platform audio workers; compiled selection, no dynamic dispatch in the hot path.
//! Contract: publish canonical 48 kHz PCM blocks with the shared host-clock timeline.
use super::*;
#[cfg(target_os = "windows")]
#[path = "windows/capture.rs"]
mod capture;
#[cfg(target_os = "windows")]
#[path = "windows/local_output.rs"]
pub(crate) mod local_output;
#[cfg(target_os = "windows")]
pub(super) use capture::capture_audio;
