use crate::discovery::Device;
use crate::timing::{Clock, ClockSync};

use serde_json::{json, Value};

use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpStream, UdpSocket},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use super::wire::{routed_packet, RoutedPacket};
use super::{AudioBlock, Hub, ReceiverState, Res, FRAMES, PLAYOUT_NS};

fn routing_metadata(hub: &Hub, device_id: &str) -> Value {
    let speaker = hub.assignments.lock().unwrap().get(device_id).copied();
    let state = hub.state.lock().unwrap();
    let available = speaker.is_none_or(|s| state.layout.channels.iter().any(|c| c.mask == s));
    json!({"speaker": speaker, "available": available})
}

pub(super) fn stream_peer(
    device: &Device,
    stop: &AtomicBool,
    hub: &Hub,
    shared: &Mutex<ReceiverState>,
    tx: SyncSender<Arc<AudioBlock>>,
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
    let start = json!({"type":"start", "protocolVersion":4, "deviceId":device.device_id, "sessionId":session.to_string(), "sampleRate":48000, "channels":2, "framesPerPacket":FRAMES, "format":"s16le", "targetDelayMs":PLAYOUT_NS / 1_000_000, "routing":routing_metadata(hub, &device.device_id)});
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
    let low_rate_supported = response["pcm16k"].as_bool() == Some(true);
    shared.lock().map_err(|e| e.to_string())?.low_rate_supported = low_rate_supported;
    if hub.mode.load(Ordering::Acquire) & 1 != 0 && !low_rate_supported {
        return Err("Update Android to use the 16 kHz profile".into());
    }
    let fec_supported = response["xorFec"].as_bool() == Some(true);
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
    // Reserve a safe initial budget before subscribing to PCM. Existing outputs
    // follow the normal smooth ramp; an idle capture can initialize immediately.
    hub.state
        .lock()
        .map_err(|e| e.to_string())?
        .budgets
        .insert(device.device_id.clone(), PLAYOUT_NS);
    let mut admitted = false;
    let mut clock_samples = 0u32;
    log::info!("receiver connected {} at {address}", device.device_name);
    let clock = Clock::new();
    let mut sync = ClockSync::default();
    let mut pending_ping: Option<i64> = None;
    let mut last_pong = Instant::now();
    let mut last_ping = Instant::now() - Duration::from_secs(1);
    let connected = Instant::now();
    let mut preparation_ms = 0u64;
    let mut incoming = Vec::new();
    let mut outgoing: VecDeque<u8> = VecDeque::new();
    let mut sent = 0u64;
    let mut last_mode = 0u64;
    let (mut quality_packets, mut latency_packets, mut original_bytes, mut fec_wire_bytes) =
        (0u64, 0u64, 0u64, 0u64);
    let mut fec_encoder = crate::fec::Encoder::default();
    let mut fec_bytes = [0u8; crate::fec::MAX];
    let (
        mut expired,
        mut would_block,
        mut fec_sent,
        mut fec_failed,
        mut nack_received,
        mut repairs_sent,
        mut repairs_expired,
        mut repair_failed,
    ) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
    let mut read_to_publish = crate::transport_stats::Histogram::default();
    let mut publish_to_send = crate::transport_stats::Histogram::default();
    let mut send_intervals = crate::transport_stats::Histogram::default();
    let mut raw_rtt = crate::transport_stats::Histogram::default();
    let mut last_send = 0u64;
    let mut packet_processing = crate::transport_stats::Histogram::default();
    let mut sender_iterations = crate::transport_stats::Histogram::default();
    let mut pending_block = None;
    let mut recent: VecDeque<RoutedPacket> = VecDeque::with_capacity(64);
    let result = (|| -> Res<()> {
        while !stop.load(Ordering::Relaxed) {
            let iteration_started = Instant::now();
            // Endpoint changes may briefly pause capture; keep peer clocks/connections alive.
            // Drain stale backlog after handshake; deadlines are shared, never shifted per receiver.
            for _ in 0..40 {
                let block = match pending_block
                    .take()
                    .map(Ok)
                    .unwrap_or_else(|| rx.try_recv())
                {
                    Ok(block) => block,
                    Err(_) => break,
                };
                if block.play_ns as i64 <= clock.now() {
                    expired += 1;
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
                let send_at = clock.now() as u64;
                read_to_publish.add_ns(block.published_ns.saturating_sub(block.read_ns));
                publish_to_send.add_ns(send_at.saturating_sub(block.published_ns));
                if last_send > 0 {
                    send_intervals.add_ns(send_at.saturating_sub(last_send));
                }
                last_send = send_at;
                let processing_started = Instant::now();
                let routed = routed_packet(session, &block, send_at, speaker);
                last_mode = block.mode;
                let sent_result = udp.send(&routed.bytes[..routed.len]);
                match sent_result {
                    Ok(_) => {
                        sent += 1;
                        original_bytes += routed.len as u64;
                        if block.mode & 1 == 0 {
                            quality_packets += 1;
                        } else {
                            latency_packets += 1;
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        would_block += 1;
                    }
                    Err(e) => return Err(e.into()),
                }
                if fec_supported {
                    if let Some(len) = fec_encoder.push(
                        session,
                        routed.frame,
                        &routed.bytes[..routed.len],
                        &mut fec_bytes,
                    ) {
                        match udp.send(&fec_bytes[..len]) {
                            Ok(_) => {
                                fec_sent += 1;
                                fec_wire_bytes += len as u64;
                            }
                            Err(_) => fec_failed += 1,
                        }
                    }
                }
                if recent.len() == 64 {
                    recent.pop_front();
                }
                recent.push_back(routed);
                packet_processing.add_ns(processing_started.elapsed().as_nanos() as u64);
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
                        nack_received += 1;
                        let frame = u64::from_be_bytes(nack[16..24].try_into().unwrap());
                        let now = clock.now();
                        let guard =
                            sync.best(now).map(|s| s.rtt_ns).unwrap_or(20_000_000) + 5_000_000;
                        if let Some(block) = recent
                            .iter()
                            .find(|b| b.frame == frame && b.play_ns as i64 > now + guard)
                        {
                            let mut bytes = block.bytes;
                            if fec_supported {
                                bytes[7] |= 2;
                            } else {
                                bytes[48..56].copy_from_slice(&(now as u64).to_be_bytes());
                            }
                            match udp.send(&bytes[..block.len]) {
                                Ok(_) => repairs_sent += 1,
                                Err(_) => repair_failed += 1,
                            }
                        } else {
                            repairs_expired += 1;
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
                if incoming.len() > 16 * 1024 {
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
                        let rtt_ns =
                            (i128::from(t4) - i128::from(t1)) - (i128::from(t3) - i128::from(t2));
                        if t3 >= t2 && (0..=1_000_000_000).contains(&rtt_ns) {
                            raw_rtt.add_ns(rtt_ns as u64);
                        }
                        sync.observe(t1, t2, t3, t4);
                        clock_samples = clock_samples.saturating_add(1);
                    }
                    pending_ping = None;
                    last_pong = Instant::now();
                }
                if admitted && message["stages"]["outputBackend"] == "AAudio" {
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
                s.transport = json!({"streamMode":if last_mode&1==0 {"quality"} else {"latency"},"modeRevision":last_mode>>1,"qualityPacketsSent":quality_packets,"latencyPacketsSent":latency_packets,"originalBytesSent":original_bytes,"fecBytesSent":fec_wire_bytes,"pipelinePolicy":"pcm48-prepared-join-v2","admitted":admitted,"preparationMs":if admitted {preparation_ms} else {connected.elapsed().as_millis() as u64},"packetProcessing":packet_processing.snapshot(),"senderIteration":sender_iterations.snapshot(),"fecEnabled":fec_supported,"expiredBeforeSend":expired,"udpWouldBlock":would_block,
                    "fecSent":fec_sent,"fecSendFailed":fec_failed,"nackReceived":nack_received,"repairsSent":repairs_sent,
                    "repairUnavailableOrExpired":repairs_expired,"repairSendFailed":repair_failed,
                    "readToPublish":read_to_publish.snapshot(),"publishToSend":publish_to_send.snapshot(),
                    "sendIntervals":send_intervals.snapshot(),"rawControlRtt":raw_rtt.snapshot()});
                s.packets_sent = sent;
                s.receiver_packets = message["packets"].as_u64().unwrap_or(0);
                s.receiver_lost = message["lost"].as_u64().unwrap_or(0);
                s.last_report_unix_ms = Some(
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                );
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
            let interval = if !admitted || connected.elapsed() < Duration::from_secs(3) {
                50
            } else {
                500
            };
            if outgoing.is_empty()
                && pending_ping.is_none()
                && last_ping.elapsed() >= Duration::from_millis(interval)
            {
                let now = clock.now();
                let sample = sync.best(now);
                let ping = json!({"type":"ping", "hostSendNs":now.to_string(), "clockOffsetNs":sample.map(|s| s.offset_ns.to_string()), "rttMs":sample.map(|s| s.rtt_ns as f64 / 1_000_000.0), "routing":routing_metadata(hub, &device.device_id)});
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
            if !admitted
                && hub.admit_peer(
                    &device.device_id,
                    &tx,
                    clock_samples >= 3 && sync.best(clock.now()).is_some(),
                )?
            {
                admitted = true;
                preparation_ms = connected.elapsed().as_millis() as u64;
                shared.lock().map_err(|e| e.to_string())?.status = "streaming".into();
            }
            sender_iterations.add_ns(iteration_started.elapsed().as_nanos() as u64);
            // Wake immediately on PCM; retain a bounded timeout for control/NACK work.
            // This is a capacity limit, not a playback prebuffer.
            pending_block = match rx.recv_timeout(Duration::from_millis(2)) {
                Ok(block) => Some(block),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err("Capture queue disconnected".into())
                }
            };
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
        .filter(|value| value.is_finite() && (min..=max).contains(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Layout;

    #[test]
    fn routing_metadata_tracks_assignment_and_layout_changes() {
        let hub = Hub::default();

        hub.state.lock().unwrap().layout = Layout::new(6, 0x3f);

        assert_eq!(
            routing_metadata(&hub, "phone"),
            json!({"speaker":null,"available":true})
        );

        hub.assignments.lock().unwrap().insert("phone".into(), 16);

        assert_eq!(
            routing_metadata(&hub, "phone"),
            json!({"speaker":16,"available":true})
        );

        hub.state.lock().unwrap().layout = Layout::new(6, 0x60f);

        assert_eq!(
            routing_metadata(&hub, "phone"),
            json!({"speaker":16,"available":false})
        );

        hub.assignments.lock().unwrap().insert("phone".into(), 512);

        assert_eq!(
            routing_metadata(&hub, "phone"),
            json!({"speaker":512,"available":true})
        );

        hub.assignments.lock().unwrap().remove("phone");

        assert_eq!(
            routing_metadata(&hub, "phone"),
            json!({"speaker":null,"available":true})
        );
    }
}
