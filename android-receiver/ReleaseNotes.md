RoomWave Android receiver 0.1.1, compatible with Windows Host 0.1.0 and 0.1.1.

Periodic logging of detailed metrics to Logcat has been removed. Error logging and metric reporting to the host are preserved. The update installs over the 0.1.0 release APK.

1. Download `RoomWave-…-android.apk` from Assets to a phone running Android 8.0 or later.
2. Open the file. If Android asks, allow app installation for the browser or file manager used to open the APK.
3. Start RoomWave, connect the phone and PC to the same local network, and connect the phone in the PC application.

[Windows Host installer](https://github.com/potemkeen/RoomWave/releases/tag/windows-v0.1.1).

**Switching from a debug build:** first uninstall the previous test version of RoomWave from the phone. It uses a different signature, so the release cannot be installed over it. Local settings will be reset during uninstall; you may need to assign the channel again on the PC. Future release APKs are signed with the same permanent key and install as updates.

A single APK contains builds for arm64-v8a, armeabi-v7a and x86_64. A SHA256 checksum is provided separately. Google Play publishing is not configured yet.