# RoomWave architecture

RoomWave consists of a Windows Host and mobile clients. The host captures multichannel system audio, routes individual channels between the local audio output and connected phones, and maintains a shared synchronized playback timeline.

No external server, account or internet connection is required. Devices communicate directly over the local network.

## Overview

```text
Windows audio source
        ↓
Virtual multichannel endpoint / capture source
        ↓
RoomWave Host
        ↓
Shared audio timeline
        ├── Local Windows output
        ├── Android receiver #1
        ├── Android receiver #2
        └── ...
```

The host captures one multichannel PCM stream and creates a shared `AudioBlock` stream from it. No additional capture is performed for individual devices.

Each phone can be assigned a separate speaker channel. The local PC output can be assigned multiple channels with matrix mixing according to the physical device configuration.

## Windows Host

The Windows Host is implemented with Tauri 2, React and Rust.

React handles the user interface. Discovery, networking, synchronization, audio processing and realtime logic run in Rust.

### Device discovery

`discovery.rs` uses mDNS/DNS-SD to discover RoomWave clients on the local network.

Devices are identified by a persistent `deviceId`, not by IP address or mDNS instance name.

The UI periodically receives prepared state snapshots through Tauri IPC. Network discovery does not run in React.

### Audio capture

The Windows platform implementation is located in `audio_backend/windows`.

RoomWave uses event-driven WASAPI capture.

Primary internal format:

- 48 kHz;
- PCM;
- 5 ms audio blocks;
- the original `WAVEFORMATEXTENSIBLE` channel mask is preserved.

RoomWave does not determine channel positions from channel count alone. The Windows speaker mask is used for the multichannel layout.

Supported layouts include:

- Stereo;
- Quad;
- 5.1 Back;
- 5.1 Side;
- 7.1;
- 7.1 Wide.

When the capture endpoint or its format changes, the capture worker is reinitialized while the global playback timeline is preserved.

### Audio routing

One captured multichannel `AudioBlock` is published into the shared subscriber pipeline.

Each receiver routes only its assigned channels.

For a phone, the selected speaker channel is transmitted as mono PCM and played through the phone's available audio output.

Multiple speaker channels can be selected for the local Windows output.

If the physical output does not provide the corresponding multichannel layout, RoomWave applies limited matrix mixing. For example, Center can be added to both stereo front channels.

RoomWave does not synthesize missing surround channels from stereo.

### Local output

The Windows Host can simultaneously:

- capture a multichannel source;
- send selected channels to phones;
- play other channels through physical PC speakers or headphones.

The capture source and local output must be different endpoints.

The local renderer uses event-driven WASAPI and participates in the shared playback schedule together with mobile clients.

Limited rate correction compensates for small differences between hardware clocks.

### Virtual audio endpoint

The current Windows version uses VB-CABLE as a virtual multichannel render endpoint.

The RoomWave installer can install and configure VB-CABLE automatically.

RoomWave is not the developer of VB-CABLE. The driver is distributed by VB-Audio under its own terms.

## Synchronization

All outputs operate relative to a shared host timeline.

Windows uses `QueryPerformanceCounter`; Android uses `System.nanoTime`.

During the control handshake, the host and client exchange timestamps and estimate:

- clock offset;
- RTT;
- synchronization uncertainty.

Fresh measurements with the lowest RTT are used to estimate the offset in order to reduce the influence of network queues.

This model does not assume a perfectly symmetric network and is not an absolute measurement of physical acoustic latency.

Each audio block contains timing information that allows clients to play audio relative to the shared presentation timeline.

## Network transport

The control channel runs over TCP.

Audio is transmitted over UDP in short PCM packets.

The protocol supports:

- timestamps;
- frame indices;
- mono/stereo routed PCM;
- retransmission;
- XOR FEC;
- playback deadlines;
- transport diagnostics.

Additional recovery mechanisms must not independently modify the playback timeline.

If a packet cannot be recovered before its playback deadline, the client continues playback without waiting for the late packet.

The wire format is described in [protocol.md](protocol.md).

## Android Receiver

The Android client is implemented with Kotlin/Compose and a native audio backend.

`AudioReceiverService` runs as a foreground service and continues receiving audio when the Activity is in the background and the screen is off.

The service manages:

- NSD advertisement;
- TCP control connection;
- UDP audio receiver;
- synchronization;
- audio session;
- WakeLock;
- Wi-Fi lock;
- diagnostics.

### Packet processing

Received UDP packets are validated for:

- protocol/session;
- sender;
- sequence/frame index;
- payload format;
- playback deadline.

The client maintains a bounded reorder/repair pipeline.

Late packets must not increase overall latency and are not played after the corresponding playback deadline has passed.

### Audio output

The primary playback backend uses AAudio.

PCM is passed to the native audio layer through bounded buffers.

The Android client tracks:

- actual audio device;
- hardware buffer;
- underruns;
- output timing;
- estimated presentation time.

Depending on the available system APIs, the client uses the most accurate timestamps available on the specific device.

## Multi-device playback

Multiple mobile clients can be connected to one Windows Host at the same time.

Each client has:

- its own network connection;
- its own clock synchronization;
- its own transport statistics;
- its own audio output delay.

All outputs remain tied to the same host presentation timeline.

Delay budget changes are coordinated so synchronization between devices is not disrupted.

Connecting a new client includes a preparation and synchronization stage before PCM publication begins for that receiver.

## Diagnostics

RoomWave collects diagnostic metrics separately from the realtime audio path.

On Windows, the tracked metrics include:

- WASAPI engine period;
- capture buffer;
- capture/read processing;
- queue depth;
- queue drops;
- local render metrics.

For each mobile client, available metrics include:

- RTT;
- estimated latency;
- synchronization error;
- jitter;
- packet loss;
- FEC/retransmission statistics;
- underruns;
- audio output buffer;
- device/network state.

Detailed diagnostics can be recorded into a JSONL session log.

PCM is not recorded for diagnostics.

See [diagnostic-logging.md](diagnostic-logging.md) for details.

## Platform abstraction

Shared RoomWave logic is separated from platform-specific implementations.

Shared components include:

- network protocol;
- session management;
- synchronization;
- routing;
- FEC;
- diagnostics;
- common audio timeline.

Platform-specific components include:

- system audio capture;
- physical audio output;
- audio endpoint management;
- default output switching;
- virtual audio integration.

The Windows implementation is located in `audio_backend/windows` and `platform/windows`.

This boundary is used as the foundation for the future macOS Host.

## Current limitations

- The Windows Host currently supports Windows x64 only.
- The mobile client is currently implemented only for Android.
- The iOS client is not implemented yet.
- The macOS Host is not implemented yet.
- The connection is designed for trusted local networks.
- Control/audio traffic does not provide full end-to-end encryption.
- Pairing/authentication is not implemented yet.
- Full acoustic end-to-end latency is not measured: software metrics do not include every driver and physical speaker delay.
- Stability and latency may depend on the specific Android device, Wi-Fi network and audio route.
