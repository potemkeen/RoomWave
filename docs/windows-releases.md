# Windows releases

Workflow: `.github/workflows/windows-release.yml`. Windows x64 only; Android is not built.

## Releasing a version

1. Update versions consistently in `windows-host/package.json`, `package-lock.json` (including the root package), `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and `Cargo.lock`. Update `windows-installer/ReleaseNotes.md`.
2. Review the changes, commit them and push to GitHub.
3. Create and push a new immutable tag, for example `git tag windows-v0.1.0`, then `git push origin windows-v0.1.0`.
4. Wait for the **Windows installer** workflow. On success, the EXE and SHA256 will appear in Releases. No APK is included.

A manual run through Actions → Windows installer → Run workflow on a branch only builds an artifact (retained for 14 days), without publishing it. A run on a release tag publishes a release. Do not overwrite already published tags/files; publish fixes as a new version. If a failure leaves a draft, inspect it and delete only that unfinished draft before rerunning the release job.

## Environment and dependencies

Windows Server 2022 runner, Node 22.12.0, Rust 1.94.0, Inno Setup 6.7.3. `npm ci`, Cargo lockfile, `cargo test --locked`. Tests requiring real audio hardware remain ignored.

`Prepare-BuildTools.ps1` downloads VB-CABLE Pack45 and Inno Setup from official URLs and verifies pinned SHA256 values. The WebView2 Evergreen bootstrapper is updated by Microsoft and verified using Microsoft's Authenticode publisher signature; its hash is visible in the build log. `Package-RoomWave.ps1` additionally verifies the driver version and signature. The driver is not installed on the runner.

Actions are pinned to commit SHAs. The build has only `contents: read`, while the separate release job has `contents: write`; no personal tokens/secrets are required. Publishing uses the built-in GITHUB_TOKEN, verifies SHA256, first creates a draft with the files, then publishes it.

If the available GitHub Actions quota is exhausted, the GitHub limit/billing must be changed or the quota must be allowed to refresh. Release privacy follows the repository privacy setting.

Local build: `./windows-installer/Prepare-BuildTools.ps1`, then `./windows-installer/Build-Installer.ps1`. Inno Setup has separate commercial-use terms; VB-CABLE is distributed with the original files and attribution. RoomWave/Setup signing is not configured yet.

This automates building and publishing. It does not replace installation, reboot and real-audio testing on Windows.
