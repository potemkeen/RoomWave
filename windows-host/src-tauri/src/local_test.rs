use crate::layout::{channel_index, read_endpoint, test_sample};
use std::time::{Duration, Instant};
use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

fn fill(data: &mut [u8], channels: usize, selected: usize, first: u64, speaker: u32) {
    data.fill(0);
    for (n, frame) in data.chunks_exact_mut(channels * 2).enumerate() {
        frame[selected * 2..selected * 2 + 2]
            .copy_from_slice(&test_sample(first + n as u64, 48000, speaker).to_le_bytes());
    }
}

pub fn play(speaker: u32) -> Result<(), String> {
    fn run(speaker: u32) -> Result<(), Box<dyn std::error::Error>> {
        wasapi::initialize_mta().ok()?;
        struct Com;
        impl Drop for Com {
            fn drop(&mut self) {
                wasapi::deinitialize();
            }
        }
        let _com = Com;
        let device = DeviceEnumerator::new()?.get_default_device(&Direction::Render)?;
        let (layout, _) = read_endpoint(&device.get_id()?)?;
        if let Some(error) = layout.error {
            return Err(error.into());
        }
        let index = channel_index(layout.channel_mask, speaker)
            .ok_or("Channel unavailable on the Windows output")?;
        let mut audio = device.get_iaudioclient()?;
        audio.initialize_client(
            &WaveFormat::new(
                16,
                16,
                &SampleType::Int,
                48000,
                layout.channel_count,
                Some(layout.channel_mask),
            ),
            &Direction::Render,
            &StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: 200_000,
            },
        )?;
        let event = audio.set_get_eventhandle()?;
        let render = audio.get_audiorenderclient()?;
        let capacity = audio.get_buffer_size()? as usize;
        let align = layout.channel_count * 2;
        let mut bytes = vec![0u8; capacity * align];
        let mut frame = 0u64;
        let total = 28800u64;
        let first = capacity.min(total as usize);
        fill(
            &mut bytes[..first * align],
            layout.channel_count,
            index,
            0,
            speaker,
        );
        render.write_to_device(first, &bytes[..first * align], None)?;
        frame += first as u64;
        audio.start_stream()?;
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let started = Instant::now();
            loop {
                if started.elapsed() > Duration::from_secs(3) {
                    return Err("Windows speaker test timed out".into());
                }
                event.wait_for_event(500)?;
                let free = audio.get_available_space_in_frames()? as usize;
                if frame == total && free == capacity {
                    break;
                }
                let count = free.min((total - frame) as usize);
                if count == 0 {
                    continue;
                }
                fill(
                    &mut bytes[..count * align],
                    layout.channel_count,
                    index,
                    frame,
                    speaker,
                );
                render.write_to_device(count, &bytes[..count * align], None)?;
                frame += count as u64;
            }
            Ok(())
        })();
        let stopped = audio.stop_stream();
        result?;
        stopped?;
        Ok(())
    }
    run(speaker).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_test_uses_only_the_mask_selected_output() {
        for (channels, mask, speaker) in [(2, 3, 2), (6, 0x60f, 512), (8, 0x63f, 512)] {
            let index = channel_index(mask, speaker).unwrap();
            let mut data = vec![255; 240 * channels * 2];
            fill(&mut data, channels, index, 1000, speaker);
            let mut heard = false;
            for frame in data.chunks_exact(channels * 2) {
                for c in 0..channels {
                    let sample = i16::from_le_bytes([frame[2 * c], frame[2 * c + 1]]);
                    if c == index {
                        heard |= sample != 0;
                    } else {
                        assert_eq!(sample, 0);
                    }
                }
            }
            assert!(heard);
        }
    }
}
