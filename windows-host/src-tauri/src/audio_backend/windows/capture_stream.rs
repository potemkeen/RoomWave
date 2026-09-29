//! Capture initialization adapters. No device activation or format queries in read/wait.
use std::collections::VecDeque;

use serde_json::{json, Value};
use wasapi::{BufferInfo, Direction, StreamMode, WaveFormat};
use windows::{
    core::{Interface, HSTRING},
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
        Media::Audio::{
            IAudioCaptureClient, IAudioClient, IAudioClient3, IMMDeviceEnumerator,
            MMDeviceEnumerator, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK,
        },
        System::{
            Com::{CoCreateInstance, CLSCTX_ALL},
            Threading::{CreateEventW, WaitForSingleObject},
        },
    },
};

use super::Res;

const REQUESTED_PERIOD: u32 = 240;

// Fields are dropped in order: clients must release their event reference before its handle closes.
pub(super) enum CaptureStream {
    LowLatency {
        capture: IAudioCaptureClient,
        client: IAudioClient3,
        event: Event,
        align: usize,
    },
    Legacy {
        capture: wasapi::AudioCaptureClient,
        client: wasapi::AudioClient,
        event: wasapi::Handle,
    },
}

pub(super) struct Event(HANDLE);
impl Drop for Event {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

fn validate_period(requested: u32, fundamental: u32, min: u32, max: u32) -> Res<()> {
    if fundamental == 0 || requested < min || requested > max || requested % fundamental != 0 {
        return Err(format!(
            "unsupported period {requested} frames (fundamental={fundamental}, min={min}, max={max})"
        ).into());
    }
    Ok(())
}

fn stream_flags(direction: Direction) -> u32 {
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK
        | if direction == Direction::Render {
            AUDCLNT_STREAMFLAGS_LOOPBACK
        } else {
            0
        }
}

fn with_fallback<T>(
    primary: impl FnOnce() -> Res<T>,
    fallback: impl FnOnce() -> Res<T>,
) -> Res<(T, Option<String>)> {
    match primary() {
        Ok(stream) => Ok((stream, None)),
        Err(error) => {
            let reason = error.to_string();
            fallback()
                .map(|stream| (stream, Some(reason.clone())))
                .map_err(|error| {
                    format!("IAudioClient3: {reason}; shared-event fallback: {error}").into()
                })
        }
    }
}

// The caller owns the WASAPI buffer until ReleaseBuffer. Silent packets may have no data pointer.
unsafe fn append_pcm(pcm: &mut VecDeque<u8>, data: *const u8, bytes: usize, silent: bool) {
    if silent {
        pcm.extend(std::iter::repeat_n(0, bytes));
    } else if bytes > 0 {
        pcm.extend(std::slice::from_raw_parts(data, bytes).iter().copied());
    }
}

impl CaptureStream {
    pub(super) fn open(
        device: &wasapi::Device,
        id: &str,
        format: &WaveFormat,
    ) -> Res<(Self, Value)> {
        let ((stream, buffer), reason) = with_fallback(
            || Self::low_latency(id, device.get_direction(), format),
            || {
                // A failed Initialize must never be retried on the same COM client.
                let mut client = device.get_iaudioclient()?;
                client.initialize_client(
                    format,
                    &Direction::Capture,
                    &StreamMode::EventsShared {
                        autoconvert: true,
                        buffer_duration_hns: 200_000,
                    },
                )?;
                let event = client.set_get_eventhandle()?;
                let capture = client.get_audiocaptureclient()?;
                let buffer = client.get_buffer_size()?;
                Ok((
                    Self::Legacy {
                        capture,
                        client,
                        event,
                    },
                    buffer,
                ))
            },
        )?;
        // Current engine timing, not the default period and not inferred from buffer size.
        let diagnostics = json!({
            "initPath": if reason.is_none() { "IAudioClient3" } else { "wasapi-shared-event-fallback" },
            "requestedPeriodFrames": REQUESTED_PERIOD,
            "requestedPeriodMs": REQUESTED_PERIOD as f64 * 1000. / format.get_samplespersec() as f64,
            "fallbackReason": reason,
            "engine": crate::windows_audio::engine(id),
            "wasapiBufferFrames": buffer,
            "wasapiBufferMs": buffer as f64 * 1000. / format.get_samplespersec() as f64,
            "requestedBufferMs": if matches!(stream, Self::Legacy { .. }) { Some(20) } else { None },
        });
        Ok((stream, diagnostics))
    }

    fn low_latency(id: &str, direction: Direction, format: &WaveFormat) -> Res<(Self, u32)> {
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let base: IAudioClient = enumerator
                .GetDevice(&HSTRING::from(id))?
                .Activate(CLSCTX_ALL, None)?;
            let client: IAudioClient3 = base.cast()?;
            let (mut default, mut fundamental, mut min, mut max) = (0, 0, 0, 0);
            client
                .GetSharedModeEnginePeriod(
                    format.as_waveformatex_ref(),
                    &mut default,
                    &mut fundamental,
                    &mut min,
                    &mut max,
                )
                .map_err(|e| format!("GetSharedModeEnginePeriod: {e}"))?;
            validate_period(REQUESTED_PERIOD, fundamental, min, max)?;
            // Unlike the old render-only probe this must include LOOPBACK for render endpoints.
            // Some endpoints/Windows versions reject that combination; retain the legacy path.
            client
                .InitializeSharedAudioStream(
                    stream_flags(direction),
                    REQUESTED_PERIOD,
                    format.as_waveformatex_ref(),
                    None,
                )
                .map_err(|e| {
                    format!(
                        "InitializeSharedAudioStream (flags=0x{:x}): {e}",
                        stream_flags(direction)
                    )
                })?;
            let event = Event(CreateEventW(None, false, false, None)?);
            client
                .SetEventHandle(event.0)
                .map_err(|e| format!("SetEventHandle: {e}"))?;
            let capture = client
                .GetService::<IAudioCaptureClient>()
                .map_err(|e| format!("GetService(IAudioCaptureClient): {e}"))?;
            let buffer = client.GetBufferSize()?;
            Ok((
                Self::LowLatency {
                    capture,
                    client,
                    event,
                    align: format.get_blockalign() as usize,
                },
                buffer,
            ))
        }
    }

    pub(super) fn start_stream(&self) -> Res<()> {
        match self {
            Self::LowLatency { client, .. } => unsafe { client.Start()? },
            Self::Legacy { client, .. } => client.start_stream()?,
        }
        Ok(())
    }
    pub(super) fn stop_stream(&self) -> Res<()> {
        match self {
            Self::LowLatency { client, .. } => unsafe { client.Stop()? },
            Self::Legacy { client, .. } => client.stop_stream()?,
        }
        Ok(())
    }
    pub(super) fn get_next_packet_size(&self) -> Res<Option<u32>> {
        Ok(match self {
            Self::LowLatency { capture, .. } => Some(unsafe { capture.GetNextPacketSize()? }),
            Self::Legacy { capture, .. } => capture.get_next_packet_size()?,
        })
    }
    pub(super) fn wait(&self, timeout_ms: u32) -> bool {
        match self {
            Self::LowLatency { event, .. } => unsafe {
                WaitForSingleObject(event.0, timeout_ms) == WAIT_OBJECT_0
            },
            Self::Legacy { event, .. } => event.wait_for_event(timeout_ms).is_ok(),
        }
    }
    pub(super) fn read_from_device_to_deque(&self, pcm: &mut VecDeque<u8>) -> Res<BufferInfo> {
        match self {
            Self::Legacy { capture, .. } => Ok(capture.read_from_device_to_deque(pcm)?),
            Self::LowLatency { capture, align, .. } => unsafe {
                let mut data = std::ptr::null_mut();
                let (mut frames, mut flags, mut index, mut timestamp) = (0, 0, 0, 0);
                capture.GetBuffer(
                    &mut data,
                    &mut frames,
                    &mut flags,
                    Some(&mut index),
                    Some(&mut timestamp),
                )?;
                let info = BufferInfo::new(flags, index, timestamp);
                if frames > 0 {
                    let bytes = frames as usize * align;
                    append_pcm(pcm, data, bytes, info.flags.silent);
                    capture.ReleaseBuffer(frames)?;
                }
                Ok(info)
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_copy_preserves_interleaved_pcm_and_handles_null_silence() {
        // Two 7.1 PCM16 frames; no conversion, channel reordering or intermediate buffer.
        let samples: Vec<u8> = (0..32).collect();
        let mut pcm = VecDeque::with_capacity(64);
        unsafe {
            append_pcm(&mut pcm, samples.as_ptr(), samples.len(), false);
            append_pcm(&mut pcm, std::ptr::null(), 16, true);
            append_pcm(&mut pcm, std::ptr::null(), 0, false);
        }
        assert_eq!(pcm.len(), 48);
        assert!(pcm.iter().take(32).copied().eq(samples));
        assert!(pcm.iter().skip(32).all(|byte| *byte == 0));
        assert_eq!(pcm.capacity(), 64);
    }

    #[test]
    fn exact_period_is_required_without_rounding() {
        assert!(validate_period(240, 16, 128, 480).is_ok());
        for (fundamental, min, max) in [(128, 128, 512), (16, 256, 512), (16, 16, 128), (0, 0, 480)]
        {
            assert!(validate_period(240, fundamental, min, max).is_err());
        }
    }
    #[test]
    fn render_capture_requires_loopback() {
        assert_eq!(
            stream_flags(Direction::Render),
            AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_LOOPBACK
        );
        assert_eq!(
            stream_flags(Direction::Capture),
            AUDCLNT_STREAMFLAGS_EVENTCALLBACK
        );
    }
    #[test]
    fn fallback_preserves_failure_reason_and_primary_skips_fallback() {
        let (value, reason) = with_fallback(|| Ok(5), || panic!("fallback must not run")).unwrap();
        assert_eq!((value, reason), (5, None));
        let (value, reason) =
            with_fallback(|| Err("unsupported format".into()), || Ok(20)).unwrap();
        assert_eq!(value, 20);
        assert_eq!(reason.as_deref(), Some("unsupported format"));
        let error = with_fallback::<()>(|| Err("primary".into()), || Err("legacy".into()))
            .unwrap_err()
            .to_string();
        assert!(error.contains("primary") && error.contains("legacy"));
    }
}
