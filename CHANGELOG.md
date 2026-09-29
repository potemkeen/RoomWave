# Changelog

Notable changes to RoomWave, newest first. Windows and Android are released
independently; version numbers do not need to match. Dates are GitHub publication
dates in UTC. Entries cover published releases, not every development commit.

## Unreleased

No unreleased application changes recorded yet.

## Windows 0.1.4 — 2026-09-29

[Download release](https://github.com/potemkeen/RoomWave/releases/tag/windows-v0.1.4)

- Consolidated receiver health into one stable-height summary per phone, with explanations and raw metrics in a collapsed section directly underneath.

- Added a receiver health summary above detailed diagnostics, using recent observations to explain recurring delivery, output and synchronization problems. Group-delay attribution is explicitly an estimate; raw metrics remain available.

- Attempt event-driven `IAudioClient3` capture at 240 frames / 5 ms, retaining the existing shared-event capture if initialization is unsupported or fails. Render endpoints require loopback support; the earlier render-only probes did not establish capture compatibility.
- Report the selected capture initialization path, requested/current engine period, actual WASAPI buffer and fallback reason in diagnostics. Removed the two startup-only period probes.
- The tested VB-CABLE loopback endpoint rejects the low-period stream flags; fallback retains the existing 10 ms capture period. No capture-latency reduction is claimed for that endpoint.
- Added frontend health-classification tests and formatting checks to Windows release CI.

## Windows 0.1.3 — 2026-09-27

[Download release](https://github.com/potemkeen/RoomWave/releases/tag/windows-v0.1.3)

### Fixed

- Settings and Diagnostics dialogs block background scrolling while keeping their own content scrollable.
- Settings close automatically after a successful save. Failed saves keep the dialog open.
- Updated the startup-budget test to use the current minimum instead of an obsolete fixed value.

### Changed

- Reduced the minimum playout budget from 80 ms to 30 ms. Adaptive receiver/group budgets still determine actual latency; this does not guarantee 30 ms playback.
- Refactored audio transport, packet routing and host UI components without intentional behavior changes.
- Updated public documentation, contribution guidance, licensing and third-party notices.

### Added

- WASAPI shared-period diagnostic probes. Production capture remains on the existing event-driven shared WASAPI path.

Windows 0.1.2 was not published: its release build stopped at an outdated test. Its application changes are included here.

## Android 0.1.2 — 2026-09-27

[Download release](https://github.com/potemkeen/RoomWave/releases/tag/android-v0.1.2)

### Changed

- Try additional native-rate float and PCM16 AAudio configurations when the existing configurations cannot provide low latency. Actual playback remains 48 kHz stereo; an existing fast path remains preferred.
- Request higher priority for the UDP receiver thread.
- Assemble diagnostic JSON at 4 Hz instead of 20 Hz while keeping audio-clock polling at 20 Hz.
- Update audio-output lead estimates even when no usable network PCM is available.

### Added

- Receiver timing and audio-configuration diagnostics for investigating delivery pauses and slow output paths.
- Optional keep-screen-on setting while connected and the app is visible, disabled by default.

### Known limitation

- Screen-off interruptions on the tested Xiaomi remain unresolved, including with battery restrictions disabled. These changes do not establish a latency improvement on that device.

Installs over release 0.1.1 and the signed Xiaomi test APK using the same signing key.

## Windows 0.1.1 — 2026-09-25

[Download release](https://github.com/potemkeen/RoomWave/releases/tag/windows-v0.1.1)

### Changed

- Detailed session logging is disabled by default. Diagnostics can start a ten-minute recording, stop it early and reveal the resulting file.
- Continuous logging is limited to a small, bounded error journal.

## Android 0.1.1 — 2026-09-25

[Download release](https://github.com/potemkeen/RoomWave/releases/tag/android-v0.1.1)

### Changed

- Removed periodic detailed metric dumps from Logcat. Error logging and telemetry needed by the host remain available.

## Android 0.1.0 — 2026-09-25

[Download release](https://github.com/potemkeen/RoomWave/releases/tag/android-v0.1.0)

### Added

- First signed Android release APK for use with the Windows host, supporting Android 8.0 and later.
- A single APK for arm64-v8a, armeabi-v7a and x86_64, with a SHA256 checksum and automated release builds.
- A permanent release signing key for subsequent in-place updates. Earlier debug builds require uninstalling before switching to a release APK.

## Windows 0.1.0 — 2026-09-24

[Download release](https://github.com/potemkeen/RoomWave/releases/tag/windows-v0.1.0)

### Added

- First Windows x64 installer release and automated GitHub release builds with SHA256 checksums.
- Bundled signed VB-CABLE and automatic 7.1 / 48 kHz / loopback configuration, removing Voicemeeter from the installation workflow.
- WebView2 installation when needed. Windows test-signing mode is not required; the RoomWave application and installer themselves are not publisher-signed.
