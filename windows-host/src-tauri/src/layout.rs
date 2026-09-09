use serde::Serialize;
/// Read the original structure: wasapi's legacy WAVEFORMATEX conversion synthesizes
/// a mask from channel count, which must not be used for multichannel routing.
pub fn read_endpoint(id: &str) -> windows::core::Result<(Layout, u32)> {
    use windows::{
        core::HSTRING,
        Win32::{
            Media::Audio::{
                IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEXTENSIBLE,
            },
            System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL},
        },
    };
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDevice(&HSTRING::from(id))?;
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
        let ptr = client.GetMixFormat()?;
        let format = ptr.read_unaligned();
        let mask = if format.wFormatTag == 0xfffe && format.cbSize >= 22 {
            ptr.cast::<WAVEFORMATEXTENSIBLE>()
                .read_unaligned()
                .dwChannelMask
        } else {
            // Legacy PCM mono/stereo have defined conventional positions. A legacy
            // multichannel format has no speaker mapping; never guess its mask.
            match format.nChannels {
                1 => 4,
                2 => 3,
                _ => 0,
            }
        };
        CoTaskMemFree(Some(ptr.cast()));
        Ok((
            Layout::new(format.nChannels as usize, mask),
            format.nSamplesPerSec,
        ))
    }
}

#[derive(Clone, Default, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub name: String,
    pub channel_count: usize,
    pub channel_mask: u32,
    pub channels: Vec<Speaker>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Speaker {
    pub mask: u32,
    pub code: String,
    pub name: String,
    pub index: usize,
}
pub fn channel_index(mask: u32, speaker: u32) -> Option<usize> {
    (speaker.is_power_of_two() && mask & speaker != 0)
        .then(|| (mask & (speaker - 1)).count_ones() as usize)
}
impl Layout {
    pub fn new(count: usize, mask: u32) -> Self {
        let name = match (count, mask) {
            (1, 4) => "Mono",
            (2, 3) => "Stereo",
            (4, 0x33) => "Quad",
            (6, 0x3f) => "5.1 (Back)",
            (6, 0x60f) => "5.1 (Side)",
            (8, 0x63f) => "7.1",
            (8, 0xff) => "7.1 (Wide)",
            _ => "Custom",
        }
        .to_owned();
        let error = if count == 0 || count > 8 || mask.count_ones() as usize != count {
            Some(format!(
                "Channel mask 0x{mask:X} does not describe {count} channels; routing unavailable"
            ))
        } else {
            None
        };
        let channels = if error.is_none() {
            (0..32)
                .filter_map(|bit| {
                    let speaker = 1u32 << bit;
                    let index = channel_index(mask, speaker)?;
                    let (code, label) = match speaker {
                        1 => ("FL", "Front Left"),
                        2 => ("FR", "Front Right"),
                        4 => ("FC", "Center"),
                        8 => ("LFE", "LFE"),
                        16 => ("BL", "Back Left"),
                        32 => ("BR", "Back Right"),
                        64 => ("FLC", "Front Left of Center"),
                        128 => ("FRC", "Front Right of Center"),
                        256 => ("BC", "Back Center"),
                        512 => ("SL", "Surround Left"),
                        1024 => ("SR", "Surround Right"),
                        _ => ("?", "Other speaker"),
                    };
                    Some(Speaker {
                        mask: speaker,
                        code: code.into(),
                        name: label.into(),
                        index,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };
        Self {
            name,
            channel_count: count,
            channel_mask: mask,
            channels,
            error,
        }
    }
}
pub fn test_sample(frame: u64, rate: u32, speaker: u32) -> i16 {
    let duration = rate as u64 * 3 / 5;
    if frame >= duration {
        return 0;
    }
    let time = frame as f64 / rate as f64;
    let remaining = (duration - 1 - frame) as f64 / rate as f64;
    // A soft, decaying chord rather than a sustained calibration beep.
    // Raised-cosine edges keep both the amplitude and its slope smooth.
    let ramp = |position: f64| 0.5 - 0.5 * (std::f64::consts::PI * position.clamp(0.0, 1.0)).cos();
    let gain = ramp(time / 0.045) * ramp(remaining / 0.160) * (-3.5 * time).exp();
    let sine = |frequency: f64| (std::f64::consts::TAU * frequency * time).sin();
    let tone = if speaker == 8 {
        sine(82.4)
    } else {
        0.65 * sine(330.0) + 0.22 * sine(412.5) + 0.13 * sine(495.0)
    };
    (5000.0 * gain * tone) as i16
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn masks_define_order_not_channel_count() {
        for (n, m) in [
            (1, 4),
            (2, 3),
            (4, 0x33),
            (6, 0x3f),
            (6, 0x60f),
            (8, 0x63f),
            (8, 0xff),
        ] {
            let l = Layout::new(n, m);
            assert!(l.error.is_none());
            assert_eq!(l.channels.len(), n);
            for (i, s) in l.channels.iter().enumerate() {
                assert_eq!(s.index, i);
            }
        }
        assert_eq!(channel_index(0x60f, 512), Some(4));
        assert_eq!(channel_index(0x63f, 512), Some(6));
        assert_eq!(channel_index(0x3f, 512), None);
        assert!(Layout::new(6, 0).channels.is_empty());
        assert!(Layout::new(6, 3).error.is_some());
    }
    #[test]
    fn test_tone_fades_and_stops() {
        assert_eq!(test_sample(0, 48000, 1), 0);
        assert_eq!(test_sample(28799, 48000, 1), 0);
        assert_eq!(test_sample(28800, 48000, 1), 0);
        assert!((1000..2000).any(|f| test_sample(f, 48000, 1) != 0));
    }
}
