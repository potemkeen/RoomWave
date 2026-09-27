RoomWave Windows Host 0.1.2. The Android APK is released separately.

- Settings and Diagnostics dialogs now block scrolling of the main window while retaining their own scrolling.
- Settings close automatically after a successful save; failed saves keep the dialog open.
- Minimum playout budget reduced to 30 ms. Adaptive device/group budgets can still raise actual latency; this is not a promise of 30 ms playback.
- Added WASAPI shared-period diagnostic probes. Production capture is unchanged.
- Internal host/UI refactoring and updated documentation.

Detailed session logging remains opt-in in Diagnostics.

### Installation

1. Download `RoomWave-Setup-…-x64.exe` from the Assets below.
2. Close any running RoomWave instance and start the installer; approve the administrator prompt.
3. If the installer asks for a restart, restart Windows and then open RoomWave from the Start menu.

Windows 10 2004+ / Windows 11, x64, is required. WebView2 is downloaded automatically when required; an internet connection is needed for this.

The package includes signed VB-CABLE Pack45 by [VB-Audio](https://vb-audio.com/Cable/). It is automatically configured for 7.1 / 48 kHz / loopback. VB-CABLE is donationware; see the [author's terms and support information](https://vb-audio.com/Services/licensing.htm). An already installed standard VB-CABLE device also receives this profile; its previous settings are preserved in `%ProgramData%\RoomWave\Setup`.

RoomWave itself and the installer are not yet publisher-signed, so Windows may display a SmartScreen warning. Windows test-signing mode is not required for VB-CABLE. A SHA256 checksum is published next to the EXE.

When RoomWave is uninstalled, VB-CABLE, WebView2 and user settings remain on the PC. The build passed automated host tests; real-audio verification requires physical devices.