# dm on Android

dm's phone app is Rust alone (`docs/decisions.md`, 2026-11-20): the desktop's engine and
`dm-shell`'s `Backend` under Slint screens, in `desktop/crates/dm-android`. What only a phone
has is reached through JNI. Nothing here is Java or Kotlin, and no Gradle is needed.

## What is in the crate

| File | What it does |
|------|--------------|
| `src/lib.rs` | `Engine`: dm started in the app's home, over `Backend` (gaps row N3) |
| `src/app.rs` | The Slint window. For now a placeholder (dm's name and the row count); the screens (N7) replace `app::run` |
| `src/main.rs` | The same window on a computer, at a phone's size (412 × 915) |
| `src/shared.rs`, `src/ongoing.rs` | What a shared intent hands over, and the words of the ongoing notification (tested on the host) |
| `src/android/mod.rs` | `android_main`: Slint's Android backend, dm's home (`files/dm`), the download folder (`Download/dm` in shared storage, else the app's own folder on it), the engine, the window |
| `src/android/intent.rs` | The activity's intent at start and on each resume: SEND text, VIEW magnet or web address, VIEW a `.torrent` (copied into `files/dm/torrents` and added as `file://`) |
| `src/android/notify.rs`, `busy.rs` | Notification channels and messages; the ongoing notification and the partial wake lock while `Engine::busy()`; the permissions asked for at start |
| `src/android/open.rs`, `system.rs` | `AndroidSystem` over JNI: opening a file (by its media index address, else `file://`), the system's Downloads, the clipboard, the notification sound |

`Cargo.toml`'s `[package.metadata.android]` is the APK's manifest: package `com.dm.android`,
Android 8.0 (API 26) and up, built against API 35, the permissions, and the intent filters.

## Running it on a computer

```sh
cd desktop
cargo run -p dm-android            # dm's files in <temp>/dm-phone
cargo run -p dm-android -- ~/dm-phone
```

This is the phone's engine and window, drawn by Slint's winit backend. Notifications are
printed on the terminal. Opening files and the clipboard are refused, and the window says why.

## Building the APK

You need Rust's Android targets, the Android SDK with platform 35 and its build tools, the NDK,
a JDK (Slint's Android backend compiles a small Java helper), and cargo-apk:

```sh
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
export ANDROID_HOME=$HOME/Android/Sdk
export ANDROID_NDK_ROOT=$ANDROID_HOME/ndk/<version>
cargo install cargo-apk --version 0.10.0 --locked
cd desktop
cargo apk run -p dm-android --lib --target aarch64-linux-android   # onto a phone over adb
cargo apk build -p dm-android --lib --release                     # all three, needs a key
```

A debug build is signed with `~/.android/debug.keystore`. A release build needs
`CARGO_APK_RELEASE_KEYSTORE` and `CARGO_APK_RELEASE_KEYSTORE_PASSWORD`. CI
(`.github/workflows/android.yml`) builds the x86_64 APK for the emulator with a key made for
that run, and keeps it as the run's artifact.

## Still owed

- **The screens (N7).** The list, a row's details, Add, Options. Each one's words should come
  from `dm-view` (N15).
- **A link shared while dm is open (N5).** NativeActivity does not pass `onNewIntent` to native
  code, so such a link reaches dm only when Android starts the activity again. The Add sheet
  and the clipboard offer are also still owed.
- **Downloading in the background (N6).** The wake lock and the ongoing notification are in,
  but Android may still stop a process left in the background. A foreground service, a Stop All
  button in the notification and a Retry button on a failure each need a Java class. Whether
  to add one is the maintainer's call.
- **First run (N9).** On Android 8 and 9, the first start downloads into the app's own folder
  until storage is granted. On Android 10, the shared Download folder cannot be written by path
  at all.
- **Testing (N13, N14).** None of the Android code has been run yet: not built for a phone, not
  run on an emulator. Also owed: Slint's testing backend for the screens, a release key, F-Droid.
