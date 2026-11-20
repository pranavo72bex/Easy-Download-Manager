# dm on Android

dm's phone app is Rust (`docs/decisions.md`, 2026-11-20, "dm on Android with GPUI"): the
desktop's own window, dm-gpui's (`dm_gpui::run_app`), which starts the desktop's engine and
`dm-shell`'s `Backend` as on a computer, run from `desktop/crates/dm-android`. GPUI has no Android platform of its makers', so dm carries one:
`desktop/vendor/gpui-android`, gpui-mobile's Android platform at a pinned commit, moved to dm's
`gpui-pre` 0.3.12 (gaps row N16, `desktop/vendor/README.md`). What only a phone has is reached
through JNI. The activity is gpui-mobile's NativeActivity with two small Java classes of
gpui-mobile's (below), packed by a minimal Gradle project; there is no Kotlin.

## What is in the crate

| File | What it does |
|------|--------------|
| `src/android.rs` | `android_main`: the phone's services handed to `dm_desktop_base::phone::set_phone` first; dm's folders (`DM_HOME` = `files/dm`, `TMPDIR` = `files/dm/tmp`, `dm_core::folders::set_phone_folders` with the download folder: `Download/dm` in shared storage, else the app's own folder on it); then `dm_gpui::run_app(&[], …)` with GPUI's application on gpui-android's platform, until the activity is destroyed |
| `src/phone.rs` | `Phone`, dm's `PhoneServices` over the calls into Android (`Java`): a finished file indexed before its notification (a tap opens it), the wake lock and the ongoing notification together (`keep_awake`), open, reveal, the clipboard, the Trash refused. Tested on the host |
| `src/shared.rs` | What a shared intent hands over (tested on the host) |
| `src/android/java.rs` | `Java` over JNI: notifications, the media index, opening a file (by its media index address, else `file://`), the system's Downloads, the clipboard, the ongoing notification, the wake lock |
| `src/android/intent.rs` | The activity's intent, read when the activity comes to the front (a thread watching gpui-android's `is_active`): SEND text and VIEW magnet or web address go to `dm_desktop_base::phone::shared`, a `.torrent` is copied into `files/dm/torrents` and goes to `phone::shared_file`; the window adds both as it adds a dropped link |
| `src/android/notify.rs` | Notification channels and messages, the ongoing notification, the partial wake lock, the permissions asked for once the window is up |
| `src/android/open.rs`, `jvm.rs` | Opening a file; the Java machine and the activity, kept once at start |
| `java/` | The activity's Java classes (below) |
| `android/` | The Gradle project that makes the APK: `app/src/main/AndroidManifest.xml` (package `com.dm.android`, Android 8.0 (API 26) and up, built against API 35, the permissions and the intent filters) and `app/build.gradle.kts` |

dm-gpui's start does the rest as on a computer: the engine, the settings, the queue, the window,
and on GPUI's quit (the activity destroyed) the downloads stopped and saved to be resumed.
`android_main` and the JNI modules are built only for Android; on the host the crate's tests
check `phone.rs` and `shared.rs`.

## The Java classes

NativeActivity alone cannot hold the soft keyboard's composition (an IME's `InputConnection`
lives on Android's UI thread), and gpui-android calls these classes by name through JNI, so
they are fixed by its symbols. Java and Gradle as gpui-mobile has them, at the maintainer's word
(2026-11-20, "use gpui-mobile."); no other Java (N6's service, `onNewIntent`, notification
buttons) without a word of its own:

| Class | From | Why |
|-------|------|-----|
| `dev.gpui.mobile.GpuiInputActivity` | gpui-mobile, unchanged | The activity the manifest names: a NativeActivity with a hidden `EditText` whose `InputConnection` reports composition and commits to `Java_dev_gpui_mobile_GpuiInputActivity_nativeIme` (gpui-android `jni.rs`), and `gpuiShowKeyboard`/`gpuiHideKeyboard`/`gpuiResetComposition`, which gpui-android calls. It also loads `libdm_android.so` with `System.loadLibrary` so the JNI methods resolve |
| `dev.gpui.mobile.GpuiPathPicker` | gpui-mobile, unchanged | GPUI's `prompt_for_paths`: the system's document picker, the documents imported into the app's cache as paths |

Both are taken under GPL-3.0-or-later, as gpui-android is.

## Building the APK

You need Rust's Android targets, the Android SDK with platform 35 and build tools 35, the NDK,
JDK 17, Gradle 8.9 or later (CI uses 8.14.3), and cargo-ndk:

```sh
rustup target add aarch64-linux-android x86_64-linux-android
export ANDROID_HOME=$HOME/Android/Sdk
export ANDROID_NDK_HOME=$ANDROID_HOME/ndk/<version>
cargo install cargo-ndk
cd desktop
cargo ndk --platform 26 -t arm64-v8a -t x86_64 \
    -o crates/dm-android/android/app/src/main/jniLibs build -p dm-android --lib --release
cd crates/dm-android/android
gradle assembleDebug                 # app/build/outputs/apk/debug/app-debug.apk
adb install app/build/outputs/apk/debug/app-debug.apk
```

`app/src/main/jniLibs/` and Gradle's `build/` are not kept in git. A debug APK is signed with
the debug key Gradle makes; a release needs dm's own key (N14). CI
(`.github/workflows/android.yml`) checks dm-android for arm64-v8a and x86_64, runs its tests,
builds the x86_64 library with cargo-ndk and the debug APK with Gradle (downloaded and checked
against its published SHA-256), and keeps the APK as the run's artifact.

## Still owed

- **dm's window on a phone (N3, N16, N17, N7).** `android_main` runs dm-gpui's window; it has
  been checked for `aarch64-linux-android` (with a stand-in C compiler for ring), not linked
  with the NDK, packed or run. The narrow layout that makes the window a phone's is N7's.
- **A link shared while dm is open (N5).** NativeActivity does not pass `onNewIntent` to native
  code, so such a link reaches dm only when Android starts the activity again; a Java
  `onNewIntent` waits for the maintainer's word, as N6's service does.
- **Downloading in the background (N6).** The wake lock and the ongoing notification are in,
  held while a download runs (dm's `awake` watch); the notification's words are fixed ("dm is
  downloading") until the seam carries the count and the speed. Android may still stop a
  process left in the background. A foreground service, a Stop All
  button in the notification and a Retry button on a failure each need a Java class. Whether
  to add one is the maintainer's call.
- **First run (N9).** On Android 8 and 9, the first start downloads into the app's own folder
  until storage is granted. On Android 10, the shared Download folder cannot be written by path
  at all.
- **Testing (N13, N14).** None of the Android code has been run yet: not linked for a phone,
  not run on an emulator. Also owed: a release key, F-Droid.
