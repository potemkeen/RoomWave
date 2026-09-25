# Diagnostic session recording

Detailed recording is OFF by default (including release builds). In Diagnostics,
choose “Записать сеанс · 10 минут” to create a JSONL file in
`%LOCALAPPDATA%\RoomWave\logs\session-<Unix milliseconds>-<PID>.jsonl`.
The Diagnostics panel displays the current path, sample count and recording errors.
No PCM/audio data is recorded. Files contain device names/IDs, endpoint identifiers,
channel assignments, playback/test status, errors, and cumulative receiver metrics.

A dedicated worker samples AudioSession every 500 ms, independently of the UI.
Disk serialization and flushing happen after releasing the snapshot locks, outside
capture, network and playback workers. Each complete line is independently readable.
`unixMs` aligns approximate wall-clock observations; `elapsedMs` is monotonic within
a session. `lastReportUnixMs` distinguishes fresh receiver statistics from repeated
snapshots. These are sampled state records, not a packet trace: sub-500 ms state
transitions may be missed. Counters can reset on reconnection and need per-session
segmentation when computing deltas. Recovered packets do not guarantee on-time playback.

Recording stops automatically after 10 minutes, on manual stop, or at 64 MiB / an I/O error;
audio continues. Files from earlier runs are not overwritten or deleted. Start
another session with the same button; no application restart is needed. Use
“Показать файл” to select the recording in Explorer. No recording worker or periodic
file writes run while recording is disabled. Normal exit appends `session_end`; abrupt termination
may leave no end record, but complete preceding lines remain usable.

For a test: connect both phones, play audio for 3–5 minutes, note wall-clock times
of audible glitches, pause/resume, and screen-off changes. Close the host or simply
send the latest log. Logs from another computer must be copied from that computer.
The app currently does not capture an acoustic end-to-end latency measurement.

## Recovery experiment (2026-09-20)

`audio.hostMetrics` contains process-lifetime aggregate counters:
`subscriberQueueDrops` (all subscribers including local output),
`captureTrimmedFrames`, `timelineSkippedFrames`. These do not reset on peer reconnect.

Each receiver's `transport` is per connection: `fecEnabled`, `expiredBeforeSend`,
`udpWouldBlock`, `fecSent`, `fecSendFailed`, `nackReceived`, `repairsSent`,
`repairUnavailableOrExpired`, `repairSendFailed`. `packetsSent` still counts only
successful original UDP sends, excluding parity and retransmissions.
`readToPublish`, `publishToSend`, `sendIntervals`, `rawControlRtt` contain cumulative
histograms (samples, p50/p95/p99 rounded up to milliseconds, exact maximum).
The top histogram bucket includes all values >=255 ms; use maxMs for the tail.
Read-to-publish does not measure pre-read WASAPI/virtual-device buffering.
Send intervals include idle/discontinuity gaps; rawControlRtt is TCP control RTT,
not UDP repair RTT. Displayed rttMs remains the low-RTT clock-sync sample.

Android `stages` adds `originalsReceived`, `repairsReceived`, `nativeRejected`,
`fecRecovered`, `fecAccepted`, `fecInvalid`, `playedPackets`, `fecPlayed`,
`retransmitPlayed`, `duplicates`, `queueCollisions`. `recoveredPackets` now counts
repairs accepted by the native queue, not merely arriving after a NACK. A played
packet counter increments when the first PCM sample of that packet is rendered;
it does not certify complete or acoustic playback. Do not sum loss, late and
rejected as independent losses. FEC recovery without native acceptance is not success.

`transitP50Ms/P95Ms/P99Ms/MaxMs` sample original packets only over the last 2048
samples, excluding reconstructed and marked retransmitted packets. These are
clock-offset estimates, not calibrated one-way physical latency. `repairRttP95Ms`
and `repairRttSamples` measure request-to-arrival of marked retransmissions over
128 samples; after retries attribution to the latest attempt is approximate.
`suggestedJitterMs` is a **shadow** percentile-based recommendation, with a 200-sample
warm-up. It does not change `recommendedJitterMs` or the shared playback budget yet.

The first comparison deliberately retains the existing output/buffer policy to
isolate packet handling and recovery improvements. No new latency result should be
claimed before a phone run. Next: compare the same two phones on 5 GHz with local
PC output enabled, including pause/resume and screen-off; then use measured tails
and output limits to choose a smaller coordinated budget.

## Android GAME usage experiment

`audioPolicy=game-usage-v1` requests AAUDIO_USAGE_GAME when the API-28 usage
functions are available. API 26/27 retains the previous default. If both exclusive
and shared GAME opens fail, opening retries with MEDIA. All symbol lookup and
opening happen outside the callback. This does not force exclusive/MMAP support.

Additional stages: `exclusiveOpenResult` is the result of the first exclusive
request (0 means open succeeded, not necessarily exclusive granted);
`sharedOpenRetried` records explicit shared retry after that request failed;
`gameUsageRequested`, `mediaUsageFallback`, `actualUsage` (-1 if unavailable),
`actualSampleRate`, `audioDeviceId`, `bufferCapacityFrames`. `sharingMode` and
`performanceMode` remain the actual modes. These fields do not identify MMAP.

PCM, channel routing, packet/FEC policy, jitter recommendation and the two-burst
hardware buffer policy are unchanged. The existing host may automatically lower
the common schedule if measured output latency decreases; its network margin is
unchanged. Quality and packet-loss non-regression are acceptance criteria, not
guarantees provided by changing usage. Compare against the previous FEC build on
the same network/devices/routes, including screen-off and pause/resume. Require
no increase in missing-PCM rate or xruns and no audible degradation before keeping
this policy. The pre-experiment APK is retained locally at
`.build-tmp/app-before-game-usage.apk` for rollback/comparison.

## Manual quality/latency comparison

Historical A/B format: the 16 kHz host mode was removed after the September 21
test. The current host sends only quality/48 kHz; mode fields remain for log
compatibility. Android can still decode packets from older experimental hosts.

`audio.streamMode` is `quality` or `latency`; `modeRevision` increases on an actual
selection change, `modeChangedUnixMs` records wall-clock selection time. Logger
emits `mode_changed` on its next snapshot (and an initial record), including peer
counter snapshots. Very fast repeated toggles between two 500-ms samples may be
coalesced; revisions expose this. The UI applies the selection at a source block
boundary, so in-flight old-profile blocks may still play briefly.

Per-peer `transport.streamMode/modeRevision` describes the last routed block;
`qualityPacketsSent`, `latencyPacketsSent`, `originalBytesSent`, `fecBytesSent`
are cumulative successful sends. Receiver stages expose `streamMode`,
`wireSampleRate`, `resamplerDelayMs`, `qualityPacketsReceived`,
`latencyPacketsReceived`, `audioWireBytesReceived` (valid original/RTX datagrams,
not parity), and `playbackSampleRate` (source rate of last packet started by the
renderer, not hardware rate). Hardware `actualSampleRate` remains 48000.
`qualityPlayed/latencyPlayed`, `qualityLost/latencyLost` count renderer blocks;
missing blocks are attributed to the last known playing profile, so exclude the
first two seconds around a transition when comparing rates. Use counter deltas,
not whole-session totals or a mean over both modes. No counters reset on switches.

## PCM48 event wake experiment (21 September 2026)

`audio.hostMetrics.pipelinePolicy` and each peer's `transport.pipelinePolicy`
are `pcm48-event-wake-v1`. The host now waits for audio with a 2 ms control-work
timeout instead of sleeping unconditionally for 2 ms. Incoming audio wakes the
sender immediately. Queue capacity, playback schedule, FEC and jitter budgets
are unchanged. One already-dequeued pending block may be held by the sender.
The unused continuous 48→16 kHz filter has been removed from the sender path.

`subscriberQueueDropsByDevice` attributes cumulative full-queue drops to each
device ID, including `__local_output`. It survives reconnects within the host
session, just like the aggregate counter. `packetProcessing` measures routing,
send, FEC and repair-cache insertion. `senderIteration` measures active sender
loop work (including control processing), excluding the final audio wait.
Both are per-connection cumulative host histograms, with millisecond buckets.
`publishToSend` remains queue/dispatch latency, not full socket-send processing.

Android `receiverPolicy=pcm-scratch-v1`: a reusable 960-byte PCM buffer replaces
one per-packet allocation. JNI copies it synchronously into native storage before
returning, so later packets/recovery can reuse it safely. Packet metadata and
FEC allocations still exist; this is not a claim of a zero-allocation pipeline.

`decodeProcessing`, `nativePushProcessing`, `receiveProcessing` expose `samples`,
`firstMs`, `p50Ms`, `p95Ms`, `p99Ms`, `maxMs`. Recording uses fixed atomic storage;
JSON snapshots run on the metrics thread. Quantiles are approximate upper bucket
boundaries (0.125 ms); the overflow bucket uses observed max. These are lifetime
connection statistics, including startup, and may be slightly inconsistent during
concurrent sampling. `receiveProcessing` runs from socket receive return through
decode, FEC, periodic network summaries and NACK processing; it excludes time
blocked inside receive. It includes any scheduling pauses inside that interval.
Nested measurements must not be summed. No claim that this measures NIC arrival.

Validation: Rust routing/FEC/isolation tests plus per-device drop attribution;
Android PCM buffer ownership/content regression and timing histogram tests;
Android unit tests/lint and full host production build. Real latency/loss outcome
requires a new run on both phones with the same network and screen state.

## Prepared joins and capture packet publication (21 September 2026)

Host policy `pcm48-prepared-join-v2`. New receivers reserve an initial 80 ms
group budget and exchange at least three clock samples before subscribing to PCM.
The subscription is created only when the group budget is ready; handshake audio
no longer fills and overflows a queue that nobody is draining. With no active
subscribers the initial budget can be set immediately. With existing outputs the
normal smooth ramp is preserved: preparing a join from 42 ms can take about
38 seconds at 1 ms/s. This is connection preparation time, not an additional
permanent playback buffer. Once admitted, normal adaptive requests resume.
No independent timeline shift or replay of handshake packets is introduced.
An 80 ms initial budget is a conservative policy for the currently tested outputs,
not a measured guarantee for arbitrary Android hardware or networks.

Per-peer transport adds `admitted` and `preparationMs` (elapsed while waiting,
fixed after admission). New tests cover admission with/without existing outputs
and refusal to start before clocks/budget are ready.

Capture now reads one WASAPI engine packet, publishes all resulting complete
5 ms blocks, then reads the next ready engine packet without an event wait.
It previously drained all available engine packets before publishing any PCM.
`hostMetrics.captureStages` includes cumulative histograms `readWork`,
`packetizationWork`, `eventWait`, `readToPublish`, plus `maxReadFrames` and
`eventTimeouts`. These reset on capture endpoint reinitialization. `readWork`
includes WASAPI read and timestamp bookkeeping; `packetizationWork` includes
publication and can include lock/scheduling delay. `eventWait` includes time
inside the OS wait. `eventTimeouts` counts unsuccessful waits, including errors;
it is not an audio-loss counter. Millisecond quantiles are upper bucket bounds.
These metrics distinguish driver/event wake delay from processing and dispatch;
they do not measure sound before capture or acoustic output.

Android and wire protocol are unchanged for this step. A new host run is needed
to confirm startup loss reduction and whether capture publication tails improve.

## Screen, power and network state

Receiver stages now include `deviceState.schema=1`. A separate HandlerThread
queries Android once per second and after screen/power broadcasts. Cached
snapshots travel in existing control replies and appear in PC JSONL files.
No system service queries run in UDP or AAudio callbacks. The host bounds
incoming diagnostic control data at16KiB (previously4KiB); use the updated host
with this APK. No new permissions or playback-policy changes are introduced.

Fields: sampleUnixMs, phoneMonoNs, androidApi/androidRelease/manufacturer,
screenInteractive, powerSave, deviceIdle, ignoringBatteryOptimizations,
backgroundRestricted (API28+), processImportance, plugged, batteryLevel,
partialWakeLockHeld, wifiLockHeld, wifiLockRequested, networkHandle,
wifiTransport/vpnTransport, wifiFrequencyMhz, wifiRssiDbm and wifiLinkMbps.
Unavailable Wi-Fi values are null. `ignoringBatteryOptimizations` describes the
Android Doze allowlist, not every vendor battery restriction. `wifiLockHeld`
means the application holds the request, not that the driver disabled power save.
Frequency/link speed/RSSI describe exposed Wi-Fi connection metadata, not measured
throughput or airtime; default network may differ from an existing socket route.

`recentEvents` retains8 events with monotonic revision, kind, unixMs, phoneMonoNs
and screenInteractive. Events are initial, screen ON/OFF, power connected/
disconnected, power-save/idle-mode broadcasts and default-network changes.
Timestamps mark delivery to our background handler, not physical button presses.
The bounded history allows transitions to survive delayed control delivery;
more than8 changes between successful reports can be truncated. Deduplicate
by event revision within a receiver connection. Metadata sampling failures are
reported as queryError; missing fields must not be interpreted as false/zero.

SSID/BSSID are not collected and location permissions are not requested. Thus
roaming between access points inside the same default network may be invisible.
Screen OFF does not prove Doze, and screenInteractive is not a screen-brightness
measurement. Compare observed screen events with late/missing counter deltas,
receiveProcessing and transit tails before assigning a cause.

Next experiment: Xiaomi ON2min→OFF2min→ON2min with fixed AP/power settings, then
a separate run after manually removing app battery restrictions. Battery
exemption does not guarantee removal of Wi-Fi firmware/Android screen-off
restrictions. Official references:
https://developer.android.com/training/monitoring-device-state/doze-standby
https://developer.android.com/reference/android/net/wifi/WifiManager#WIFI_MODE_FULL_HIGH_PERF

Release hosts keep only an asynchronous, best-effort error journal by default: errors.log
and errors.previous.log, at most 512 KiB each. The queue holds at most 64 messages;
errors are truncated, repeating consecutive errors are rate-limited, and a full queue
drops records rather than blocking an audio worker. Debug builds retain console logs.
Android no longer emits periodic stage snapshots to Logcat. Error/session messages
and metrics sent to the host remain available; synchronization/FEC/buffer feedback
is unchanged. No PCM is recorded.
