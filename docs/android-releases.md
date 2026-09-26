# Android releases

Workflow `.github/workflows/android-release.yml` builds a regular signed release APK for installation without Google Play. Android 8.0+ is required; one APK contains arm64-v8a, armeabi-v7a and x86_64.

## Release process

1. Increment `versionCode` and update `versionName` in `android-receiver/app/build.gradle.kts`. `versionCode` must increase with every APK update.
2. Update `android-receiver/ReleaseNotes.md`, commit and push the changes.
3. Create the `android-v<versionName>` tag and push it to GitHub, for example `git tag android-v0.1.0` and `git push origin android-v0.1.0`.
4. Wait for the **Android APK** workflow: tests, lint, all-ABI build, signing, certificate and SHA256 verification, and APK/checksum publication.

A manual workflow run on a branch creates an artifact without a release. A tag release is first created as a draft and published after the files are uploaded. Do not overwrite published tags or APKs. If publishing is interrupted and leaves a draft, inspect it before rerunning the release job.

Windows and Android are released separately. An Android release does not replace Windows at `/releases/latest`; find the current Android release in the Releases list by platform name.

## Signing

Repository GitHub Secrets:

- `ANDROID_KEYSTORE_BASE64` — Base64-encoded JKS contents.
- `ANDROID_KEYSTORE_PASSWORD` — JKS password.
- `ANDROID_KEY_ALIAS` — key alias.
- `ANDROID_KEY_PASSWORD` — key password.

The key is used only during the signing step and is not passed to tests. The temporary JKS is deleted when the step finishes. The key and passwords must not be added to git, artifacts, release notes or logs. The public certificate fingerprint is stored in `android-receiver/release-certificate.sha256`; the workflow verifies it before uploading the APK.

Keep a separate protected backup of the key and passwords: GitHub Secrets cannot be downloaded back. Losing the key prevents normal updates from being installed over existing APKs. If Google Play is added later, take this key into account when configuring Play App Signing if update compatibility between distribution channels is required.

Switching from an older debug build requires a one-time reinstall; subsequent releases use the same key. Development debug builds remain unchanged.

## Checks

CI environment: Ubuntu 24.04, JDK 21, SDK/build-tools 35, NDK 28.2.13676358, CMake 3.22.1; Gradle Wrapper verifies the SHA256 of the distribution. `testReleaseUnitTest`, `lintRelease`, `assembleRelease`, then `apksigner verify`. Reports are available as the `android-reports` artifact.

Real Wi-Fi, background playback and synchronization with the PC require testing on physical devices. This workflow does not publish the application to Google Play and does not use AAB.

A local backup of the first key is stored in the git-ignored `.signing/android/` directory (JKS and password file). Before moving the project, save this directory in protected storage. Do not attach it to a release.