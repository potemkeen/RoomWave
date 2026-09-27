RoomWave Android receiver 0.1.2, compatible with Windows Host 0.1.1 and 0.1.3.

- Additional native-rate/PCM16 AAudio candidates when the existing output cannot provide low latency; PCM remains 48 kHz.
- Higher UDP worker priority, less frequent diagnostic JSON construction, and output-lead updates even during missing audio.
- More targeted receiver timing diagnostics and an optional keep-screen-on setting during connection.
- Existing fast output remains preferred. These changes did not resolve screen-off interruptions on the tested Xiaomi; no latency improvement is promised for that model.

Signed with the existing release key; installs over release 0.1.1 and the signed Xiaomi test APK. Detailed host logging remains opt-in.

1. Download `RoomWave-…-android.apk` from Assets to a phone running Android 8.0 or later.
2. Open the file. If Android asks, allow app installation for the browser or file manager used to open the APK.
3. Start RoomWave, connect the phone and PC to the same local network, and connect the phone in the PC application.

[Windows Host installer](https://github.com/potemkeen/RoomWave/releases/tag/windows-v0.1.3).

**Switching from a debug build:** first uninstall the previous test version of RoomWave from the phone. It uses a different signature, so the release cannot be installed over it. Local settings will be reset during uninstall; you may need to assign the channel again on the PC. Future release APKs are signed with the same permanent key and install as updates.

A single APK contains builds for arm64-v8a, armeabi-v7a and x86_64. A SHA256 checksum is provided separately. Google Play publishing is not configured yet.