use crate::layout::{channel_index, test_sample, Layout};
use crate::timing::Clock;

use serde_json::json;

use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

use super::local_output;

use super::super::{
    next_playout_budget, AudioBlock, Hub, Res, FRAMES, PCM_BYTES, PERIOD_NS, PLAYOUT_NS,
};

pub(in crate::audio) fn capture_audio(stop: &AtomicBool, hub: &Hub) -> Res<()> {
    thread::scope(|scope| {
        scope.spawn(|| local_output::worker(stop, hub));
        // Query device metadata on its own worker; COM/driver queries never delay UDP PCM production.
        scope.spawn(|| {
            if wasapi::initialize_mta().is_err() {
                return;
            }
            while !stop.load(Ordering::Relaxed) {
                let signature = endpoint_signature(hub).ok();
                let output_id = hub.local_config.lock().unwrap().output_id.clone();
                let output_signature = output_id.and_then(|id| {
                    crate::layout::read_endpoint(&id)
                        .ok()
                        .map(|(layout, rate)| (id, layout, rate))
                });
                *hub.output_endpoint.lock().unwrap() = output_signature;
                if let Ok(devices) = local_output::endpoints() {
                    *hub.windows_outputs.lock().unwrap() = devices;
                }
                if let Ok(mut current) = hub.endpoint.lock() {
                    *current = signature;
                }
                thread::sleep(Duration::from_millis(500));
            }
            wasapi::deinitialize();
        });
        let mut timeline = (0u64, 0i64);
        while !stop.load(Ordering::Relaxed) {
            hub.capture_ready.store(false, Ordering::Release);
            if let Err(e) = capture_endpoint(stop, hub, &mut timeline) {
                if let Ok(mut s) = hub.state.lock() {
                    s.error = Some(e.to_string());
                    s.layout = Layout::default();
                }
                thread::sleep(Duration::from_millis(500));
            }
        }
    });
    Ok(())
}
fn capture_device(hub: &Hub) -> Res<wasapi::Device> {
    let source = hub
        .local_config
        .lock()
        .map_err(|e| e.to_string())?
        .source_id
        .clone();
    let enumerator = DeviceEnumerator::new()?;
    Ok(if let Some(id) = source {
        enumerator.get_device(&id)?
    } else {
        enumerator.get_default_device(&Direction::Render)?
    })
}
fn endpoint_signature(hub: &Hub) -> Res<(String, Layout, u32)> {
    let device = capture_device(hub)?;
    let id = device.get_id()?;
    let (layout, rate) = crate::layout::read_endpoint(&id)?;
    Ok((id, layout, rate))
}
fn capture_endpoint(stop: &AtomicBool, hub: &Hub, timeline: &mut (u64, i64)) -> Res<()> {
    wasapi::initialize_mta().ok()?;
    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            wasapi::deinitialize();
        }
    }
    let _com = Com;
    let output = capture_device(hub)?;
    let id = output.get_id()?;
    let (layout, rate) = crate::layout::read_endpoint(&id)?;
    if let Some(e) = &layout.error {
        return Err(e.clone().into());
    }
    let capture_id = crate::default_output::capture_endpoint(&id, &layout)?;
    let capture_device = DeviceEnumerator::new()?.get_device(&capture_id)?;
    let mut audio = capture_device.get_iaudioclient()?;
    let signature = (id, layout.clone(), rate);
    {
        let mut s = hub.state.lock().map_err(|e| e.to_string())?;
        s.layout = layout.clone();
        s.error = layout.error.clone();
        s.output_name = Some(output.get_friendlyname()?);
    }
    if let Some(e) = &layout.error {
        return Err(e.clone().into());
    }
    let channels = layout.channel_count;
    let align = channels * 2;
    let block_bytes = FRAMES * align;
    let wave_format = WaveFormat::new(
        16,
        16,
        &SampleType::Int,
        48000,
        channels,
        Some(layout.channel_mask),
    );

    let period_probe_240 =
        crate::windows_audio::shared_period_probe(&capture_id, &wave_format, 240);

    let period_probe_128 =
        crate::windows_audio::shared_period_probe(&capture_id, &wave_format, 128);

    audio.initialize_client(
        &wave_format,
        &Direction::Capture,
        &StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: 200_000,
        },
    )?;
    let event = audio.set_get_eventhandle()?;
    let capture = audio.get_audiocaptureclient()?;
    let engine = crate::windows_audio::engine(&capture_id);
    let buffer_frames = audio.get_buffer_size()?;
    let mmcss = crate::windows_audio::Mmcss::enter();
    hub.capture_ready.store(true, Ordering::Release);
    hub.state.lock().map_err(|e| e.to_string())?.output_name = Some(output.get_friendlyname()?);
    let clock = Clock::new();
    let mut frame = timeline.0;
    let mut next_send = timeline.1;
    let mut started = false;
    let result = (|| -> Res<()> {
        let mut pcm = VecDeque::with_capacity(4800 * align);
        let mut times = VecDeque::with_capacity(4800);
        let mut reads = VecDeque::with_capacity(4800);
        let mut endpoint_checked = Instant::now();
        let mut peak = 0f32;
        let mut reported = Instant::now();
        let mut budget_at = Instant::now();
        let mut read_work = crate::transport_stats::Histogram::default();
        let mut pack_work = crate::transport_stats::Histogram::default();
        let mut event_wait = crate::transport_stats::Histogram::default();
        let mut read_to_publish = crate::transport_stats::Histogram::default();
        let mut capture_packet_age = crate::transport_stats::Histogram::default();
        let mut event_timeouts = 0u64;
        let mut max_read_frames = 0usize;
        let mut max_queue_frames = 0usize;
        let mut discontinuities = 0u64;
        let mut timestamp_errors = 0u64;
        while !stop.load(Ordering::Relaxed) {
            if endpoint_checked.elapsed() >= Duration::from_millis(500) {
                if hub.endpoint.lock().map_err(|e| e.to_string())?.as_ref() != Some(&signature) {
                    break;
                }
                endpoint_checked = Instant::now();
            }
            if hub
                .subscribers
                .lock()
                .map_err(|e| e.to_string())?
                .is_empty()
            {
                if started {
                    audio.stop_stream()?;
                    started = false;
                    pcm.clear();
                    times.clear();
                    reads.clear();
                    hub.state.lock().map_err(|e| e.to_string())?.peak = 0.0;
                }
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            if !started {
                audio.start_stream()?;
                started = true;
                if next_send == 0 {
                    next_send = clock.now();
                }
            }
            // Read one engine packet, then publish all complete network blocks.
            // Another ready engine packet is drained below without sleeping.
            if capture.get_next_packet_size()?.unwrap_or(0) > 0 {
                let read_started = Instant::now();
                let previous = pcm.len();
                let info = capture.read_from_device_to_deque(&mut pcm)?;
                discontinuities += u64::from(info.flags.data_discontinuity);
                timestamp_errors += u64::from(info.flags.timestamp_error);
                max_queue_frames = max_queue_frames.max(pcm.len() / align);
                let read_ns = clock.now() as u64;
                if !info.flags.timestamp_error && info.timestamp != 0 {
                    if let Some(age) = read_ns.checked_sub(info.timestamp.saturating_mul(100)) {
                        capture_packet_age.add_ns(age);
                    }
                }
                max_read_frames = max_read_frames.max((pcm.len() - previous) / align);
                for n in 0..(pcm.len() - previous) / align {
                    reads.push_back(read_ns);
                    times.push_back(if info.flags.timestamp_error || info.timestamp == 0 {
                        0
                    } else {
                        info.timestamp * 100 + n as u64 * 1_000_000_000 / 48000
                    });
                }
                read_work.add_ns(read_started.elapsed().as_nanos() as u64);
            }
            if pcm.len() > 4800 * align {
                let n = pcm.len() - 2400 * align;
                hub.capture_trimmed_frames
                    .fetch_add((n / align) as u64, Ordering::Relaxed);
                pcm.drain(..n);
                times.drain(..n / align);
                reads.drain(..n / align);
            }
            let now = clock.now();
            if now > next_send + 100_000_000 {
                let skipped = (now - next_send) / PERIOD_NS;
                hub.timeline_skipped_frames
                    .fetch_add(skipped as u64 * FRAMES as u64, Ordering::Relaxed);
                frame += skipped as u64 * FRAMES as u64;
                next_send += skipped * PERIOD_NS;
                pcm.clear();
                times.clear();
                reads.clear();
            }
            let packing_started = Instant::now();
            while pcm.len() >= block_bytes || clock.now() >= next_send + 20_000_000 {
                // Never move the timeline without advancing its frame index. In
                // particular, an idle endpoint must catch up with silence; resetting
                // next_send alone strands receivers ahead of every future packet.
                let mut block = AudioBlock {
                    frame,
                    capture_ns: 0,
                    read_ns: clock.now() as u64,
                    published_ns: 0,
                    play_ns: (next_send + hub.delay()) as u64,
                    pcm: [0; PCM_BYTES],
                    source: [0; FRAMES * 8],
                    mode: 0,
                    mask: layout.channel_mask,
                    channels,
                    test_channel: None,
                };
                if pcm.len() >= block_bytes {
                    block.capture_ns = times.front().copied().unwrap_or(0);
                    block.read_ns = reads.front().copied().unwrap_or(block.read_ns);
                    times.drain(..FRAMES);
                    reads.drain(..FRAMES);
                    for sample in &mut block.source[..FRAMES * channels] {
                        *sample = i16::from_le_bytes([
                            pcm.pop_front().unwrap(),
                            pcm.pop_front().unwrap(),
                        ]);
                    }
                }
                {
                    let mut state = hub.state.lock().map_err(|e| e.to_string())?;
                    if hub.local_test.load(Ordering::Acquire) {
                        block.source.fill(0);
                    }
                    if let Some((speaker, start)) = state.test {
                        let start = if start == 0 { frame } else { start };
                        if frame - start >= 28800 || channel_index(block.mask, speaker).is_none() {
                            state.test = None;
                        } else {
                            block.source.fill(0);
                            block.test_channel = Some(speaker);
                            let index = channel_index(block.mask, speaker).unwrap();
                            for n in 0..FRAMES {
                                block.source[n * channels + index] =
                                    test_sample(frame - start + n as u64, 48000, speaker);
                            }
                            state.test = Some((speaker, start));
                        }
                    }
                }
                // Compatibility mode: front stereo (mono duplicated), unchanged for a stereo endpoint.
                for n in 0..FRAMES {
                    for (side, speaker) in [1, 2].into_iter().enumerate() {
                        let index = channel_index(block.mask, speaker).or({
                            if channels == 1 {
                                Some(0)
                            } else {
                                None
                            }
                        });
                        let value = index.map(|i| block.source[n * channels + i]).unwrap_or(0);
                        block.pcm[n * 4 + side * 2..n * 4 + side * 2 + 2]
                            .copy_from_slice(&value.to_le_bytes());
                    }
                }
                for value in block.pcm.chunks_exact(2) {
                    peak =
                        peak.max((i16::from_le_bytes([value[0], value[1]]) as f32 / 32768.0).abs());
                }
                block.mode = hub.mode.load(Ordering::Acquire);
                block.published_ns = clock.now() as u64;
                read_to_publish.add_ns(block.published_ns.saturating_sub(block.read_ns));
                hub.publish(Arc::new(block));
                frame += FRAMES as u64;
                next_send += PERIOD_NS;
            }
            pack_work.add_ns(packing_started.elapsed().as_nanos() as u64);
            if reported.elapsed() >= Duration::from_millis(500) {
                *hub.capture_stages.lock().map_err(|e| e.to_string())? = json!({
                    "engine": engine,
                    "sharedPeriodProbe240": period_probe_240,
                    "sharedPeriodProbe128": period_probe_128,
                    "wasapiBufferFrames": buffer_frames,
                    "wasapiBufferMs":buffer_frames as f64 / 48.,"requestedBufferMs":20,
                    "streamSampleRate":48000,"pcmBlockFrames":FRAMES,"pcmBlockBytes":block_bytes,
                    "queueFrames":pcm.len()/align,"maxQueueFrames":max_queue_frames,
                    "discontinuities":discontinuities,"timestampErrors":timestamp_errors,
                    "mmcssError":mmcss.error,
                    "captureEndpointId":capture_id,
                    "captureBackend":if capture_device.get_direction() == Direction::Capture {"vb-cable-recording"} else {"wasapi-loopback"},
                    "readWork":read_work.snapshot(),"packetizationWork":pack_work.snapshot(),
                    "eventWait":event_wait.snapshot(),"eventTimeouts":event_timeouts,
                    "maxReadFrames":max_read_frames,"readToPublish":read_to_publish.snapshot(),
                    "capturePacketAge":capture_packet_age.snapshot(),
                    "sourceEndpointId":signature.0,"managedVirtualSource":hub.managed_source.is_some()});
                hub.state.lock().map_err(|e| e.to_string())?.peak = peak;
                peak = 0.0;
                reported = Instant::now();
            }
            if budget_at.elapsed() >= Duration::from_millis(100) {
                let requested = hub
                    .state
                    .lock()
                    .map_err(|e| e.to_string())?
                    .budgets
                    .values()
                    .copied()
                    .max()
                    .unwrap_or(PLAYOUT_NS);
                let current = hub.delay();
                // A single host budget keeps all receivers on one timeline. Limit rate
                // Rising budget: 0.3%, below the renderers' 0.5% correction limit.
                // Reductions remain at 0.1%; no deadline jumps on an audible stream.
                hub.delay_ns
                    .store(next_playout_budget(current, requested), Ordering::Relaxed);
                budget_at = Instant::now();
            }
            // Capture wakes on the engine event and publishes complete 5 ms packets immediately.
            // The timeout only maintains silence/stop responsiveness when the endpoint is idle.
            if capture.get_next_packet_size()?.unwrap_or(0) == 0 {
                let waiting = Instant::now();
                if event.wait_for_event(10).is_err() {
                    event_timeouts += 1;
                }
                event_wait.add_ns(waiting.elapsed().as_nanos() as u64);
            }
        }
        Ok(())
    })();
    *timeline = (frame, next_send);
    if started {
        audio.stop_stream()?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a Windows audio endpoint playing system sound"]
    fn capture_timestamp_probe() -> Res<()> {
        wasapi::initialize_mta().ok()?;
        let device = DeviceEnumerator::new()?.get_default_device(&Direction::Render)?;
        let mut audio = device.get_iaudioclient()?;
        audio.initialize_client(
            &WaveFormat::new(16, 16, &SampleType::Int, 48000, 2, None),
            &Direction::Capture,
            &StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: 200_000,
            },
        )?;
        let event = audio.set_get_eventhandle()?;
        let capture = audio.get_audiocaptureclient()?;

        let clock = Clock::new();
        let mut bytes = vec![0; audio.get_buffer_size()? as usize * 4];
        audio.start_stream()?;
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(2) {
            let _ = event.wait_for_event(50);
            while capture.get_next_packet_size()?.unwrap_or(0) > 0 {
                let (frames, info) = capture.read_from_device(&mut bytes)?;
                let now = clock.now();
                println!(
                    "capture_probe frames={frames} index={} qpc_age_ms={:.3} timestamp_error={}",
                    info.index,
                    (now as i128 - info.timestamp as i128 * 100) as f64 / 1e6,
                    info.flags.timestamp_error
                );
            }
        }
        audio.stop_stream()?;
        Ok(())
    }
}
