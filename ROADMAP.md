# RoomWave Roadmap

This document contains the current high-level development priorities for RoomWave.

The roadmap may change as the project evolves.

## Windows audio

- Reduce latency in the VB-CABLE capture pipeline.
- Evaluate `IAudioClient3` and smaller shared-mode engine periods.
- Reduce unnecessary buffering, copying and locking in realtime audio paths.
- Improve latency, underrun, overrun and queue diagnostics.
- Continue automatic VB-CABLE installation and configuration improvements.

## Client experience

- Improve the initial connection flow on mobile clients.
- Show clear connection states:
    - searching for host;
    - host found;
    - connecting;
    - synchronizing;
    - ready;
    - connection error.
- Improve reconnect behavior and error reporting.

## macOS

- Implement a RoomWave host for macOS.
- Research and select a suitable virtual multichannel audio device.
- Preserve the same channel-routing model as the Windows host where possible.

See [macOS host plan](docs/macos-host-plan.md).

## iOS

- Implement a native RoomWave client for iOS.
- Support host discovery on the local network.
- Support synchronized audio playback.
- Preserve channel assignment and reconnect behavior available on Android.

## Diagnostics

- Replace raw technical output with clearer user-facing diagnostics.
- Keep detailed technical metrics available for troubleshooting.
- Improve measurements for:
    - audio queue depth;
    - effective latency;
    - packet loss;
    - underruns and overruns;
    - synchronization quality.

## Longer term

- Improve cross-platform support.
- Reduce setup complexity.
- Improve resilience on unstable Wi-Fi networks.
- Continue reducing end-to-end latency without sacrificing synchronization or playback stability.