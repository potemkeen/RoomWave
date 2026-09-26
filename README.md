# RoomWave

RoomWave streams system audio from a Windows computer to Android phones over the local network. You can connect multiple phones, assign each one a separate audio channel, and keep some channels on the PC speakers or headphones.

**[Download for Windows](https://github.com/potemkeen/RoomWave/releases/latest)** · **[Download for Android](https://github.com/potemkeen/RoomWave/releases/tag/android-v0.1.1)** · [Build from source](docs/development.md)

## Features

- Automatic phone discovery without entering IP addresses.
- Synchronized playback across multiple devices.
- Stereo, 5.1 and 7.1 with channel selection for the PC and each phone.
- Audio transmission without lossy compression and background playback on Android.
- Channel testing with a test signal; technical metrics are available in a separate diagnostics view.

For example: the front channels and center play through the PC speakers, while two phones play the left and right rear channels. This requires content with a multichannel audio track: ordinary stereo does not become 5.1 on its own.

## Installation

**Windows:** download `RoomWave-Setup-…-x64.exe` from [Releases](https://github.com/potemkeen/RoomWave/releases/latest) and run it. Windows 10 version 2004 or later / Windows 11, x64, is required.

The installer automatically installs and configures the VB-CABLE virtual audio device. Voicemeeter is not required. If WebView2 is needed, it will be downloaded automatically. Restart Windows if the installer asks you to.

**Android:** download the APK from the [Android release](https://github.com/potemkeen/RoomWave/releases/tag/android-v0.1.1) to a phone running Android 8.0 or later. Open the file and, if prompted, allow app installation for the browser or file manager. When switching from an older debug build, uninstall it once before installing the release because the signatures are different. Subsequent release APKs install as updates.

> The RoomWave installer is not yet publisher-signed, so Windows may display a SmartScreen warning. The package includes the signed VB-CABLE driver; Windows test-signing mode is not required for it.

## How to use

1. Connect the PC and phones to the same local network and open RoomWave on the phones.
2. Start RoomWave on the PC, connect the discovered phones, and select which channels they should play.
3. For PC audio, select real headphones or speakers in the audio settings and assign channels to them.
4. Start music or video playback. The “Sound Test” tab helps verify the assignments.

When RoomWave starts, it automatically switches system audio to the virtual device. When it closes, it restores the previous output unless you changed it manually. The channel assigned to a phone is played through all available speakers on that phone.

## If something does not work

- **Phone not found:** make sure the devices are on the same network without client isolation. Allow RoomWave access to the private network in Windows Firewall.
- **Audio interruptions:** try 5 GHz Wi-Fi closer to the access point and allow the Android app to run in the background. Power-saving features on some phones may interfere with audio when the screen is off.
- **Need more details:** open “Diagnostics” in the app. Detailed recording can be enabled with the “Record session” button for 10 minutes. Files are stored in `%LOCALAPPDATA%\RoomWave\logs`; by default, only errors are recorded.

Latency depends on the network and audio devices. RoomWave is designed for trusted local networks; transmission is not encrypted.

## Documentation

- [Architecture](docs/architecture.md)
- [Build and development](docs/development.md)
- [Installer and VB-CABLE configuration](windows-installer/README.md)
- [How to publish Windows releases](docs/windows-releases.md) and [Android APKs](docs/android-releases.md)
- [macOS version plan](docs/macos-host-plan.md) — not implemented yet.

VB-CABLE is developed by [VB-Audio](https://vb-audio.com/Cable/) and distributed as donationware. [Terms of use and author support](https://vb-audio.com/Services/licensing.htm).

## License

RoomWave source code is distributed under the [MIT](LICENSE) license.

RoomWave uses third-party components distributed under their own license terms. In particular, the Windows version uses VB-CABLE by VB-Audio Software.

See [Third-Party Software Notices](THIRD_PARTY_NOTICES.md) for details.