use super::*;
use serde::Deserialize;

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub enabled: bool,
    pub source_id: Option<String>,
    pub output_id: Option<String>,
    pub speakers: Vec<u32>,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub status: String,
    pub error: Option<String>,
    pub sync_error_ms: Option<f64>,
    pub latency_ms: Option<f64>,
    pub output_ms: Option<f64>,
    pub unavailable: Vec<u32>,
}
#[derive(Clone, Serialize)]
pub struct Endpoint {
    pub id: String,
    pub name: String,
}
pub fn endpoints() -> Res<Vec<Endpoint>> {
    let collection = DeviceEnumerator::new()?.get_device_collection(&Direction::Render)?;
    let mut result = Vec::new();
    for index in 0..collection.get_nbr_devices()? {
        let device = collection.get_device_at_index(index)?;
        result.push(Endpoint {
            id: device.get_id()?,
            name: device.get_friendlyname()?,
        });
    }
    Ok(result)
}
pub fn config_path() -> std::path::PathBuf {
    assignments_path().with_file_name("local-output.json")
}

/// Matrix rows are physical PCM output channels; columns are source PCM channels.
/// Exact speaker positions are retained. Missing positions fold into mono/stereo,
/// with per-output headroom normalization rather than clipping summed PCM.
fn matrix(source: u32, output: u32, selected: &[u32]) -> [[f64; 8]; 8] {
    let mut result = [[0.; 8]; 8];
    for &speaker in selected {
        let Some(input) = channel_index(source, speaker).filter(|i| *i < 8) else {
            continue;
        };
        if let Some(out) = channel_index(output, speaker).filter(|i| *i < 8) {
            result[out][input] = 1.;
            continue;
        }
        if output.count_ones() == 1 {
            result[0][input] = 1.;
            continue;
        }
        let (left, right) = match speaker {
            1 | 16 | 64 | 512 => (1., 0.),
            2 | 32 | 128 | 1024 => (0., 1.),
            _ => (
                std::f64::consts::FRAC_1_SQRT_2,
                std::f64::consts::FRAC_1_SQRT_2,
            ),
        };
        if let Some(i) = channel_index(output, 1) {
            result[i][input] = left;
        }
        if let Some(i) = channel_index(output, 2) {
            result[i][input] = right;
        }
    }
    for row in &mut result {
        let sum = row.iter().sum::<f64>().max(1.);
        for gain in row {
            *gain /= sum;
        }
    }
    result
}
fn sample(block: &AudioBlock, cursor: f64, row: &[f64; 8]) -> f64 {
    let a = (cursor as usize).min(FRAMES - 1);
    let b = (a + 1).min(FRAMES - 1);
    let fraction = cursor - a as f64;
    (0..block.channels)
        .map(|c| {
            let x = block.source[a * block.channels + c] as f64;
            let y = block.source[b * block.channels + c] as f64;
            (x + (y - x) * fraction) * row[c]
        })
        .sum()
}
pub(super) fn worker(stop: &AtomicBool, hub: &Hub) {
    while !stop.load(Ordering::Relaxed) {
        let config = hub.local_config.lock().unwrap().clone();
        if !config.enabled {
            *hub.local_state.lock().unwrap() = State {
                status: "disabled".into(),
                ..Default::default()
            };
            thread::sleep(Duration::from_millis(100));
            continue;
        }
        let result = run(stop, hub, &config);
        hub.subscribers.lock().unwrap().remove("__local_output");
        hub.state.lock().unwrap().budgets.remove("__local_output");
        if let Err(e) = result {
            *hub.local_state.lock().unwrap() = State {
                status: "error".into(),
                error: Some(e.to_string()),
                ..Default::default()
            };
            thread::sleep(Duration::from_millis(500));
        }
    }
}
fn run(stop: &AtomicBool, hub: &Hub, config: &Config) -> Res<()> {
    wasapi::initialize_mta().ok()?;
    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            wasapi::deinitialize();
        }
    }
    let _com = Com;
    let source = config
        .source_id
        .as_ref()
        .ok_or("Select a separate capture source first")?;
    let output = config.output_id.as_ref().ok_or("Select the PC output")?;
    if source == output {
        return Err(
            "Capture source and PC output must be different to prevent audio feedback".into(),
        );
    }
    let device = DeviceEnumerator::new()?.get_device(output)?;
    let (layout, rate) = crate::layout::read_endpoint(output)?;
    if let Some(error) = &layout.error {
        return Err(error.clone().into());
    }
    let channels = layout.channel_count;
    let align = channels * 2;
    let mut audio = device.get_iaudioclient()?;
    audio.initialize_client(
        &WaveFormat::new(
            16,
            16,
            &SampleType::Int,
            48000,
            channels,
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
    let clock = audio.get_audioclock()?;
    let frequency = clock.get_frequency()?;
    if frequency == 0 {
        return Err("Windows output clock has zero frequency".into());
    }
    let capacity = audio.get_buffer_size()? as usize;
    let mut bytes = vec![0u8; capacity * align];
    render.write_to_device(capacity, &bytes, None)?;
    let mut written = capacity as u64;
    let (tx, rx) = mpsc::sync_channel(4);
    hub.subscribers
        .lock()
        .unwrap()
        .insert("__local_output".into(), tx);
    audio.start_stream()?;
    let result = (|| -> Res<()> {
        let host_clock = Clock::new();
        let mut queue = VecDeque::with_capacity(64);
        let mut current: Option<Arc<AudioBlock>> = None;
        let mut cursor = 0.;
        let mut gain: f64 = 0.;
        let mut previous = [0.; 8];
        let mut weights = [[0.; 8]; 8];
        let mut refreshed = Instant::now();
        let mut metric_at = Instant::now();
        let mut phase = 0.;
        let mut latency = None;
        let mut unavailable = Vec::new();
        while !stop.load(Ordering::Relaxed) {
            if *hub.local_config.lock().unwrap() != *config {
                break;
            }
            if refreshed.elapsed() > Duration::from_millis(500) {
                if hub.output_endpoint.lock().unwrap().as_ref()
                    != Some(&(output.clone(), layout.clone(), rate))
                {
                    break;
                }
                refreshed = Instant::now();
            }
            while let Ok(block) = rx.try_recv() {
                if queue.len() == 64 {
                    queue.pop_front();
                }
                queue.push_back(block);
            }
            let free = audio.get_available_space_in_frames()? as usize;
            if free == 0 {
                let _ = event.wait_for_event(5);
                continue;
            }
            let now = host_clock.now();
            let (position, qpc) = clock.get_position()?;
            if qpc == 0 {
                let _ = event.wait_for_event(5);
                continue;
            }
            let first = qpc as i128 * 100 + written as i128 * 1_000_000_000 / 48000
                - position as i128 * 1_000_000_000 / frequency as i128;
            let lead = ((first - now as i128) as f64 / 1e6).max(0.);
            bytes[..free * align].fill(0);
            for n in 0..free {
                let presentation = first + n as i128 * 1_000_000_000 / 48000;
                if current.is_none() {
                    current = queue.pop_front();
                    if let Some(b) = &current {
                        weights = matrix(b.mask, layout.channel_mask, &config.speakers);
                        unavailable = config
                            .speakers
                            .iter()
                            .filter(|s| channel_index(b.mask, **s).is_none())
                            .copied()
                            .collect();
                    }
                }
                let mut step = 0.;
                if let Some(b) = &current {
                    let error = (presentation - b.play_ns as i128) as f64 - cursor * 1e9 / 48000.;
                    phase = error / 1e6;
                    if error > 20e6 {
                        gain = (gain - 1. / 240.).max(0.);
                        if gain == 0. {
                            current = None;
                            cursor = 0.;
                        }
                    } else if error < -1e6 && gain == 0. {
                        // Keep this source frame until its absolute host deadline.
                    } else {
                        gain = (gain + 1. / 240.).min(1.);
                        for out in 0..channels {
                            previous[out] = sample(b, cursor, &weights[out]);
                        }
                        latency = Some((presentation - b.read_ns as i128) as f64 / 1e6);
                        step = 1. + (error / 1e9).clamp(-0.005, 0.005);
                    }
                } else {
                    gain = (gain - 1. / 240.).max(0.);
                }
                for out in 0..channels {
                    let value = (previous[out] * gain).round().clamp(-32768., 32767.) as i16;
                    bytes[n * align + out * 2..n * align + out * 2 + 2]
                        .copy_from_slice(&value.to_le_bytes());
                }
                cursor += step;
                if cursor >= FRAMES as f64 {
                    cursor -= FRAMES as f64;
                    current = None;
                }
            }
            render.write_to_device(free, &bytes[..free * align], None)?;
            written += free as u64;
            if metric_at.elapsed() > Duration::from_millis(250) {
                *hub.local_state.lock().unwrap() = State {
                    status: if gain == 0. {
                        "buffering"
                    } else if phase.abs() < 3. {
                        "synced"
                    } else {
                        "aligning"
                    }
                    .into(),
                    error: None,
                    sync_error_ms: Some(phase),
                    latency_ms: latency,
                    output_ms: Some(lead),
                    unavailable: unavailable.clone(),
                };
                hub.state.lock().unwrap().budgets.insert(
                    "__local_output".into(),
                    ((lead + 30.).clamp(40., 500.) * 1e6) as i64,
                );
                metric_at = Instant::now();
            }
            let _ = event.wait_for_event(5);
        }
        Ok(())
    })();
    let stopped = audio.stop_stream();
    result?;
    stopped?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn front_and_center_mix_excludes_rear_and_has_headroom() {
        let m = matrix(0x3f, 3, &[1, 2, 4]);
        assert!(m[0][0] > 0. && m[0][2] > 0. && m[0][1] == 0.);
        assert!(m[1][1] > 0. && m[1][2] > 0. && m[1][0] == 0.);
        for row in &m {
            assert!(row.iter().sum::<f64>() <= 1.000001);
            assert_eq!(row[4], 0.);
            assert_eq!(row[5], 0.);
        }
        let m = matrix(0x63f, 0x63f, &[1, 2, 4]);
        assert_eq!(m[0][0], 1.);
        assert_eq!(m[1][1], 1.);
        assert_eq!(m[2][2], 1.);
        assert_eq!(matrix(3, 3, &[512]), [[0.; 8]; 8]);
    }
}
