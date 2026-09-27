//! Setup-time probes only; no COM activation or format allocation in the audio loop.
use serde_json::{json, Value};
use windows::{
    core::{w, Interface, HSTRING},
    Win32::{
        Foundation::HANDLE,
        Media::Audio::{
            IAudioClient, IAudioClient3, IMMDeviceEnumerator, MMDeviceEnumerator,
            AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
        },
        System::{
            Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL},
            Threading::{AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW},
        },
    },
};

pub fn engine(id: &str) -> Value {
    fn query(id: &str) -> windows::core::Result<Value> {
        unsafe {
            let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let client: IAudioClient = e
                .GetDevice(&HSTRING::from(id))?
                .Activate(CLSCTX_ALL, None)?;
            let ptr = client.GetMixFormat()?;
            let format = ptr.read_unaligned();
            let mut value = json!({"mixSampleRate":({format.nSamplesPerSec}),
                "mixChannels":({format.nChannels}),"mixBits":({format.wBitsPerSample})});
            match client.cast::<IAudioClient3>() {
                Ok(c) => {
                    let (mut def, mut fundamental, mut min, mut max) = (0, 0, 0, 0);
                    match c.GetSharedModeEnginePeriod(
                        ptr,
                        &mut def,
                        &mut fundamental,
                        &mut min,
                        &mut max,
                    ) {
                        Ok(()) => {
                            value["supportedPeriods"] = json!({"defaultFrames":def,
                            "fundamentalFrames":fundamental,"minimumFrames":min,"maximumFrames":max,
                            "sampleRate":({format.nSamplesPerSec})})
                        }
                        Err(e) => value["periodQueryError"] = json!(e.to_string()),
                    }
                    let mut current = std::ptr::null_mut();
                    let mut frames = 0;
                    match c.GetCurrentSharedModeEnginePeriod(&mut current, &mut frames) {
                        Ok(()) => {
                            let f = current.read_unaligned();
                            value["currentPeriodFrames"] = json!(frames);
                            value["currentPeriodMs"] =
                                json!(frames as f64 * 1000. / f.nSamplesPerSec as f64);
                            value["currentSampleRate"] = json!(({ f.nSamplesPerSec }));
                            CoTaskMemFree(Some(current.cast()));
                        }
                        Err(e) => value["currentPeriodError"] = json!(e.to_string()),
                    }
                }
                Err(e) => value["client3Error"] = json!(e.to_string()),
            }
            CoTaskMemFree(Some(ptr.cast()));
            Ok(value)
        }
    }
    query(id).unwrap_or_else(|e| json!({"error":e.to_string()}))
}

pub fn shared_period_probe(id: &str, format: &wasapi::WaveFormat, period_frames: u32) -> Value {
    fn query(
        id: &str,
        format: &wasapi::WaveFormat,
        period_frames: u32,
    ) -> windows::core::Result<Value> {
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

            let device = enumerator.GetDevice(&HSTRING::from(id))?;

            let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
            let client3: IAudioClient3 = client.cast()?;

            let (
                mut default_period,
                mut fundamental_period,
                mut minimum_period,
                mut maximum_period,
            ) = (0, 0, 0, 0);

            client3.GetSharedModeEnginePeriod(
                format.as_waveformatex_ref(),
                &mut default_period,
                &mut fundamental_period,
                &mut minimum_period,
                &mut maximum_period,
            )?;

            let initialize_result = client3.InitializeSharedAudioStream(
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                period_frames,
                format.as_waveformatex_ref(),
                None,
            );

            let mut value = json!({
                "requestedPeriodFrames": period_frames,
                "requestedPeriodMs": period_frames as f64 * 1000.0 / format.get_samplespersec() as f64,
                "defaultPeriodFrames": default_period,
                "fundamentalPeriodFrames": fundamental_period,
                "minimumPeriodFrames": minimum_period,
                "maximumPeriodFrames": maximum_period,
            });

            match initialize_result {
                Ok(()) => {
                    let initialized: IAudioClient = client3.cast()?;
                    let buffer_frames = initialized.GetBufferSize()?;

                    value["initialized"] = json!(true);
                    value["bufferFrames"] = json!(buffer_frames);
                    value["bufferMs"] =
                        json!(buffer_frames as f64 * 1000.0 / format.get_samplespersec() as f64);
                }
                Err(error) => {
                    value["initialized"] = json!(false);
                    value["initializeError"] = json!(error.to_string());
                }
            }

            Ok(value)
        }
    }

    query(id, format, period_frames).unwrap_or_else(|e| json!({"error": e.to_string()}))
}

/// Created and dropped on the worker that owns the WASAPI stream.
pub struct Mmcss {
    handle: Option<HANDLE>,
    pub error: Option<String>,
}
impl Mmcss {
    pub fn enter() -> Self {
        let mut index = 0;
        match unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut index) } {
            Ok(handle) => Self {
                handle: Some(handle),
                error: None,
            },
            Err(e) => Self {
                handle: None,
                error: Some(e.to_string()),
            },
        }
    }
}
impl Drop for Mmcss {
    fn drop(&mut self) {
        if let Some(h) = self.handle {
            let _ = unsafe { AvRevertMmThreadCharacteristics(h) };
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "Opens physical render endpoints; run manually on the target PC"]
    fn endpoint_buffers() {
        wasapi::initialize_mta().ok().unwrap();
        let id = std::env::var("ROOMWAVE_PROBE_ENDPOINT").unwrap();
        println!("engine: {}", super::engine(&id));
        let (layout, _) = crate::layout::read_endpoint(&id).unwrap();
        for duration in [200_000, 0] {
            let device = wasapi::DeviceEnumerator::new()
                .unwrap()
                .get_device(&id)
                .unwrap();
            let mut c = device.get_iaudioclient().unwrap();
            c.initialize_client(
                &wasapi::WaveFormat::new(
                    16,
                    16,
                    &wasapi::SampleType::Int,
                    48000,
                    layout.channel_count,
                    Some(layout.channel_mask),
                ),
                &wasapi::Direction::Render,
                &wasapi::StreamMode::EventsShared {
                    autoconvert: true,
                    buffer_duration_hns: duration,
                },
            )
            .unwrap();
            let _event = c.set_get_eventhandle().unwrap();
            println!(
                "requestedHns={duration} actualFrames={}",
                c.get_buffer_size().unwrap()
            );
        }
        wasapi::deinitialize();
    }
}
