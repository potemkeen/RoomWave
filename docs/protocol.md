# RoomWave — protocol v3

Version 3 adds a capture timestamp and latency measurement. Windows and Android must be updated together: older versions are rejected during discovery/handshake.

## Discovery

DNS-SD type `_roomwave._udp.local.`; Android NSD receives `_roomwave._udp.`. Instance name `RoomWave-<8 ID characters>` may change in case of a conflict. Identity is determined by `deviceId`.

| TXT | Value |
| --- | --- |
| deviceId | Persistent UUID from SharedPreferences; backup/transfer disabled |
| deviceName | Manufacturer and model, UTF-8 |
| protocolVersion | String `3` |

Fields are required, non-empty, contain no control characters, and are limited to 200 bytes. Additional TXT fields are ignored. SRV points to UDP 47800. The address is taken from A/AAAA, with IPv4 preferred. TCP control uses fixed port 47801 on the same address. These ports were chosen by the project; this is not a claim of IANA registration.

The advertisement is owned by the foreground service, not the Activity. Ready means NSD registration succeeded; connection status is shown separately. Stopping the service removes the advertisement. After an unexpected disconnect, removal from the list depends on the mDNS TTL; Refresh clears the cache and starts discovery again. mDNS uses UDP 5353, multicast 224.0.0.251 / ff02::fb.

## Control: TCP 47801

UTF-8 JSON, one message per line, maximum 4096 bytes. One active session per Receiver. Integer nanoseconds and sessionId are transmitted as **strings** to avoid precision loss in JSON clients.

The Host sends:

```json
{"type":"start","protocolVersion":3,"deviceId":"UUID","sessionId":"123","sampleRate":48000,"channels":2,"framesPerPacket":240,"format":"s16le"}
```

The Receiver validates the format/ID, obtains AudioFocus, starts UDP and replies with `{"type":"ready","sessionId":"123"}`. An unsupported format receives an `error` message. UDP starts only after ready. `{"type":"stop"}` ends the session. EOF and errors also terminate it.

Example heartbeat once per second:

```json
{"type":"ping","hostSendNs":"1000000000","clockOffsetNs":null,"rttMs":null}
{"type":"pong","hostSendNs":"1000000000","phoneReceiveNs":"912000000","phoneSendNs":"915000000","packets":200,"lost":0,"playedFrames":48000,"latencyMs":null,"rttMs":null,"latencyMethod":null}
```

`playedFrames` is the historical name of the counter for frames **written** to AudioTrack, not frames that have actually been heard. Missing heartbeat for 5 seconds or missing valid UDP packets for 3 seconds terminates the connection. The wake lock is extended by heartbeat and released when the session ends.

## UDP 47800

Exactly **1000 bytes**: 40-byte big-endian header + 960 bytes of interleaved signed 16-bit little-endian PCM, stereo, 48 kHz.

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 4 | ASCII `RWAV` |
| 4 | 1 | Version `3` |
| 5 | 1 | Format `1` = PCM s16le |
| 6 | 1 | Channels `2` |
| 7 | 1 | Reserved `0` |
| 8 | 8 | Session ID, positive int64 |
| 16 | 4 | Sequence, uint32 starting from zero |
| 20 | 8 | Frame index = sequence × 240 |
| 28 | 2 | Frames = 240 |
| 30 | 2 | Reserved `0` |
| 32 | 8 | Capture time of the first frame, Windows QPC in nanoseconds; `0` = unknown/synthetic silence |
| 40 | 960 | PCM |

The Receiver validates size, magic, version, format, session, frame index and sender address matching the TCP peer. Duplicate packets are not added to the queue a second time. Late packets are not played. Sequence is currently not unwrapped after uint32 overflow; continuous sessions longer than approximately 248 days require a new session.

## Latency and clocks

The clocks are independent of calendar time: Windows QueryPerformanceCounter and Android System.nanoTime. The WASAPI timestamp in 100 ns units is converted to nanoseconds. Timestamps for each captured frame are preserved together with PCM, including queue trimming.

`t1` — ping sent by the PC; `t2` — ping parsed by the phone; `t3` — pong sent; `t4` — pong processed by the PC.

```text
RTT = (t4 - t1) - (t3 - t2)
offset = ((t2 - t1) + (t3 - t4)) / 2
phoneCaptureTime = hostCaptureTime + offset
latency = estimatedPresentationTime - phoneCaptureTime
```

The Host verifies that the pong matches the expected ping. It selects the minimum RTT among fresh exchanges from the last 10 seconds and sends the offset and RTT in subsequent pings. The offset means “Android minus Windows”. Asymmetric directional delays introduce error; RTT/2 indicates the scale of this uncertainty but not the full audio error.

Every 250 ms, the Receiver estimates the presentation time of the first frame of the next written packet using AudioTimestamp, or playback head + current time as a fallback. AudioTimestamp is cached for up to 10 seconds and reset on underrun/route change. The estimate is smoothed using an EMA with a new-sample weight of 0.25. When the method changes or clock correction exceeds 5 ms, smoothing restarts. Missing measurements for 1.5 seconds or missing clock sync for 5 seconds hides the value. Unknown/negative/>5 second estimates are discarded. RTT is accepted in the range 0–1000 ms.

`latencyMs`, `rttMs`, `latencyMethod` (`timestamp` / `queue` / null) in pong are a single snapshot used by both interfaces. Before they are ready, null is available. Interfaces update approximately once per second; this is not an acoustic speaker measurement and not a tool for measuring exact hardware latency.

Timestamp references: [Microsoft IAudioCaptureClient::GetBuffer](https://learn.microsoft.com/en-us/windows/win32/api/audioclient/nf-audioclient-iaudiocaptureclient-getbuffer), [Android AudioTrack](https://developer.android.com/reference/android/media/AudioTrack#getTimestamp(android.media.AudioTimestamp)).

## Optional PCM repair extension (2026-09-20)

Protocol 4 clients may advertise `xorFec: true` in `ready`. The host then sends
one `RWFX` parity datagram after every two consecutive routed originals. Existing
clients receive only their original RWAV stream. Original datagrams are sent
immediately; the common frame index, deadline, PCM format and routing are unchanged.

RWFX header (big endian): bytes 0..3 `RWFX`; byte 4 version 1; byte 5 reserved 0;
bytes 6..7 original datagram length (544 mono or 1024 stereo); bytes 8..15 session;
bytes 16..23 first frame index. Payload is the bytewise XOR of the two **complete**
original RWAV datagrams (including timing headers). Frames differ by 240. Total
length is 568 or 1048 bytes; no IP fragmentation is required on a normal LAN.
Discontinuity or payload format change restarts the pair. Parity is emitted once,
does not wait for a NACK and is not itself retransmitted.

One missing original is reconstructed bit exactly if the other original and parity
arrive in time. Two missing originals cannot be reconstructed. Receivers keep a
bounded 64-slot cache, accept parity before originals, validate the reconstructed
RWAV header/session/frame and retain the original playback deadline. Never play
repair data after that deadline has been committed to the output device.

For clients advertising `xorFec`, retransmitted RWAV uses flags byte 7 = 3
(extended timing + retransmission marker). Its original send/read timestamps are
preserved. FEC protects flags=1 originals, so a flags=3 retransmission must be
normalized back to flags=1 before entering the FEC cache. Legacy receivers retain
the previous flags=1 retransmission behavior. New clients accept both.

The receiver briefly allows reorder (2 ms after detecting a gap) before NACK;
retry spacing uses at least the observed repair RTT p95 when enough samples exist.
Repair remains subject to the output submission deadline. It must not block
current PCM or independently shift a device's playback schedule.

## Optional 16 kHz PCM profile

Ready capability `pcm16k:true`. RWAV flags 5 (original) / 7 (retransmission)
indicate 16 kHz; bytes 30..31 contain 16000, replacing the reserved zero.
Sequence and frame index retain the original 48 kHz timeline and increment by
one packet / 240 frames. Bytes 28..29 remain 240: **output/timeline** frames.
The payload contains 21 preceding + 80 current s16le interleaved samples per
channel at 16 kHz. Datagram lengths: 266 mono, 468 stereo. Corresponding RWFX
lengths: 290 and 492. FEC pairs restart on length changes; cache normalization
clears bit 1 for retransmission flags (7 -> 5, 3 -> 1).

Sender applies a 63-tap normalized Hamming-windowed sinc FIR (cutoff 6500/48000)
and keeps every third sample starting at offset 0. Receiver inserts two zeros
between samples, applies the same FIR with gain 3 using the included history,
and outputs 240 stereo frames. No arrival-order-dependent resampler state is needed.
Combined group delay is 62/48000 seconds: subtract its integer nanoseconds from
the nominal playback deadline; receiver latency estimation adds this signal delay
back to keep the two profiles comparable. Mode changes do not reset clocks,
retransmission caches, session IDs or cumulative counters. An invalid rate/length/
flags combination must be rejected, not guessed from packet length alone.
