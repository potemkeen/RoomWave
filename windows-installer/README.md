# RoomWave Windows installer

Graphical Inno Setup installer: `dist/windows/RoomWave-Setup-0.1.0-x64.exe`. The version is read from `windows-host/src-tauri/tauri.conf.json`. GitHub Actions and Releases publishing are not configured at this stage.

## User flow

1. Close RoomWave, run the EXE and allow administrator installation.
2. The installer checks for WebView2 and installs it if missing (internet access is required).
3. It finds a standard VB-CABLE installation or installs the original signed Pack45. Users do not need WDK/DevCon.
4. It configures loopback, internal SR 48 kHz, latency 3072; render Speakers — 7.1/48 kHz (mask 0x63f). Original settings are saved and the resulting values are verified.
5. It installs the host into `Program Files\RoomWave`, creates a Start menu shortcut and an entry in Installed Apps.
6. On first driver installation/profile change, it prompts for a restart. Starting the host from the finish page is available only when no restart is required and is performed with the original user's privileges.

Windows 10 version 2004+ and Windows 11, x64, are supported. ARM64 and 32-bit are not included in this package. The host and installer are not yet signed with a publisher certificate, so Windows may display a warning. The VB-CABLE driver retains its original Microsoft signature; Windows test-signing mode is not required.

## Updating and uninstalling

AppId is fixed across versions, so updates install into the same directory. If the host is running during installation, it must be closed; the script gives the guardian time to restore the original output. If the existing VB-CABLE version differs from 3.3.1.7 or multiple devices are detected, installation stops with an error instead of modifying an unknown driver.

Uninstall is available from Windows Settings and the Start menu. Installer-owned files and shortcuts are removed. VB-CABLE, WebView2, user assignments/logs and backups are preserved. The driver may be used by other applications. The installer does not modify the old RoomWaveAudio PoC driver, TESTSIGNING, certificates or the protected audio process.

## Build

Requires Node/npm, Rust/toolchain for Windows x64, Inno Setup 6.7.3, an unpacked official VB-CABLE Pack45, and the official WebView2 Evergreen Bootstrapper. Tools can be stored outside the repository and their paths passed explicitly:

```powershell
.\windows-installer\Build-Installer.ps1 `
  -VBCablePackage C:\build-tools\vbcable-pack45 `
  -IsccPath 'C:\Program Files (x86)\Inno Setup 6\ISCC.exe' `
  -WebViewBootstrapper C:\build-tools\MicrosoftEdgeWebview2Setup.exe
```

By default, the script builds the host first. Use `-SkipHostBuild` only after a successful build of the current host. Local defaults point to `.build-tmp/vbcable/package` and `.build-tmp/installer-tools`. The script verifies dependency signatures and the INF version, builds the EXE and writes a `.sha256` file next to it. Built binaries and dependencies are excluded from git through `dist/.build-tmp`; installer sources are included in the repository.

To obtain the tools:

- Inno Setup: https://jrsoftware.org/isdl.php (6.7.3 at this stage; signed by Pyrsys B.V.). Take the compiler's commercial-use license into account.
- VB-CABLE: https://vb-audio.com/Cable/ (standard Pack45, not A/B/C/D).
- WebView2 Bootstrapper: https://go.microsoft.com/fwlink/p/?LinkId=2124703 (signed by Microsoft).

## Implementation

- `RoomWave.iss`: wizard, prerequisites, file/uninstall registration, restart request.
- `Build-Installer.ps1`: host and installer build, SHA256.
- `Package-RoomWave.ps1`: payload preparation; the full unchanged vendor package is included with its readme.
- `Install-RoomWave.ps1`: version check, snapshots, device installation, configuration, restoring the previous render default after automatic switching by the driver.
- `DriverSetup.cs`: SetupAPI creates a root devnode; NewDev installs the signed INF. On failure it removes only the devnode created by that invocation. It does not reinstall an existing driver. The source is compiled using system PowerShell/.NET; WDK is not distributed.
- `Configure-VBCable.ps1`: standard Pack45 profile, writes/verifies Registry64 and Registry32 under `HKLM\SOFTWARE\VB-Audio\Cable`. Previous unsuccessful experiments with the MultiCable branch are not carried over.
- `SetupInfo.txt`: installation terms and VB-Audio/donationware attribution.

If the endpoint does not appear after 15 seconds, installation does not report success: a restart and another installer run are required. Automatic continuation after this error is not implemented yet. External prerequisites (driver/runtime) are not removed if installation is cancelled. If the profile was written only partially, the original values remain in the backup; the result file reports an error.

Registry verification is not equivalent to verifying a real audio signal. `loopbackAudioVerified=false` in the report intentionally means that an audio test is still required after restart. The installer does not play test signals over the user's music.

## Logs and checks

Inno Setup writes its standard setup log; `/LOG="C:\path\setup.log"` can be specified. Audio configuration writes `%ProgramData%\RoomWave\Setup\install.log`, `result.json`, `error.txt`, snapshots of endpoints/defaults and changed registry values. An empty `error.txt` after a successful run does not indicate an error.

Verified here: Inno build, C# helper compilation, installation over an existing VB-CABLE and profile readback. Uninstall and reinstall completed with exit code 0; the driver and its profile were preserved. The installed EXE matches the release build by SHA256. Installation of a new root device, missing WebView2/runtime download and restart prompting require separate testing on a clean Windows installation/VM. Testing an existing driver must not be treated as testing a new installation.

Silent installation for future CI/testing: `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /LOG="..."`. The flags matter: without `/NORESTART`, silent installer behavior regarding restart is different. In interactive mode, the wizard offers a choice. See https://jrsoftware.org/ishelp/topic_setupcmdline.htm .

## Before a public Release

- Clean Windows: first install → reboot → 8-channel audio, update, uninstall, denied permissions, unknown VB-CABLE version.
- Sign RoomWave/Setup with a trusted publisher certificate when available.
- Preserve VB-CABLE vendor visibility and donationware attribution; separately verify professional/mass-distribution terms: https://vb-audio.com/Services/licensing.htm .
- GitHub Actions and EXE/SHA256 publishing are configured: see [Windows releases](../docs/windows-releases.md). Android release signing is a separate step.
