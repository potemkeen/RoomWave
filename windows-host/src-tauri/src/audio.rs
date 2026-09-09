use crate::discovery::Device;
use crate::layout::{channel_index, test_sample, Layout};
use crate::timing::{Clock, ClockSync};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpStream, UdpSocket},
    sync::{
        atomic::{AtomicBool, AtomicI64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};
#[path = "local_output.rs"]
mod local_output;
pub use local_output::Config as LocalOutputConfig;

const FRAMES: usize = 240;
const PCM_BYTES: usize = FRAMES * 4;
const PERIOD_NS: i64 = 5_000_000;
const PLAYOUT_NS: i64 = 80_000_000;
type Res<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiverState {
    pub device_id: String,
    pub device_name: String,
    pub status: String,
    pub packets_sent: u64,
    pub receiver_packets: u64,
    pub receiver_lost: u64,
    pub latency_ms: Option<f64>,
    pub rtt_ms: Option<f64>,
    pub sync_error_ms: Option<f64>,
    pub sync_status: String,
    pub stages: Value,
    pub error: Option<String>,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioState {
    pub output_name: Option<String>,
    pub peak: f32,
    pub target_delay_ms: u32,
    pub receivers: Vec<ReceiverState>,
    pub layout: Layout,
    pub assignments: BTreeMap<String, u32>,
    pub capture_error: Option<String>,
    pub testing: Option<u32>,
    pub local_config: LocalOutputConfig,
    pub local_output: local_output::State,
    pub windows_outputs: Vec<local_output::Endpoint>,
}
#[derive(Default)]
struct HubState {
    output_name: Option<String>,
    peak: f32,
    error: Option<String>,
    budgets: BTreeMap<String, i64>,
    layout: Layout,
    test: Option<(u32, u64)>,
}
#[derive(Default)]
struct Hub {
    local_config: Mutex<LocalOutputConfig>,
    local_state: Mutex<local_output::State>,
    windows_outputs: Mutex<Vec<local_output::Endpoint>>,
    local_test: AtomicBool,
    state: Mutex<HubState>,
    subscribers: Mutex<BTreeMap<String, SyncSender<Arc<AudioBlock>>>>,
    delay_ns: AtomicI64,
    assignments: Mutex<BTreeMap<String, u32>>,
    endpoint: Mutex<Option<(String, Layout, u32)>>,
    output_endpoint: Mutex<Option<(String, Layout, u32)>>,
}
impl Hub {
    fn delay(&self) -> i64 {
        self.delay_ns.load(Ordering::Relaxed).max(PLAYOUT_NS / 2)
    }
    fn publish(&self, block: Arc<AudioBlock>) {
        if let Ok(mut subscribers) = self.subscribers.lock() {
            subscribers.retain(|_, tx| match tx.try_send(block.clone()) {
                Ok(()) | Err(TrySendError::Full(_)) => true,
                Err(TrySendError::Disconnected(_)) => false,
            });
        }
    }
}
struct Job {
    stop: Arc<AtomicBool>,
    worker: JoinHandle<()>,
}
impl Job {
    fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.worker.join();
    }
}
struct PeerJob {
    job: Job,
    state: Arc<Mutex<ReceiverState>>,
}
#[derive(Default)]
struct Jobs {
    capture: Option<Job>,
    peers: BTreeMap<String, PeerJob>,
}
pub struct AudioSession {
    hub: Arc<Hub>,
    jobs: Mutex<Jobs>,
}
impl AudioSession {
    pub fn set_local_output(&self, config: LocalOutputConfig) -> Result<(), String> {
        if config.enabled
            && (config.source_id.is_none()
                || config.output_id.is_none()
                || config.source_id == config.output_id)
        {
            return Err("Выберите разные устройства: источник захвата и выход ПК. Для разделения 5.1 используйте отдельный многоканальный, например виртуальный, источник.".into());
        }
        if config.speakers.len() > 8 || config.speakers.iter().any(|s| !s.is_power_of_two()) {
            return Err("Invalid speaker selection".into());
        }
        if config.enabled && config.speakers.is_empty() {
            return Err("Выберите хотя бы один канал для ПК".into());
        }
        let path = local_output::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(
            path,
            serde_json::to_vec(&config).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        *self.hub.local_config.lock().map_err(|e| e.to_string())? = config;
        Ok(())
    }
    pub fn routed_local_test(&self) -> bool {
        self.hub.local_config.lock().unwrap().enabled
    }
    pub fn local_test_guard(&self, speaker: u32) -> Result<LocalTestGuard, String> {
        let mut state = self.hub.state.lock().map_err(|e| e.to_string())?;
        if !state.layout.channels.iter().any(|c| c.mask == speaker) {
            return Err("Channel unavailable".into());
        }
        if state.test.is_some() || self.hub.local_test.swap(true, Ordering::AcqRel) {
            return Err("A speaker test is already running".into());
        }
        state.test = Some((speaker, 0));
        Ok(LocalTestGuard(self.hub.clone()))
    }
    pub fn new() -> Self {
        let hub = Arc::new(Hub {
            local_config: Mutex::new(
                std::fs::read(local_output::config_path())
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
                    .unwrap_or_default(),
            ),
            delay_ns: AtomicI64::new(PLAYOUT_NS),
            assignments: Mutex::new(
                std::fs::read(assignments_path())
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
                    .unwrap_or_default(),
            ),
            ..Default::default()
        });
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let capture_hub = hub.clone();
        let worker = thread::spawn(move || {
            let _ = capture_audio(&worker_stop, &capture_hub);
        });
        Self {
            hub,
            jobs: Mutex::new(Jobs {
                capture: Some(Job { stop, worker }),
                ..Default::default()
            }),
        }
    }
    pub fn set_channel(&self, id: String, speaker: Option<u32>) -> Result<(), String> {
        if let Some(s) = speaker {
            if !self
                .hub
                .state
                .lock()
                .map_err(|e| e.to_string())?
                .layout
                .channels
                .iter()
                .any(|c| c.mask == s)
            {
                return Err(
                    "Channel unavailable: choose a channel from the current Windows layout".into(),
                );
            }
        }
        let mut assignments = self.hub.assignments.lock().map_err(|e| e.to_string())?;
        let mut next = assignments.clone();
        if let Some(s) = speaker {
            next.insert(id, s);
        } else {
            next.remove(&id);
        }
        let path = assignments_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, serde_json::to_vec(&next).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        *assignments = next;
        Ok(())
    }
    pub fn snapshot(&self) -> Result<AudioState, String> {
        let jobs = self.jobs.lock().map_err(|e| e.to_string())?;
        let hub = self.hub.state.lock().map_err(|e| e.to_string())?;
        let receivers = jobs
            .peers
            .values()
            .map(|p| p.state.lock().map(|s| s.clone()).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(AudioState {
            output_name: hub.output_name.clone(),
            peak: hub.peak,
            target_delay_ms: (self.hub.delay() / 1_000_000) as u32,
            receivers,
            layout: hub.layout.clone(),
            assignments: self
                .hub
                .assignments
                .lock()
                .map_err(|e| e.to_string())?
                .clone(),
            capture_error: hub.error.clone(),
            testing: hub.test.map(|t| t.0),
            local_config: self
                .hub
                .local_config
                .lock()
                .map_err(|e| e.to_string())?
                .clone(),
            local_output: self
                .hub
                .local_state
                .lock()
                .map_err(|e| e.to_string())?
                .clone(),
            windows_outputs: self
                .hub
                .windows_outputs
                .lock()
                .map_err(|e| e.to_string())?
                .clone(),
        })
    }
    pub fn connect(&self, device: Device) -> Result<(), String> {
        let mut jobs = self.jobs.lock().map_err(|e| e.to_string())?;
        if jobs
            .peers
            .get(&device.device_id)
            .is_some_and(|p| !p.job.worker.is_finished())
        {
            return Ok(());
        }
        if let Some(old) = jobs.peers.remove(&device.device_id) {
            old.job.stop();
        }
        if jobs
            .capture
            .as_ref()
            .is_some_and(|j| j.worker.is_finished())
        {
            if let Some(old) = jobs.capture.take() {
                old.stop();
            }
        }
        if jobs.capture.is_none() {
            *self.hub.state.lock().map_err(|e| e.to_string())? = HubState::default();
            let hub = self.hub.clone();
            let stop = Arc::new(AtomicBool::new(false));
            let worker_stop = stop.clone();
            let worker = thread::Builder::new()
                .name("roomwave-capture".into())
                .spawn(move || {
                    if let Err(e) = capture_audio(&worker_stop, &hub) {
                        log::error!("capture: {e}");
                        if let Ok(mut s) = hub.state.lock() {
                            s.error = Some(e.to_string());
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
            jobs.capture = Some(Job { stop, worker });
        }
        let state = Arc::new(Mutex::new(ReceiverState {
            device_id: device.device_id.clone(),
            device_name: device.device_name.clone(),
            status: "connecting".into(),
            sync_status: "warming".into(),
            ..Default::default()
        }));
        let (tx, rx) = mpsc::sync_channel(4); // At most 20 ms of pending audio per peer.
        self.hub
            .subscribers
            .lock()
            .map_err(|e| e.to_string())?
            .insert(device.device_id.clone(), tx);
        let id = device.device_id.clone();
        let worker_id = id.clone();
        let hub = self.hub.clone();
        let peer_state = state.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = match thread::Builder::new()
            .name(format!("roomwave-peer-{id}"))
            .spawn(move || {
                let result = stream_peer(&device, &worker_stop, &hub, &peer_state, rx);
                if let Ok(mut subscribers) = hub.subscribers.lock() {
                    subscribers.remove(&worker_id);
                }
                if let Ok(mut s) = hub.state.lock() {
                    s.budgets.remove(&worker_id);
                }
                if let Ok(mut s) = peer_state.lock() {
                    s.status = "disconnected".into();
                    s.sync_status = "disconnected".into();
                    s.latency_ms = None;
                    s.rtt_ms = None;
                    s.sync_error_ms = None;
                    if !worker_stop.load(Ordering::Relaxed) {
                        if let Err(e) = result {
                            log::error!("receiver {worker_id}: {e}");
                            s.error = Some(e.to_string());
                        }
                    }
                }
            }) {
            Ok(worker) => worker,
            Err(e) => {
                self.hub
                    .subscribers
                    .lock()
                    .map_err(|e| e.to_string())?
                    .remove(&id);
                return Err(e.to_string());
            }
        };
        jobs.peers.insert(
            id,
            PeerJob {
                job: Job { stop, worker },
                state,
            },
        );
        Ok(())
    }
    pub fn disconnect(&self, device_id: Option<String>) -> Result<(), String> {
        let mut jobs = self.jobs.lock().map_err(|e| e.to_string())?;
        match device_id {
            Some(id) => {
                if let Some(peer) = jobs.peers.remove(&id) {
                    peer.job.stop();
                }
            }
            None => {
                for (_, peer) in std::mem::take(&mut jobs.peers) {
                    peer.job.stop();
                }
            }
        }
        if jobs.peers.values().all(|p| p.job.worker.is_finished()) {
            self.hub.state.lock().map_err(|e| e.to_string())?.peak = 0.0;
        }
        Ok(())
    }
}
impl Drop for AudioSession {
    fn drop(&mut self) {
        let _ = self.disconnect(None);
        if let Ok(mut jobs) = self.jobs.lock() {
            if let Some(capture) = jobs.capture.take() {
                capture.stop();
            }
        }
    }
}

struct AudioBlock {
    frame: u64,
    capture_ns: u64,
    read_ns: u64,
    play_ns: u64,
    pcm: [u8; PCM_BYTES],
    source: [i16; FRAMES * 8],
    mask: u32,
    channels: usize,
    test_channel: Option<u32>,
}
fn assignments_path() -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("RoomWave")
        .join("channels.json")
}
#[cfg(test)]
fn packet(session: u64, block: &AudioBlock) -> Vec<u8> {
    let mut result = Vec::with_capacity(48 + PCM_BYTES);
    result.extend_from_slice(b"RWAV");
    result.extend_from_slice(&[4, 1, 2, 0]);
    result.extend_from_slice(&session.to_be_bytes());
    result.extend_from_slice(&((block.frame / FRAMES as u64) as u32).to_be_bytes());
    result.extend_from_slice(&block.frame.to_be_bytes());
    result.extend_from_slice(&(FRAMES as u16).to_be_bytes());
    result.extend_from_slice(&[0, 0]);
    result.extend_from_slice(&block.capture_ns.to_be_bytes());
    result.extend_from_slice(&block.play_ns.to_be_bytes());
    result.extend_from_slice(&block.pcm);
    result
}
fn extended_packet(session: u64, block: &AudioBlock, send_ns: u64) -> [u8; 1024] {
    let mut result = [0u8; 1024];
    result[..8].copy_from_slice(b"RWAV\x04\x01\x02\x01");
    result[8..16].copy_from_slice(&session.to_be_bytes());
    result[16..20].copy_from_slice(&((block.frame / FRAMES as u64) as u32).to_be_bytes());
    result[20..28].copy_from_slice(&block.frame.to_be_bytes());
    result[28..30].copy_from_slice(&(FRAMES as u16).to_be_bytes());
    result[32..40].copy_from_slice(&block.capture_ns.to_be_bytes());
    result[40..48].copy_from_slice(&block.play_ns.to_be_bytes());
    result[48..56].copy_from_slice(&send_ns.to_be_bytes());
    result[56..64].copy_from_slice(&block.read_ns.to_be_bytes());
    result[64..].copy_from_slice(&block.pcm);
    result
}
struct RoutedPacket {
    frame: u64,
    play_ns: u64,
    bytes: [u8; 1024],
    len: usize,
}
fn routed_packet(
    session: u64,
    block: &AudioBlock,
    send: u64,
    speaker: Option<u32>,
) -> RoutedPacket {
    let mut bytes = extended_packet(session, block, send);
    let len = if let Some(s) = speaker {
        bytes[6] = 1;
        let index = channel_index(block.mask, s).filter(|i| *i < block.channels);
        for n in 0..FRAMES {
            let value = index
                .map(|i| block.source[n * block.channels + i])
                .unwrap_or(0);
            bytes[64 + n * 2..66 + n * 2].copy_from_slice(&value.to_le_bytes());
        }
        544
    } else {
        // A speaker test addresses explicitly assigned devices only.
        if block.test_channel.is_some() {
            bytes[64..].fill(0);
        }
        1024
    };
    RoutedPacket {
        frame: block.frame,
        play_ns: block.play_ns,
        bytes,
        len,
    }
}
fn capture_audio(stop: &AtomicBool, hub: &Hub) -> Res<()> {
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
    let mut audio = output.get_iaudioclient()?;
    let id = output.get_id()?;
    let (layout, rate) = crate::layout::read_endpoint(&id)?;
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
    audio.initialize_client(
        &WaveFormat::new(
            16,
            16,
            &SampleType::Int,
            48000,
            channels,
            Some(layout.channel_mask),
        ),
        &Direction::Capture,
        &StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: 200_000,
        },
    )?;
    let event = audio.set_get_eventhandle()?;
    let capture = audio.get_audiocaptureclient()?;
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
            while capture.get_next_packet_size()?.unwrap_or(0) > 0 {
                let previous = pcm.len();
                let info = capture.read_from_device_to_deque(&mut pcm)?;
                let read_ns = clock.now() as u64;
                for n in 0..(pcm.len() - previous) / align {
                    reads.push_back(read_ns);
                    times.push_back(if info.flags.timestamp_error || info.timestamp == 0 {
                        0
                    } else {
                        info.timestamp * 100 + n as u64 * 1_000_000_000 / 48000
                    });
                }
            }
            if pcm.len() > 4800 * align {
                let n = pcm.len() - 2400 * align;
                pcm.drain(..n);
                times.drain(..n / align);
                reads.drain(..n / align);
            }
            let now = clock.now();
            if now > next_send + 100_000_000 {
                let skipped = (now - next_send) / PERIOD_NS;
                frame += skipped as u64 * FRAMES as u64;
                next_send += skipped * PERIOD_NS;
                pcm.clear();
                times.clear();
                reads.clear();
            }
            while pcm.len() >= block_bytes || clock.now() >= next_send + 20_000_000 {
                // Never move the timeline without advancing its frame index. In
                // particular, an idle endpoint must catch up with silence; resetting
                // next_send alone strands receivers ahead of every future packet.
                let mut block = AudioBlock {
                    frame,
                    capture_ns: 0,
                    read_ns: clock.now() as u64,
                    play_ns: (next_send + hub.delay()) as u64,
                    pcm: [0; PCM_BYTES],
                    source: [0; FRAMES * 8],
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
                hub.publish(Arc::new(block));
                frame += FRAMES as u64;
                next_send += PERIOD_NS;
            }
            if reported.elapsed() >= Duration::from_millis(500) {
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
                // changes to 0.1%, within the receivers' continuous drift correction.
                hub.delay_ns.store(
                    current + (requested - current).clamp(-100_000, 100_000),
                    Ordering::Relaxed,
                );
                budget_at = Instant::now();
            }
            // Capture wakes on the engine event and publishes complete 5 ms packets immediately.
            // The timeout only maintains silence/stop responsiveness when the endpoint is idle.
            let _ = event.wait_for_event(10);
        }
        Ok(())
    })();
    *timeline = (frame, next_send);
    if started {
        audio.stop_stream()?;
    }
    result
}
pub struct LocalTestGuard(Arc<Hub>);
impl Drop for LocalTestGuard {
    fn drop(&mut self) {
        // Keep the loopback tail out of phone streams without stopping their clocks.
        thread::sleep(Duration::from_millis(150));
        if let Ok(mut state) = self.0.state.lock() {
            state.test = None;
        }
        self.0.local_test.store(false, Ordering::Release);
    }
}

fn stream_peer(
    device: &Device,
    stop: &AtomicBool,
    hub: &Hub,
    shared: &Mutex<ReceiverState>,
    rx: Receiver<Arc<AudioBlock>>,
) -> Res<()> {
    let address: SocketAddr = if device.ip_address.contains(':') {
        format!("[{}]:47801", device.ip_address)
    } else {
        format!("{}:47801", device.ip_address)
    }
    .parse()?;
    let mut control = TcpStream::connect_timeout(&address, Duration::from_secs(3))?;
    control.set_nodelay(true)?;
    control.set_read_timeout(Some(Duration::from_secs(3)))?;
    control.set_write_timeout(Some(Duration::from_secs(2)))?;
    let session = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos() as u64 & i64::MAX as u64;
    let start = json!({"type":"start", "protocolVersion":4, "deviceId":device.device_id, "sessionId":session.to_string(), "sampleRate":48000, "channels":2, "framesPerPacket":FRAMES, "format":"s16le", "targetDelayMs":PLAYOUT_NS / 1_000_000});
    writeln!(control, "{start}")?;
    let mut response = String::new();
    BufReader::new(control.try_clone()?)
        .take(4096)
        .read_line(&mut response)?;
    let response: Value = serde_json::from_str(&response)?;
    if response["type"] != "ready"
        || response["sessionId"]
            .as_str()
            .and_then(|v| v.parse::<u64>().ok())
            != Some(session)
    {
        return Err(format!("Receiver rejected connection: {response}").into());
    }
    let extended = response["extendedTiming"].as_bool() == Some(true);
    let mono_supported = response["monoPcm"].as_bool() == Some(true);
    if !extended {
        return Err("Update this phone to the AAudio RoomWave receiver before connecting".into());
    }
    hub.state
        .lock()
        .map_err(|e| e.to_string())?
        .budgets
        .insert(device.device_id.clone(), PLAYOUT_NS);
    let mut destination = address;
    destination.set_port(device.port);
    let udp = UdpSocket::bind(if address.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    })?;
    udp.connect(destination)?;
    udp.set_nonblocking(true)?;
    control.set_nonblocking(true)?;
    shared.lock().map_err(|e| e.to_string())?.status = "streaming".into();
    log::info!("receiver connected {} at {address}", device.device_name);
    let clock = Clock::new();
    let mut sync = ClockSync::default();
    let mut pending_ping: Option<i64> = None;
    let mut last_pong = Instant::now();
    let mut last_ping = Instant::now() - Duration::from_secs(1);
    let connected = Instant::now();
    let mut incoming = Vec::new();
    let mut outgoing: VecDeque<u8> = VecDeque::new();
    let mut sent = 0u64;
    let mut recent: VecDeque<RoutedPacket> = VecDeque::with_capacity(64);
    let result = (|| -> Res<()> {
        while !stop.load(Ordering::Relaxed) {
            // Endpoint changes may briefly pause capture; keep peer clocks/connections alive.
            // Drain stale backlog after handshake; deadlines are shared, never shifted per receiver.
            for _ in 0..40 {
                let block = match rx.try_recv() {
                    Ok(block) => block,
                    Err(_) => break,
                };
                if block.play_ns as i64 <= clock.now() {
                    continue;
                }
                let speaker = hub
                    .assignments
                    .lock()
                    .map_err(|e| e.to_string())?
                    .get(&device.device_id)
                    .copied();
                if speaker.is_some() && !mono_supported {
                    return Err("Update Android RoomWave to receive assigned mono channels".into());
                }
                let routed = routed_packet(session, &block, clock.now() as u64, speaker);
                let sent_result = udp.send(&routed.bytes[..routed.len]);
                match sent_result {
                    Ok(_) => sent += 1,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(e.into()),
                }
                if recent.len() == 64 {
                    recent.pop_front();
                }
                recent.push_back(routed);
            }
            // Feedback arrives on the same connected UDP socket, so the peer address
            // and session are authenticated to this local stream. Bound amplification.
            let mut nack = [0u8; 64];
            for _ in 0..4 {
                match udp.recv(&mut nack) {
                    Ok(24)
                        if &nack[..8] == b"RWNA\0\0\0\x04"
                            && nack[8..16] == session.to_be_bytes() =>
                    {
                        let frame = u64::from_be_bytes(nack[16..24].try_into().unwrap());
                        let now = clock.now();
                        let guard =
                            sync.best(now).map(|s| s.rtt_ns).unwrap_or(20_000_000) + 5_000_000;
                        if let Some(block) = recent
                            .iter()
                            .find(|b| b.frame == frame && b.play_ns as i64 > now + guard)
                        {
                            let mut bytes = block.bytes;
                            bytes[48..56].copy_from_slice(&(now as u64).to_be_bytes());
                            let _ = udp.send(&bytes[..block.len]);
                        }
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => return Err(e.into()),
                }
            }
            let mut chunk = [0u8; 1024];
            // Bound work per iteration, even if the peer floods the control connection.
            for _ in 0..8 {
                match control.read(&mut chunk) {
                    Ok(0) => return Err("Receiver closed the connection".into()),
                    Ok(n) => incoming.extend_from_slice(&chunk[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => return Err(e.into()),
                }
                if incoming.len() > 4096 {
                    return Err("Control response too large".into());
                }
            }
            while let Some(end) = incoming.iter().position(|b| *b == b'\n') {
                let line: Vec<_> = incoming.drain(..=end).collect();
                let message: Value = serde_json::from_slice(&line)?;
                if message["type"] != "pong" {
                    return Err(format!("Receiver: {message}").into());
                }
                let t4 = clock.now();
                let t1 = message["hostSendNs"]
                    .as_str()
                    .and_then(|v| v.parse::<i64>().ok());
                if let Some(t1) = t1.filter(|v| Some(*v) == pending_ping) {
                    if let (Some(t2), Some(t3)) = (
                        message["phoneReceiveNs"]
                            .as_str()
                            .and_then(|v| v.parse().ok()),
                        message["phoneSendNs"].as_str().and_then(|v| v.parse().ok()),
                    ) {
                        sync.observe(t1, t2, t3, t4);
                    }
                    pending_ping = None;
                    last_pong = Instant::now();
                }
                if message["stages"]["outputBackend"] == "AAudio" {
                    let stages = &message["stages"];
                    let jitter =
                        finite_metric(stages, "recommendedJitterMs", 20.0, 50.0).unwrap_or(30.0);
                    let output = finite_metric(stages, "audioOutputMs", 0.0, 500.0).unwrap_or(40.0);
                    let rtt = finite_metric(&message, "rttMs", 0.0, 1000.0).unwrap_or(20.0);
                    let requested =
                        ((jitter + output + rtt / 2.0 + 10.0).clamp(40.0, 500.0) * 1e6) as i64;
                    hub.state
                        .lock()
                        .map_err(|e| e.to_string())?
                        .budgets
                        .insert(device.device_id.clone(), requested);
                }
                let mut s = shared.lock().map_err(|e| e.to_string())?;
                s.packets_sent = sent;
                s.receiver_packets = message["packets"].as_u64().unwrap_or(0);
                s.receiver_lost = message["lost"].as_u64().unwrap_or(0);
                s.stages = message["stages"].clone();
                s.latency_ms = finite_metric(&message, "latencyMs", 0.0, 5000.0);
                s.rtt_ms = finite_metric(&message, "rttMs", 0.0, 1000.0);
                s.sync_error_ms = finite_metric(&message, "syncErrorMs", -5000.0, 5000.0);
                s.sync_status = message["syncStatus"]
                    .as_str()
                    .unwrap_or("warming")
                    .to_string();
            }
            if last_pong.elapsed() > Duration::from_secs(5) {
                return Err("Receiver heartbeat timed out".into());
            }
            let interval = if connected.elapsed() < Duration::from_secs(3) {
                100
            } else {
                500
            };
            if outgoing.is_empty()
                && pending_ping.is_none()
                && last_ping.elapsed() >= Duration::from_millis(interval)
            {
                let now = clock.now();
                let sample = sync.best(now);
                let ping = json!({"type":"ping", "hostSendNs":now.to_string(), "clockOffsetNs":sample.map(|s| s.offset_ns.to_string()), "rttMs":sample.map(|s| s.rtt_ns as f64 / 1_000_000.0)});
                outgoing.extend(format!("{ping}\n").bytes());
                pending_ping = Some(now);
                last_ping = Instant::now();
            }
            while !outgoing.is_empty() {
                match control.write(outgoing.make_contiguous()) {
                    Ok(0) => return Err("Control socket stopped accepting data".into()),
                    Ok(n) => {
                        outgoing.drain(..n);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) => return Err(e.into()),
                }
            }
            thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    })();
    // Do not append a stop to a partial JSON line. Closing TCP also terminates the session.
    if outgoing.is_empty() {
        let _ = control.write_all(b"{\"type\":\"stop\"}\n");
    }
    result
}
fn finite_metric(message: &Value, key: &str, min: f64, max: f64) -> Option<f64> {
    message[key]
        .as_f64()
        .filter(|v| v.is_finite() && (min..=max).contains(v))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mono_routing_preserves_samples_deadlines_and_isolates_test() {
        for (channels, mask) in [(2, 3), (6, 0x3f), (6, 0x60f), (8, 0x63f)] {
            let mut block = AudioBlock {
                frame: 240,
                capture_ns: 10,
                read_ns: 20,
                play_ns: 100,
                pcm: [9; PCM_BYTES],
                source: [0; FRAMES * 8],
                mask,
                channels,
                test_channel: None,
            };
            for n in 0..FRAMES {
                for c in 0..channels {
                    block.source[n * channels + c] = (n as i16) * 8 + c as i16;
                }
            }
            for speaker in Layout::new(channels, mask).channels {
                let wire = routed_packet(1, &block, 30, Some(speaker.mask));
                assert_eq!(wire.len, 544);
                assert_eq!(wire.bytes[6], 1);
                assert_eq!(
                    u64::from_be_bytes(wire.bytes[40..48].try_into().unwrap()),
                    100
                );
                for n in 0..FRAMES {
                    assert_eq!(
                        i16::from_le_bytes(wire.bytes[64 + n * 2..66 + n * 2].try_into().unwrap()),
                        n as i16 * 8 + speaker.index as i16
                    );
                }
            }
            let unavailable = routed_packet(1, &block, 30, Some(0x80000000));
            assert!(unavailable.bytes[64..544].iter().all(|v| *v == 0));
            assert_eq!(&routed_packet(1, &block, 30, None).bytes[64..], &block.pcm);
            block.test_channel = Some(1);
            block.source.fill(0);
            for n in 0..FRAMES {
                block.source[n * channels] = test_sample(n as u64 + 1000, 48000, 1);
            }
            assert!(routed_packet(1, &block, 30, Some(1)).bytes[64..544]
                .iter()
                .any(|v| *v != 0));
            assert!(routed_packet(1, &block, 30, Some(2)).bytes[64..544]
                .iter()
                .all(|v| *v == 0));
            assert!(routed_packet(1, &block, 30, None).bytes[64..]
                .iter()
                .all(|v| *v == 0));
        }
    }
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
    #[test]
    fn wire_packet_preserves_common_timeline_after_sequence_wrap() {
        let block = AudioBlock {
            frame: (u32::MAX as u64 + 6) * 240,
            capture_ns: 1234567890123,
            read_ns: 1234567890123,
            play_ns: 1235067890123,
            pcm: [7; PCM_BYTES],
            source: [0; FRAMES * 8],
            mask: 3,
            channels: 2,
            test_channel: None,
        };
        let a = packet(123, &block);
        let b = packet(456, &block);
        assert_eq!(a.len(), 1008);
        assert_eq!(&a[..8], b"RWAV\x04\x01\x02\x00");
        assert_eq!(u32::from_be_bytes(a[16..20].try_into().unwrap()), 5);
        assert_eq!(
            u64::from_be_bytes(a[20..28].try_into().unwrap()),
            block.frame
        );
        assert_eq!(
            u64::from_be_bytes(a[40..48].try_into().unwrap()),
            block.play_ns
        );
        assert_eq!(&a[16..], &b[16..]);
        assert_ne!(&a[8..16], &b[8..16]);
    }
    #[test]
    fn stalled_receiver_cannot_block_other_receivers_and_late_join_keeps_timeline() {
        let hub = Hub::default();
        let (slow, _slow_rx) = mpsc::sync_channel(1);
        let (fast, fast_rx) = mpsc::sync_channel(1);
        hub.subscribers.lock().unwrap().insert("slow".into(), slow);
        hub.subscribers.lock().unwrap().insert("fast".into(), fast);
        for n in 0..100 {
            let block = Arc::new(AudioBlock {
                frame: n * 240,
                capture_ns: 1,
                read_ns: 1,
                play_ns: n * 5_000_000 + 500_000_000,
                pcm: [0; PCM_BYTES],
                source: [0; FRAMES * 8],
                mask: 3,
                channels: 2,
                test_channel: None,
            });
            hub.publish(block.clone());
            assert!(Arc::ptr_eq(&fast_rx.recv().unwrap(), &block));
        }
        let (late, late_rx) = mpsc::sync_channel(1);
        hub.subscribers.lock().unwrap().insert("late".into(), late);
        let block = Arc::new(AudioBlock {
            frame: 24000,
            capture_ns: 1,
            read_ns: 1,
            play_ns: 1_000_000_000,
            pcm: [0; PCM_BYTES],
            source: [0; FRAMES * 8],
            mask: 3,
            channels: 2,
            test_channel: None,
        });
        hub.publish(block.clone());
        assert!(Arc::ptr_eq(
            &late_rx.recv().unwrap(),
            &fast_rx.recv().unwrap()
        ));
        hub.subscribers.lock().unwrap().remove("slow");
        hub.publish(block);
        assert!(fast_rx.try_recv().is_ok());
    }
}
