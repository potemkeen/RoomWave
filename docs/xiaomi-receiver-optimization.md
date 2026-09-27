# Xiaomi receiver optimization experiment

Date: 2026-09-27. Status: implementation for device testing, not a measured latency improvement.

## Evidence and constraints

Xiaomi 23106RN0DA previously returned AAudio performance NONE/shared for all four
Game/Media and exclusive/shared requests, with a 770-frame burst and 1540-frame
buffer at 48 kHz. Samsung granted low latency with 240/480 frames. Buffer duration
is not total presentation latency. Packets rejected after their playback deadline
are not necessarily packets lost by Wi-Fi.

The Xiaomi logs identify Android 15. Android documents that HIGH_PERF Wi-Fi locks
are mapped to LOW_LATENCY starting at API 34, including screen/foreground
restrictions. A held lock is not proof of active Wi-Fi latency protection.
Firmware behavior still needs physical verification; battery exemptions do not
guarantee disabling radio power save.

## Changes

- Keep the original four AAudio candidates first. Stop immediately if a valid
  low-latency stream is granted, preserving the existing Samsung path.
- When none is fast, additionally try native-rate float and native-rate PCM16,
  each with the same four usage/sharing combinations. Accept only actual 48 kHz
  stereo. Reject other rates rather than playing PCM on an incorrect clock.
  Fall back to the valid candidate with the smallest burst, preferring the original
  candidate on ties. Open one stream at a time; never reopen during playback.
- PCM16 uses the existing renderer and fixed 240-frame scratch storage. Conversion
  is saturated/rounded; it adds no queue, lossy codec, or network format change.
- Keep two-burst initial buffering and existing xrun-driven expansion. Record the
  actual buffer setter result rather than assuming the request was honored.
- Update hardware presentation lead without requiring successfully played PCM.
  Feed the larger of that lead and the existing per-packet estimate to the host
  and retransmission deadline checks. This can correctly *raise* an insufficient
  budget; it does not manufacture lower latency by underreporting output cost.
- Request Android AUDIO priority for the UDP worker only. A denied request falls
  back gracefully. No realtime scheduler/root permission is requested.
- Continue native timestamp polling at 20 Hz; construct detailed telemetry JSON
  at 4 Hz, reducing monitor allocations. No per-packet Logcat/disk logging added.
- Optional “Не гасить экран при подключении” in Android diagnostics, off by default.
  It only keeps the visible app awake while connected. Leaving the app/manual power
  button still permits screen off. This is a battery-costing workaround, not a
  background Wi-Fi fix.

## Additional measurements

Existing host session logs automatically retain Android `stages`:

- `audioPolicy = native-format-probe-v3`, all attempted format/rate candidates,
  selected actual format/rate, buffer setter result.
- Callback count, largest callback, maximum callback interval and work time,
  timestamp age, and continuously estimated hardware deadline lead.
- UDP worker priority, receive interval histogram, and timeout-overrun histogram.
  A delayed 2 ms timeout is evidence of a late wakeup/socket return, not proof of
  a particular scheduler or radio cause. Disambiguation can require Perfetto.
- API-dependent Wi-Fi lock restrictions, necessary foreground/screen eligibility,
  and the keep-screen setting. Eligibility does not prove driver enforcement.

Callback maxima and receive histograms are session-cumulative. Reconnect between
experiments or inspect counters and screen events in context, not just maxima.

## Device validation

1. Install the test APK. Use the same host and fixed AP/band, without changing
   sound effects, battery settings or physical placement for the baseline.
2. Xiaomi alone: enable host diagnostic recording, run 2 minutes screen-on,
   2 minutes off, 2 minutes on. Record whether USB/charging is connected.
3. Repeat with Xiaomi battery policy “No restrictions”, keeping other conditions
   identical. Test the optional keep-screen workaround separately; do not count
   that as a successful screen-off fix.
4. Repeat with both Xiaomi and Samsung to check sync and group latency. The host
   intentionally retains a common timeline; a slow receiver still sets a floor.
5. Compare granted audio mode, output lead, p95/p99 transit, incremental late/lost
   PCM and xruns, FEC/retransmissions actually played, callback timing and receive
   timeout overrun. Do not equate packet arrivals with successful playback.

If all candidates remain slow, use OboeTester and `dumpsys media.audio_flinger`
to investigate native output capabilities and effects. OpenSL ES is not adopted
blindly: a replacement must preserve reliable presentation timestamps and sync.
Changing host WASAPI periods is a separate experiment to avoid confounding this
receiver comparison. Lowering quality or the hardware buffer is not part of this patch.

## Sources

- [Android low-latency checklist](https://developer.android.com/games/sdk/oboe/low-latency-audio)
- [Oboe fast-path FAQ](https://github.com/google/oboe/blob/main/docs/FAQ.md)
- [Oboe format/rate workarounds](https://github.com/google/oboe/blob/main/src/common/QuirksManager.cpp)
- [Oboe AAudio backend](https://github.com/google/oboe/blob/main/src/aaudio/AudioStreamAAudio.cpp)
- [Wi-Fi lock behavior](https://developer.android.com/reference/android/net/wifi/WifiManager#WIFI_MODE_FULL_HIGH_PERF)
- [OboeTester](https://github.com/google/oboe/tree/main/apps/OboeTester)
- [Perfetto scheduling traces](https://perfetto.dev/docs/data-sources/cpu-scheduling)

These informed the implementation; no third-party code was copied. PCM16 fallback
is an experiment, not a documented guaranteed fix for this Xiaomi model.

## Local verification

- Native engine tests pass, including 770-frame PCM16/float equivalence, stereo
  channel separation, clipping, timeline recovery, FEC and lead-before-first-PCM.
- 34 Android release unit tests pass; release lint reports no errors.
- Release APK built for arm64-v8a, armeabi-v7a and x86_64 and verified with the
  existing RoomWave release certificate (v2/v3 signatures).
- Test artifact: `dist/android/RoomWave-xiaomi-test-2026-09-27.apk` with SHA256.
  Package version remains 0.1.1; identify this experiment by `audioPolicy` above.
  This is a local test build, not a GitHub release. Device audio and screen-off
  improvements remain unverified until the comparative run.
