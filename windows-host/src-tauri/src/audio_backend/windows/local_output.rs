use super::*;
use std::collections::VecDeque;

use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

pub use crate::audio_types::{Config, Endpoint, State};
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
/// A linked peak limiter protects the actual mix, including resampler overshoot.
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
    result
}
const TAPS: usize = 32;
const PHASES: usize = 1024;
// Causal windowed-sinc interpolation: 16 source frames (0.333 ms) of delay.
// Coefficients are built once, never in the render callback/loop.
fn interpolation_table() -> Vec<[f64; TAPS]> {
    (0..PHASES)
        .map(|phase| {
            let fraction = phase as f64 / PHASES as f64;
            let mut row = [0.; TAPS];
            for (tap, value) in row.iter_mut().enumerate() {
                let x = tap as f64 - 15. - fraction;
                let sinc = if x.abs() < 1e-12 {
                    1.
                } else {
                    (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
                };
                let window = 0.42
                    + 0.5 * (std::f64::consts::PI * x / 16.).cos()
                    + 0.08 * (std::f64::consts::PI * x / 8.).cos();
                *value = sinc * window;
            }
            let sum: f64 = row.iter().sum();
            for v in &mut row {
                *v /= sum;
            }
            row
        })
        .collect()
}
fn sample(
    block: &AudioBlock,
    history: Option<&AudioBlock>,
    cursor: f64,
    row: &[f64; 8],
    table: &[[f64; TAPS]],
) -> f64 {
    let base = cursor.floor() as isize - 31;
    let phase = ((cursor.fract() * PHASES as f64) as usize).min(PHASES - 1);
    (0..block.channels)
        .map(|c| {
            if row[c] == 0. {
                return 0.;
            }
            let mut value = 0.;
            for (tap, weight) in table[phase].iter().enumerate() {
                let index = base + tap as isize;
                let x = if index >= 0 {
                    block.source[index as usize * block.channels + c] as f64
                } else {
                    history
                        .filter(|h| {
                            h.mask == block.mask
                                && h.channels == block.channels
                                && h.frame + FRAMES as u64 == block.frame
                        })
                        .map(|h| {
                            h.source[(FRAMES as isize + index) as usize * h.channels + c] as f64
                        })
                        .unwrap_or(0.)
                };
                value += x * weight;
            }
            value * row[c]
        })
        .sum()
}
fn local_budget_ns(lead: f64, remote: bool) -> i64 {
    ((lead + if remote { 30. } else { 5. }).clamp(10., 500.) * 1e6) as i64
}
fn limit_frame(frame: &mut [f64], level: &mut f64) {
    let peak = frame.iter().fold(0f64, |p, x| p.max(x.abs()));
    let needed = if peak > 32767. { 32767. / peak } else { 1. };
    // Linked instantaneous attack prevents clipping, 100 ms release avoids pumping
    // at individual waveform zero crossings. No additional lookahead buffer.
    *level = needed.min(*level + (1. - *level) / 4800.);
    for x in frame {
        *x *= *level;
    }
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
    let engine = crate::windows_audio::engine(output);
    let mmcss = crate::windows_audio::Mmcss::enter();
    let mut bytes = vec![0u8; capacity * align];
    render.write_to_device(capacity, &bytes, None)?;
    let mut written = capacity as u64;
    // Capacity is not a playout prebuffer: deadlines below still govern output.
    // Allow capture bursts while the render worker fills a Windows buffer.
    let (tx, rx) = mpsc::sync_channel(16);
    hub.subscribers
        .lock()
        .unwrap()
        .insert("__local_output".into(), tx);
    audio.start_stream()?;
    let result = (|| -> Res<()> {
        let host_clock = Clock::new();
        let mut queue = VecDeque::with_capacity(64);
        let mut current: Option<Arc<AudioBlock>> = None;
        let mut history: Option<Arc<AudioBlock>> = None;
        let interpolation = interpolation_table();
        let mut limiter = 1.;
        let mut primed = false;
        let mut local_rate = 1f64;
        let mut cursor = 0.;
        let mut gain: f64 = 0.;
        let mut previous = [0.; 8];
        let mut weights = [[0.; 8]; 8];
        let mut refreshed = Instant::now();
        let mut metric_at = Instant::now();
        let mut phase = 0.;
        let mut latency = None;
        let mut unavailable = Vec::new();
        let mut matrix_mask = None;
        let mut max_queue_frames = 0usize;
        let mut evicted_frames = 0u64;
        let mut late_blocks = 0u64;
        let mut empty_source_frames = 0u64;
        let mut empty_padding_observations = 0u64;
        let mut render_work = crate::transport_stats::Histogram::default();
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
                    evicted_frames += FRAMES as u64;
                    queue.pop_front();
                }
                queue.push_back(block);
            }
            let free = audio.get_available_space_in_frames()? as usize;
            if free == 0 {
                let _ = event.wait_for_event(5);
                continue;
            }
            let work_started = Instant::now();
            if free == capacity {
                empty_padding_observations += 1;
            }
            max_queue_frames = max_queue_frames.max(queue.len() * FRAMES);
            let now = host_clock.now();
            let (position, qpc) = clock.get_position()?;
            if qpc == 0 {
                let _ = event.wait_for_event(5);
                continue;
            }
            let first = qpc as i128 * 100 + written as i128 * 1_000_000_000 / 48000
                - position as i128 * 1_000_000_000 / frequency as i128;
            let lead = ((first - now as i128) as f64 / 1e6).max(0.);
            let (remote, budget) = {
                let mut state = hub.state.lock().unwrap();
                let remote = state.budgets.keys().any(|id| id != "__local_output");
                let budget = local_budget_ns(lead, remote);
                if !primed {
                    // No audible local stream yet: avoid a long ramp down from the
                    // network startup budget. New source blocks use this deadline.
                    if !remote {
                        hub.delay_ns.store(budget, Ordering::Relaxed);
                    }
                    state.budgets.insert("__local_output".into(), budget);
                    primed = true;
                }
                (remote, budget)
            };
            // A sole PC sink has no peer deadline to wait for. A gentle FIFO
            // servo compensates capture/output clock drift without a fixed wait.
            let available = queue.len() as f64 * FRAMES as f64
                + if current.is_some() {
                    FRAMES as f64 - cursor
                } else {
                    0.
                };
            let desired_rate =
                1. + ((available - free as f64 - FRAMES as f64) / 48000.).clamp(-0.001, 0.001);
            local_rate += (desired_rate - local_rate) * 0.02;
            bytes[..free * align].fill(0);
            for n in 0..free {
                let presentation = first + n as i128 * 1_000_000_000 / 48000;
                if current.is_none() {
                    current = queue.pop_front();
                    if let Some(b) = current.as_ref().filter(|b| matrix_mask != Some(b.mask)) {
                        matrix_mask = Some(b.mask);
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
                    let error = if remote {
                        (presentation - b.play_ns as i128) as f64 - cursor * 1e9 / 48000.
                    } else {
                        0.
                    };
                    phase = error / 1e6;
                    if error > 20e6 {
                        gain = (gain - 1. / 240.).max(0.);
                        if gain == 0. {
                            late_blocks += 1;
                            current = None;
                            history = None;
                            cursor = 0.;
                        }
                    } else if error < -1e6 && gain == 0. {
                        // Keep this source frame until its absolute host deadline.
                    } else {
                        gain = (gain + 1. / 240.).min(1.);
                        for out in 0..channels {
                            previous[out] = sample(
                                b,
                                history.as_deref(),
                                cursor,
                                &weights[out],
                                &interpolation,
                            );
                        }
                        latency = Some((presentation - b.read_ns as i128) as f64 / 1e6);
                        step = if remote {
                            1. + (error / 1e9).clamp(-0.005, 0.005)
                        } else {
                            local_rate
                        };
                    }
                } else {
                    empty_source_frames += 1;
                    gain = (gain - 1. / 240.).max(0.);
                }
                let mut frame = previous;
                for x in &mut frame[..channels] {
                    *x *= gain;
                }
                limit_frame(&mut frame[..channels], &mut limiter);
                for out in 0..channels {
                    let value = frame[out].round().clamp(-32768., 32767.) as i16;
                    bytes[n * align + out * 2..n * align + out * 2 + 2]
                        .copy_from_slice(&value.to_le_bytes());
                }
                cursor += step;
                if cursor >= FRAMES as f64 {
                    cursor -= FRAMES as f64;
                    history = current.take();
                }
            }
            render.write_to_device(free, &bytes[..free * align], None)?;
            written += free as u64;
            render_work.add_ns(work_started.elapsed().as_nanos() as u64);
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
                    sync_error_ms: if remote { Some(phase) } else { None },
                    latency_ms: latency,
                    output_ms: Some(lead),
                    buffer_margin_ms: Some(if remote { 30. } else { 0. }),
                    limiter_gain: Some(limiter),
                    unavailable: unavailable.clone(),
                    stages: json!({"engine":engine,"wasapiBufferFrames":capacity,
                        "wasapiBufferMs":capacity as f64/48.,"requestedBufferMs":20,
                        "streamSampleRate":48000,"pcmBlockFrames":FRAMES,
                        "queueFrames":queue.len()*FRAMES + current.as_ref().map(|_| FRAMES.saturating_sub(cursor as usize)).unwrap_or(0),
                        "oldestQueuedBlockAgeMs":current.as_ref().or_else(|| queue.front()).map(|b| (host_clock.now() as u64).saturating_sub(b.published_ns) as f64/1e6),
                        "maxQueuedBlockFrames":max_queue_frames,"queueEvictedFrames":evicted_frames,
                        "lateBlocks":late_blocks,"emptySourceFrames":empty_source_frames,
                        "emptyPaddingObservations":empty_padding_observations,
                        "renderWork":render_work.snapshot(),"mmcssError":mmcss.error}),
                };
                hub.state
                    .lock()
                    .unwrap()
                    .budgets
                    .insert("__local_output".into(), budget);
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
    fn front_and_center_mix_excludes_rear() {
        let m = matrix(0x3f, 3, &[1, 2, 4]);
        assert!(m[0][0] > 0. && m[0][2] > 0. && m[0][1] == 0.);
        assert!(m[1][1] > 0. && m[1][2] > 0. && m[1][0] == 0.);
        for row in &m {
            assert_eq!(row[4], 0.);
            assert_eq!(row[5], 0.);
        }
        let m = matrix(0x63f, 0x63f, &[1, 2, 4]);
        assert_eq!(m[0][0], 1.);
        assert_eq!(m[1][1], 1.);
        assert_eq!(m[2][2], 1.);
        assert_eq!(matrix(3, 3, &[512]), [[0.; 8]; 8]);
    }
    #[test]
    fn silent_center_does_not_attenuate_stereo_and_overload_is_linked() {
        let m = matrix(0x63f, 3, &[1, 2, 4]);
        assert_eq!(m[0][0], 1.);
        assert_eq!(m[1][1], 1.);
        let mut level = 1.;
        let mut stereo = [12000., -6000.];
        limit_frame(&mut stereo, &mut level);
        assert_eq!(stereo, [12000., -6000.]);
        let mut overload = [50000., 25000.];
        limit_frame(&mut overload, &mut level);
        assert!(overload[0] <= 32767.);
        assert_eq!(overload[0] / overload[1], 2.);
    }
    #[test]
    fn local_only_budget_does_not_include_network_margin() {
        assert_eq!(local_budget_ns(32., false), 37_000_000);
        assert_eq!(local_budget_ns(32., true), 62_000_000);
    }
    #[test]
    fn interpolator_preserves_high_frequencies_and_dc() {
        let table = interpolation_table();
        for row in &table {
            assert!((row.iter().sum::<f64>() - 1.).abs() < 1e-10);
            let omega = 2. * std::f64::consts::PI * 18000. / 48000.;
            let re: f64 = row
                .iter()
                .enumerate()
                .map(|(n, w)| w * (omega * n as f64).cos())
                .sum();
            let im: f64 = row
                .iter()
                .enumerate()
                .map(|(n, w)| w * (omega * n as f64).sin())
                .sum();
            assert!((re.hypot(im) - 1.).abs() < 0.02);
        }
    }
}
