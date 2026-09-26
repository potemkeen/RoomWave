# RoomWave Host implementation plan for macOS

Status as of 24.09.2026: architectural preparation, **not a working macOS version**. The target device for the first test is a MacBook Air M1; the macOS version is not yet known. This stage does not install drivers on the Mac and does not change the Android/network protocol.

## What has already been separated

All paths below are relative to `windows-host/src-tauri/src`.

```text
main.rs                         Tauri commands and application startup
  audio.rs                      sessions, Hub, AudioBlock, routing, UDP/FEC/repair, admission
  audio_types.rs                shared local-output Config / State / Endpoint
  layout.rs                     logical channels, mask→index, test generator
  timing.rs                     clock-offset estimation; Clock is imported from platform
  discovery.rs                  LAN discovery
  diagnostics.rs                log recording
  fec.rs / transport_stats.rs   recovery and statistics

  platform/mod.rs               system implementation selection via cfg(target_os)
    windows/
      clock.rs                  QPC, nanoseconds on the WASAPI time scale
      endpoint.rs               WAVEFORMATEXTENSIBLE parsing and real channel mask
      paths.rs                  settings and log paths
      default_output.rs         default device / guardian / VB-CABLE installation
      local_test.rs             test through a physical WASAPI device
      windows_audio.rs          engine-period probes / MMCSS
      virtual_probe.rs          Windows hardware checks

  audio_backend/mod.rs          audio worker selection via cfg(target_os)
    windows/
      capture.rs                WASAPI capture, endpoint monitoring, PCM packetization
      local_output.rs           WASAPI render and existing local DSP
```

`audio_backend` is a child module of `audio`, so it can access the private Hub without turning internal state into a public API. The platform is selected at compile time: no additional dynamic dispatch/allocations were introduced into the audio hot path. System-module aliases remain in `main.rs` so that all call sites and user-facing commands do not need to change at once.

`wasapi`, `windows`, `windows-sys`, and `windows-core` were moved into target-specific dependencies in `Cargo.toml`. JSON field and configuration-file names were preserved, including the historical `windowsOutputs`: renaming it requires a coordinated UI migration and must not be a side effect of the port.

For unsupported operating systems, `platform/mod.rs` emits an explicit `compile_error` pointing to this plan. Stubs that successfully start and pretend to play audio were not added. The restriction should be removed only after a real macOS backend is connected. A full application build on macOS is not currently supported; moving dependencies alone does not provide it.

## Contracts to preserve

1. `AudioBlock`: 240 frames (5 ms) at 48 kHz, up to 8 logical channels, interleaved signed PCM16. `source` stores separate channels, while `pcm` stores the existing compatible front-stereo representation. UDP headers, FEC, frame index and protocolVersion 4 are preserved.
2. `mask` describes **our canonical channel order**. Core Audio labels/order must be converted into this order BEFORE publishing AudioBlock. Do not assume that the first 8 channels of any 16-channel device automatically correspond to our 7.1, or that a Core Audio layout tag equals a Windows speaker mask. Distinguish 5.1 Back from 5.1 Side; reject unknown layouts explicitly.
3. `Clock::now()` returns i64 nanoseconds on one monotonic scale for capture, sender and local render. On Mac, use Core Audio host time/mach time and convert through the timebase. Do not mix this scale with UNIX time, arbitrary Instant values or UDP arrival time. After sleep/wake, verify anchor validity and refresh synchronization.
4. `capture_ns` is the source timestamp, `read_ns` is the moment PCM is received by the application, `published_ns` is publication time, and `play_ns` is the shared deadline. Do not replace missing driver timestamps with arrival time and do not present such an estimate as acoustic latency.
5. The backend preserves stop/reopen behavior, bounded queues, slow-subscriber isolation and the current late-join preparation. Changing the operating system must not change the rate of the shared timeline or packet-loss behavior.
6. Local output receives assigned channels through the same Hub. Its own output must not be captured. Clock-drift correction, fade and limiter are preserved; initial local-only playback does not wait for the network margin. Physical output remains explicitly separate from the capture source.
7. The default-output controller provides the same prepare/snapshot/activate/cancel/stop operations. It changes the device only after capture is ready; it restores the previous one only if the current selection still belongs to RoomWave. Do not override a user's manual selection. Guardian/journal behavior is required for abnormal termination as well.

## Next steps

### 1. Prepare the Mac and port branch

- Determine the macOS version, available physical outputs (built-in, USB, HDMI) and whether testing with two Android devices is possible.
- Install Xcode Command Line Tools, Rust and Node.js; build the `aarch64-apple-darwin` target. Intel/universal binary support is a later stage and is not required for the M1 PoC.
- Open the same repository on the Mac. Build the Windows version separately whenever shared code changes.
- In Tauri, replace Windows-only `npm.cmd` usage with appropriate platform-specific build commands, add the macOS bundle/icon/Info.plist and the required purpose strings/permissions for the selected capture API. Do not port registry/TESTSIGNING/MMCSS concepts to macOS.
- Add `platform/macos` and `audio_backend/macos` with cfg-based selection. Obtain settings/log paths through macOS application directories, without LOCALAPPDATA.

### 2. Small capture experiment before the full backend

Test two options using the same multichannel test:

**Core Audio process taps:** determine whether system audio can be captured without a virtual cable. Verify API availability on the installed macOS version, user permission, actual sample format/channel layout, exclusion of the RoomWave process, muting the original output and restoring it. Critically, prove that independent 5.1/7.1 channels are actually captured. If the system endpoint is already stereo and the application performed the downmix earlier, the tap cannot restore the lost channels.

**BlackHole 16ch for the PoC:** multichannel virtual device → Core Audio input → RoomWave. Configure 48 kHz and an explicit logical layout; verify actual channels in the player/application, not only the ability to open a 16-channel stream. Initially use the original driver installed separately. Bundling/renaming it in the installer comes only after verifying licensing and signing requirements. This is a candidate, not yet a selected dependency.

Do not use ScreenCaptureKit as the foundation for full 7.1: its documented audio capture is mono/stereo. Consider a custom Audio Server Driver Plug-in separately if neither option satisfies product requirements. Apple recommends this mechanism for virtual devices; do not automatically choose AudioDriverKit merely because it contains the word “driver”.

The result of this stage should be a table covering API/permissions/channel mapping/buffer/latency and one selected source. Do not promise users that no virtual device is required before this is established.

### 3. Further extract shared processing

The current separation follows working responsibilities rather than splitting every line:

- `audio_backend/windows/capture.rs` still contains packetization, timestamp deque handling, silence catch-up, the test signal and shared-delay adaptation. Extract these into a shared packetizer with “PCM + format + host timestamps” as input. WASAPI and Core Audio should feed one packetizer rather than duplicating the algorithm.
- Move pure matrix/interpolation/limiter logic and its unit tests out of `windows/local_output.rs` into shared DSP. Windows and Mac should retain only device APIs, callbacks/padding/clock and lifecycle handling.
- Shared input/output worker interfaces are currently compile-time modules (`capture_audio`, `local_output::worker/endpoints/config_path`). Refine the backend interface after the capture PoC. Do not introduce a universal trait that hides required timestamps/periods or forces an allocation on every callback.
- If necessary, move shared modules into a Rust library for testing without Tauri/audio hardware. Preserve the existing Windows unit tests first.

### 4. Core Audio backend

- Use an IOProc/AudioUnit callback with a short realtime path, preallocated buffers and bounded SPSC between the callback and shared worker.
- Do not perform JSON work, networking, UI work, COM-like device queries, disk I/O, mutex waits or large-object allocation inside the callback.
- Accept Core Audio float32/interleaved/non-interleaved formats and convert them explicitly into the shared format. Preserve 48 kHz where possible and resample only when sample rates actually differ. Distinguish float→PCM16 format conversion from sample-rate conversion.
- Request a small supported buffer frame size, then read the actual size, safety offset, device/stream latency and host timestamps. 128 frames ≈ 2.67 ms and 256 ≈ 5.33 ms are buffer durations, not E2E guarantees.
- Record queue fill/age, overruns, missing frames, callback duration and capture→publish. Choose the buffer based on stability, not only the minimum exposed by the API.
- Output local channels to an explicitly selected device through the shared timeline; evaluate Bluetooth separately from built-in/wired output.

### 5. Lifecycle and interface

- Identify devices using a stable UID, not the display name or temporary AudioDeviceID.
- Refresh the device list when HDMI/USB is connected or the default device changes; reread layout/rate on changes.
- Properly stop/reopen the stream after sleep/wake. Do not send stale backlog with invalid deadlines.
- Preserve the channel/diagnostics UI. Remove platform-inappropriate VB-CABLE/Windows text from the macOS build; display capabilities of the real backend rather than pretending unsupported features exist.
- Permission denial/revocation should leave normal Mac audio working and display a clear message. Do not attempt to bypass system permissions.

### 6. Acceptance tests on M1

1. Mono/stereo/5.1 Back/5.1 Side/7.1: every test channel reaches only the assigned endpoint; all others remain silent.
2. Ordinary stereo content on the Mac, then real multichannel content; compare with a standalone generator.
3. Local-only, one Android, two Android devices, late joining and disconnecting any output. Verify synchronization and no tempo change/clicks on already running outputs.
4. Pause/resume, silence, source/sample-rate change, sleep/wake, physical output disconnection, Wi-Fi loss.
5. Normal exit, crash, manual system-output change; the original endpoint is restored correctly.
6. Several minutes under load: RTT/jitter/loss/repaired/underrun, queue fill/age, hardware buffer, estimated latency. If possible, perform a separate acoustic/electrical E2E test. Comparison with Windows requires the same network/phones/screen states.
7. Reinstall/update, first launch for a clean user, permissions denied/granted again.

### 7. Distribution after a working PoC

Developer ID signing/notarization for the application; when using a virtual device, a separately verified HAL plug-in installation, package signing, licensing and update/uninstall behavior without weakening macOS security. Start with an Apple Silicon package, then add universal support if required. Development on a personal Mac and public distribution are separate readiness checks.

## Verified in this stage

Windows unit tests after the refactor: 28 passed, 9 hardware/network tests ignored. Release still builds separately through the existing `npm run build:host`. Audio algorithms, wire format and default-device policy were not changed during the refactor. No physical macOS audio test has been performed yet; absence of a Windows latency regression has not been remeasured by this refactor.

## Primary sources

- Core Audio taps: https://developer.apple.com/documentation/coreaudio/capturing-system-audio-with-core-audio-taps
- ScreenCaptureKit channel limit: https://developer.apple.com/documentation/screencapturekit/scstreamconfiguration/channelcount
- Virtual devices / Audio Server Driver Plug-in recommendation: https://developer.apple.com/documentation/audiodriverkit/creating-an-audio-device-driver
- HAL plug-in example: https://developer.apple.com/documentation/coreaudio/creating-an-audio-server-driver-plug-in
- BlackHole, channels and licensing: https://github.com/ExistentialAudio/BlackHole
- Tauri macOS signing: https://tauri.app/distribute/sign/macos/
