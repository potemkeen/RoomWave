use crate::discovery::Device;
use crate::layout::Layout;

use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
        mpsc::{self, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
#[path = "audio_backend/mod.rs"]
mod backend;

mod peer;
mod wire;

use backend::{capture_audio, local_output};
use peer::stream_peer;

pub use local_output::Config as LocalOutputConfig;

const FRAMES: usize = 240;
const PCM_BYTES: usize = FRAMES * 4;
const PERIOD_NS: i64 = 5_000_000;
const PLAYOUT_NS: i64 = 30_000_000;
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
    pub last_report_unix_ms: Option<u64>,
    pub latency_ms: Option<f64>,
    pub rtt_ms: Option<f64>,
    pub sync_error_ms: Option<f64>,
    pub sync_status: String,
    pub stages: Value,
    pub transport: Value,
    pub low_rate_supported: bool,
    pub error: Option<String>,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioState {
    pub stream_mode: String,
    pub mode_revision: u64,
    pub mode_changed_unix_ms: u64,
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
    pub host_metrics: Value,
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
    managed_source: Option<String>,
    capture_ready: AtomicBool,
    mode: AtomicU64,
    mode_changed_unix_ms: AtomicU64,
    subscriber_drops: AtomicU64,
    subscriber_drops_by_device: Mutex<BTreeMap<String, u64>>,
    capture_trimmed_frames: AtomicU64,
    capture_stages: Mutex<Value>,
    timeline_skipped_frames: AtomicU64,
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
        self.delay_ns.load(Ordering::Relaxed).max(10_000_000)
    }
    fn admit_peer(
        &self,
        id: &str,
        tx: &SyncSender<Arc<AudioBlock>>,
        clocks_ready: bool,
    ) -> Res<bool> {
        if !clocks_ready {
            return Ok(false);
        }
        let mut subscribers = self.subscribers.lock().map_err(|e| e.to_string())?;
        if subscribers.is_empty() {
            self.delay_ns.fetch_max(PLAYOUT_NS, Ordering::Relaxed);
        }
        if !startup_budget_ready(self.delay()) {
            return Ok(false);
        }
        subscribers.insert(id.to_owned(), tx.clone());
        Ok(true)
    }
    fn publish(&self, block: Arc<AudioBlock>) {
        if let Ok(mut subscribers) = self.subscribers.lock() {
            subscribers.retain(|id, tx| match tx.try_send(block.clone()) {
                Ok(()) => true,
                Err(TrySendError::Full(_)) => {
                    self.subscriber_drops.fetch_add(1, Ordering::Relaxed);
                    if let Ok(mut counts) = self.subscriber_drops_by_device.lock() {
                        *counts.entry(id.clone()).or_default() += 1;
                    }
                    true
                }
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
        if let Some(source) = &self.hub.managed_source {
            if config.source_id.as_ref() != Some(source)
                || config.output_id.as_ref() == Some(source)
            {
                return Err("RoomWave Virtual Speakers используется только как источник; выберите отдельный выход ПК.".into());
            }
        }
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
    pub fn new(virtual_audio: &crate::default_output::Status) -> Self {
        let saved = std::fs::read(local_output::config_path())
            .ok()
            .and_then(|b| serde_json::from_slice::<LocalOutputConfig>(&b).ok());
        let first_run = saved.is_none();
        let mut config = saved.unwrap_or_default();
        if let Some(source) = &virtual_audio.endpoint_id {
            config.source_id = Some(source.clone());
            if config.output_id.is_none() || config.output_id.as_ref() == Some(source) {
                config.output_id = virtual_audio.previous_output_id.clone();
            }
            if first_run {
                config.enabled = config.output_id.is_some();
                config.speakers = vec![1, 2, 4];
            }
        }
        let hub = Arc::new(Hub {
            managed_source: virtual_audio.endpoint_id.clone(),
            local_config: Mutex::new(config),
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
    pub fn wait_for_capture(&self) -> bool {
        for _ in 0..60 {
            if self.hub.capture_ready.load(Ordering::Acquire) {
                return true;
            }
            thread::sleep(Duration::from_millis(50));
        }
        false
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
        let mode = self.hub.mode.load(Ordering::Acquire);
        Ok(AudioState {
            stream_mode: if mode & 1 == 0 { "quality" } else { "latency" }.into(),
            mode_revision: mode >> 1,
            mode_changed_unix_ms: self.hub.mode_changed_unix_ms.load(Ordering::Relaxed),
            host_metrics: json!({"captureStages":self.hub.capture_stages.lock().map_err(|e| e.to_string())?.clone(),"pipelinePolicy":"pcm48-prepared-join-v2","subscriberQueueDropsByDevice":self.hub.subscriber_drops_by_device.lock().map_err(|e| e.to_string())?.clone(),"subscriberQueueDrops":self.hub.subscriber_drops.load(Ordering::Relaxed),"captureTrimmedFrames":self.hub.capture_trimmed_frames.load(Ordering::Relaxed),"timelineSkippedFrames":self.hub.timeline_skipped_frames.load(Ordering::Relaxed)}),
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
        let id = device.device_id.clone();
        let worker_id = id.clone();
        let hub = self.hub.clone();
        let peer_state = state.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = match thread::Builder::new()
            .name(format!("roomwave-peer-{id}"))
            .spawn(move || {
                let result = stream_peer(&device, &worker_stop, &hub, &peer_state, tx, rx);
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
    mode: u64,
    frame: u64,
    capture_ns: u64,
    read_ns: u64,
    published_ns: u64,
    play_ns: u64,
    pcm: [u8; PCM_BYTES],
    source: [i16; FRAMES * 8],
    mask: u32,
    channels: usize,
    test_channel: Option<u32>,
}
fn assignments_path() -> std::path::PathBuf {
    crate::platform::settings_dir().join("channels.json")
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
// Called at most once per 100 ms. Keep headroom below the 0.5% playback servo limit.
fn next_playout_budget(current: i64, requested: i64) -> i64 {
    current + (requested - current).clamp(-100_000, 300_000)
}

fn startup_budget_ready(delay_ns: i64) -> bool {
    delay_ns >= PLAYOUT_NS
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_ramp_stays_within_playback_correction_and_reaches_target() {
        let mut budget = 40_000_000;
        let mut followed = budget as f64;
        let mut max_error = 0f64;
        for _ in 0..140 {
            let next = next_playout_budget(budget, 80_000_000);
            assert!((0..=300_000).contains(&(next - budget)));
            budget = next;
            // Same proportional +/-0.5% servo as PC and native Android, 5 ms blocks.
            for _ in 0..20 {
                let error = budget as f64 - followed;
                max_error = max_error.max(error.abs());
                followed += (error / 1e9).clamp(-0.005, 0.005) * 5_000_000.;
            }
        }
        assert_eq!(budget, 80_000_000);
        assert!(max_error < 4_000_000.);
        assert_eq!(next_playout_budget(80_000_000, 40_000_000), 79_900_000);
        assert_eq!(next_playout_budget(79_950_000, 80_000_000), 80_000_000);
    }
    #[test]
    fn new_peer_waits_for_clocks_and_budget_without_shifting_existing_outputs() {
        let hub = Hub::default();
        hub.delay_ns.store(42_000_000, Ordering::Relaxed);
        let (existing, _existing_rx) = mpsc::sync_channel(4);
        hub.subscribers
            .lock()
            .unwrap()
            .insert("existing".into(), existing);
        let (new, new_rx) = mpsc::sync_channel(4);
        assert!(!hub.admit_peer("new", &new, false).unwrap());
        assert!(!hub.admit_peer("new", &new, true).unwrap());
        assert_eq!(hub.delay(), 42_000_000);
        assert_eq!(hub.subscribers.lock().unwrap().len(), 1);
        assert!(new_rx.try_recv().is_err());
        hub.delay_ns.store(PLAYOUT_NS, Ordering::Relaxed);
        assert!(hub.admit_peer("new", &new, true).unwrap());
        assert_eq!(hub.delay(), PLAYOUT_NS);
        assert_eq!(hub.subscribers.lock().unwrap().len(), 2);
    }
    #[test]
    fn idle_session_can_prepare_budget_immediately_but_still_requires_clocks() {
        let hub = Hub::default();
        let (tx, _rx) = mpsc::sync_channel(4);
        assert!(!hub.admit_peer("new", &tx, false).unwrap());
        assert!(hub.subscribers.lock().unwrap().is_empty());
        assert!(hub.admit_peer("new", &tx, true).unwrap());
        assert_eq!(hub.delay(), PLAYOUT_NS);
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
                published_ns: 0,
                play_ns: n * 5_000_000 + 500_000_000,
                pcm: [0; PCM_BYTES],
                source: [0; FRAMES * 8],
                mode: 0,
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
            published_ns: 0,
            play_ns: 1_000_000_000,
            pcm: [0; PCM_BYTES],
            source: [0; FRAMES * 8],
            mode: 0,
            mask: 3,
            channels: 2,
            test_channel: None,
        });
        hub.publish(block.clone());
        assert!(Arc::ptr_eq(
            &late_rx.recv().unwrap(),
            &fast_rx.recv().unwrap()
        ));
        assert_eq!(hub.subscriber_drops.load(Ordering::Relaxed), 100);
        assert_eq!(
            hub.subscriber_drops_by_device.lock().unwrap().get("slow"),
            Some(&100)
        );
        assert_eq!(
            hub.subscriber_drops_by_device.lock().unwrap().get("fast"),
            None
        );
        hub.subscribers.lock().unwrap().remove("slow");
        hub.publish(block);
        assert!(fast_rx.try_recv().is_ok());
    }
}
