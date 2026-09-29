RoomWave Windows Host 0.1.4. Compatible with the current Android 0.1.2 release; no Android update is required.

- Diagnostics now explains recurring network, audio-output and synchronization issues for each phone, using recent observations rather than lifetime counters or isolated spikes.
- One stable-height summary per phone, with explanations, recommendations and raw metrics directly underneath in a collapsed section. Expanded sections stay open when metrics refresh.
- Group-delay attribution is explicitly an estimate; missing or stale data is shown as insufficient for assessment.
- Capture attempts IAudioClient3 at 240 frames / 5 ms, with automatic fallback to the previous shared-event path. Diagnostics reports the selected path, actual period/buffer and fallback reason.
- On the tested VB-CABLE loopback endpoint, Windows rejects the low-period loopback flags and the existing 10 ms capture period remains in use. This release does not claim reduced capture latency on that endpoint.
- Added automated frontend health-classification tests to the release workflow.

Detailed session logging remains opt-in in Diagnostics.

### Installation

1. Download `RoomWave-Setup-…-x64.exe` from the Assets below.
2. Close any running RoomWave instance and start the installer; approve the administrator prompt.
3. If the installer asks for a restart, restart Windows and then open RoomWave from the Start menu.

Windows 10 2004+ / Windows 11, x64, is required. WebView2 is downloaded automatically when required; an internet connection is needed for this.

The package includes signed VB-CABLE Pack45 by [VB-Audio](https://vb-audio.com/Cable/). It is automatically configured for 7.1 / 48 kHz / loopback. VB-CABLE is donationware; see the [author's terms and support information](https://vb-audio.com/Services/licensing.htm). An already installed standard VB-CABLE device also receives this profile; its previous settings are preserved in `%ProgramData%\RoomWave\Setup`.

RoomWave itself and the installer are not yet publisher-signed, so Windows may display a SmartScreen warning. Windows test-signing mode is not required for VB-CABLE. A SHA256 checksum is published next to the EXE.

When RoomWave is uninstalled, VB-CABLE, WebView2 and user settings remain on the PC. The build passed automated host tests; real-audio verification requires physical devices.