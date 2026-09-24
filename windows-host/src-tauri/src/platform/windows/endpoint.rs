use crate::layout::Layout;
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
