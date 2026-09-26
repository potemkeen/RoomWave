# Build and development

The commands below are run in PowerShell from the repository root. Development tools are not required by the finished application: the Windows installer is available in [Releases](https://github.com/potemkeen/RoomWave/releases/latest).

## Windows

Node.js 22, Rust with the MSVC toolchain, Microsoft C++ Build Tools (Desktop development with C++, Windows SDK), and WebView2 are required. CI environment versions are pinned in the [release instructions](windows-releases.md).

```powershell
cd windows-host
npm.cmd ci
npm.cmd run build:host
```

Built host: `windows-host/src-tauri/target/release/roomwave-host.exe`.

For development, use `npm.cmd run tauri -- dev`. The `npm.cmd run build` command checks TypeScript and builds only the frontend. For a standalone EXE, use the full Tauri build rather than a separate `cargo build --release`.

Tests from `windows-host/src-tauri`:

```powershell
cargo test --locked
```

Tests requiring real audio hardware or networking are marked `ignored`; run them separately in a prepared environment. Regular unit tests do not verify audio on physical devices.

To build the installer from the repository root:

```powershell
./windows-installer/Prepare-BuildTools.ps1
./windows-installer/Build-Installer.ps1
```

See the [installer documentation](../windows-installer/README.md) for details and dependencies.

## Android

Open `android-receiver` in Android Studio. JDK 17 or 21, Android SDK 35, NDK 28.2.13676358 and CMake 3.22.1 are required. Gradle is provided by the repository; no separate installation is needed. The SDK and native components can be installed through SDK Manager.

```powershell
cd android-receiver
./gradlew.bat testDebugUnitTest assembleDebug
```

Result: `android-receiver/app/build/outputs/apk/debug/app-debug.apk`. This is a debug build for development. Signed APKs are published separately through the [Android release workflow](android-releases.md).

For USB installation, enable debugging on the phone and authorize the computer:

```powershell
adb devices -l
adb install -r ./app/build/outputs/apk/debug/app-debug.apk
```

With multiple phones, use `adb -s <serial> install -r …`. USB is needed for installation and debugging; audio is transmitted over the local network. The application can also be installed using the Run button in Android Studio.

If Gradle cannot find the SDK/JDK, configure `ANDROID_HOME` and `JAVA_HOME`, or configure the SDK and Gradle JDK in Android Studio. Do not add local paths from `local.properties` to git.

## Structure

- `windows-host/` — Tauri/React interface and Rust host.
- `android-receiver/` — Android application using Kotlin/Compose and native audio output.
- `windows-installer/` — application installation and virtual device configuration.
- `docs/` — technical documentation and plans.

Host platform separation and further steps are described in the [macOS plan](macos-host-plan.md). There is currently no working macOS Host version.
