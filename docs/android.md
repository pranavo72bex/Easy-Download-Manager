# dm on Android

## Test on the Mac with Android Studio

Open the Gradle project in Android Studio, press Run, and dm comes up on an emulator or a phone.
Gradle builds dm's Rust library itself before it packs the APK, so there is nothing to build by
hand first. This was written on a computer without Android Studio: none of it has been run yet,
and the first time through is its test.

### Install once

1. **Rust,** with rustup (https://rustup.rs), if it is not there yet:
   `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
2. **Rust's Android targets,** from the repository's folder, so they go to the Rust that
   `rust-toolchain.toml` names (1.98.1):

   ```sh
   cd path/to/the/repository
   rustup target add aarch64-linux-android x86_64-linux-android armv7-linux-androideabi
   ```

3. **cargo-ndk:** `cargo install cargo-ndk`
4. **The NDK and CMake,** in Android Studio: Settings › Languages & Frameworks › Android SDK ›
   SDK Tools, tick "Show Package Details", then under "NDK (Side by side)" tick **27.2.12479018**
   (the one `app/build.gradle.kts` names) and tick "CMake"; Apply. Another NDK version works too:
   the build then says which it used, or name it with `dm.ndkVersion=<version>` in
   `~/.gradle/gradle.properties`.
5. **Android 15's platform** (API 35) comes with Android Studio; if not, SDK Platforms › Android
   15. JDK 17 is Android Studio's own; nothing to install.

### Open and run

1. File › Open, and choose `desktop/crates/dm-android/android` (this folder, not the
   repository's top). Android Studio writes `local.properties` (where the SDK is) and syncs.
   The sync downloads Gradle 8.14.3 (`gradle/wrapper/gradle-wrapper.properties`, checked
   against its SHA-256) and the Android Gradle plugin; the first one takes a few minutes.
2. Tools › Device Manager › Create Virtual Device: a phone (Pixel 8, say), the system image
   **API 35, arm64-v8a** (on Apple silicon the images are arm64). Make a second one with an
   **API 26** (Android 8.0) arm64-v8a image: the oldest Android dm takes.
3. Choose the device in the toolbar and press Run (the green triangle). Gradle runs
   `cargoNdkDebug` first, which runs, from `desktop/`:

   `cargo ndk --platform 26 -t arm64-v8a -o …/app/build/rustJniLibs/debug build -p dm-android --lib`

   The first build compiles all of dm and takes a long time (many minutes); later ones only
   what changed, and Gradle skips the step when no file under `desktop/crates`,
   `desktop/vendor`, `desktop/assets`, `Cargo.toml` or `Cargo.lock` changed. The processor is
   the one of the device chosen (arm64-v8a on an Apple-silicon Mac and on phones); to build
   others, put `dm.abis=arm64-v8a,x86_64` in `~/.gradle/gradle.properties`.
4. A phone works the same: Developer options › USB debugging on, plugged in, chosen in the
   toolbar.

If a tool is missing, the build stops with a message naming it and the command that installs
it (cargo, cargo-ndk, a Rust target, the NDK). Android Studio started from the Dock does not
see the shell's PATH; the build looks for cargo in `~/.cargo/bin` itself. A release build
(Build › Select Build Variant › release) builds the Rust with `--release`.

From a terminal instead (needs Gradle 8.14.3 or a wrapper made once with
`gradle wrapper --gradle-version 8.14.3` in this folder): `gradle installDebug`.

### Hand dm a link, a magnet, a `.torrent`

- **Share a link:** in the emulator's Chrome, open a page with a file to download, then the
  menu › Share › dm. Or from the Mac's terminal (`adb` is in
  `~/Library/Android/sdk/platform-tools`):

  ```sh
  adb shell am start -a android.intent.action.SEND -t text/plain \
      --es android.intent.extra.TEXT https://example.com/file.zip
  ```

  Android asks which app; pick dm. With dm already open the link does not reach it (gaps row
  N5): close dm first (swipe it away in the recent apps), then share.
- **A magnet:** tap a magnet link on a web page, or
  `adb shell am start -a android.intent.action.VIEW -d 'magnet:?xt=urn:btih:<hash>'`.
- **A `.torrent`:** download one with the emulator's Chrome, then open it from Files
  (or Chrome's Downloads) and pick dm. Or push one and open it:
  `adb push file.torrent /sdcard/Download/` and open it from Files › Downloads.

Downloads land in `Download/dm` (Files › Downloads › dm), on Android 8 and 9 once storage is
allowed; else in dm's own folder on the phone's storage.

### Where the logs are

Android Studio's Logcat (View › Tool Windows › Logcat), filtered with
`package:com.dm.android` (everything dm's process writes), or `tag:gpui-android` (the Rust's
log: dm's and the window's messages, and a Rust panic as `PANIC at <file>:<line>`). A crash
of the app as a whole is under `adb logcat -b crash`. The same from a terminal:
`adb logcat --pid=$(adb shell pidof com.dm.android)`.

### What to try

Each line is a gaps row; what it says is owed is what this tries for the first time.

- [ ] **dm starts (N3, N16, N17).** dm's own window comes up, not a black screen, on API 35 and
      on API 26. Turn the emulator sideways and back: the window is laid out again, dm does not
      start over (no restart in Logcat).
- [ ] **The narrow layout (N7).** At a phone's width: one top bar (Places, "dm", Add URL, ⋮),
      each download a card (name, bar, size, speed, state, time left), no table and no side
      panel. ⋮ opens a sheet with the toolbar's commands, then Tasks, File, Downloads, View,
      Help; Places opens the drawer of categories and queues, a place chosen closes it. Tap a
      card: chosen; tap it again or double-tap: what a double-click does; ⋯ or a long press:
      the row menu as a sheet. Typing in Add URL brings up the keyboard.
- [ ] **Add URL downloads (N2, N3).** A download added in the window runs, segmented, and
      finishes; the file is in Files › Downloads › dm. Stop it halfway and Resume: it goes on
      from where it was.
- [ ] **A shared link (N5).** Shared to dm while dm is closed: dm opens and the link is added
      as a link dropped on the window is. Shared while dm is open: known not to arrive yet
      (N5's `onNewIntent`, the maintainer's word). Text with no link in it: nothing added.
- [ ] **A magnet and a `.torrent` (N5, N8).** Each opens dm and adds the torrent; the magnet's
      name appears once its metadata comes; it downloads. Switch the emulator's network off and
      on (the status bar's Wi-Fi) mid-way: it carries on.
- [ ] **In the background (N6).** With a download running, press Home: an ongoing notification
      "dm is downloading" is up while it runs and goes when nothing runs. Lock the screen for a
      minute: the download went on. Stop All and Retry buttons are not there yet (a Java class,
      the maintainer's word). On Android 13 and up dm asks to post notifications once.
- [ ] **A finished download's notification (N6).** It appears; tapping it opens the file.
- [ ] **Where files land (N9).** On API 26: before storage is allowed the file goes to dm's own
      folder; once allowed, to `Download/dm`. On API 35: `Download/dm`, seen in Files.
- [ ] **Secrets (N11).** Save a site's login in dm, close dm, open it again: the login is
      still there, and `files/dm/secrets/` holds `.sealed` files, not the login in clear.
- [ ] **Phone's rules (N10).** Being coded on another branch; nothing to try yet.
- [ ] **Nothing crashed.** `adb logcat -b crash` holds nothing of `com.dm.android`.

Note what broke, with the Logcat lines around it; each finding is a gaps row's next step.

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
| `src/phone.rs` | `Phone`, dm's `PhoneServices` over the calls into Android (`Java`): a finished file indexed before its notification (a tap opens it), the wake lock and the ongoing notification together while dm is busy (`busy`, its words at most once a second), open, reveal, the clipboard, the Trash refused. Tested on the host |
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
JDK 17, Gradle 8.14.3 (`gradle/wrapper/gradle-wrapper.properties`; the wrapper's scripts and
jar are not in git, made once with `gradle wrapper --gradle-version 8.14.3`), and cargo-ndk.
Gradle builds the Rust itself (`app/build.gradle.kts`): `cargoNdkDebug` before `preDebugBuild`
and `cargoNdkRelease` (with `--release`) before `preReleaseBuild` run `cargo ndk --platform 26
-t <abi> -o app/build/rustJniLibs/<type> build -p dm-android --lib` from `desktop/`, into a
folder per build type that the build type's source set packs (so `gradle assemble`, both
types at once, never packs one type's library in the other's APK). Its Gradle properties:

| Property | Default | What it does |
|----------|---------|--------------|
| `dm.abis` | the device Android Studio runs on, else `arm64-v8a` | the processors, comma-separated: `arm64-v8a`, `armeabi-v7a`, `x86_64` |
| `dm.skipCargo` | `false` | `true`: no Rust built; the library is already in `app/src/main/jniLibs` (CI) |
| `dm.ndkVersion` | `27.2.12479018` | the NDK under the SDK's `ndk/` (also the one the Android Gradle plugin strips with); `ANDROID_NDK_HOME` wins for cargo-ndk |

cargo is looked for on PATH, then in `CARGO_HOME/bin` and `~/.cargo/bin`; a missing cargo,
cargo-ndk, Rust target or NDK stops the build with the command that installs it. Gradle skips the
step when nothing under `desktop/crates` (this Gradle project aside), `desktop/vendor`,
`desktop/assets`, `Cargo.toml`, `Cargo.lock`, `.cargo/config.toml` or `rust-toolchain.toml`
changed; cargo decides what to rebuild.

```sh
cd desktop/crates/dm-android/android
gradle assembleDebug -Pdm.abis=arm64-v8a,x86_64   # app/build/outputs/apk/debug/app-debug.apk
adb install app/build/outputs/apk/debug/app-debug.apk
```

Or the library by hand, as CI builds it:

```sh
cd desktop
cargo ndk --platform 26 -t arm64-v8a -t x86_64 \
    -o crates/dm-android/android/app/src/main/jniLibs build -p dm-android --lib --release
cd crates/dm-android/android
gradle assembleDebug -Pdm.skipCargo=true
```

`app/src/main/jniLibs/`, Gradle's `build/` and `.gradle/`, Android Studio's `.idea/` and
`local.properties` are not kept in git. A debug APK is signed with
the debug key Gradle makes. A release APK is signed with dm's own key when
`DM_ANDROID_KEYSTORE_FILE` and its passwords are in the environment (below); otherwise it comes
out unsigned (`app-release-unsigned.apk`).

## In CI

`.github/workflows/android.yml` has three jobs:

| Job | When | Its steps |
|-----|------|-----------|
| `android` | each push to main, each pull request touching `desktop/` | cargo-ndk check for arm64-v8a and x86_64; dm-android's host tests; the x86_64 library with cargo-ndk; Android 35's platform and build tools from the runner's SDK; Gradle 8.14.3, checked against its SHA-256; `gradle assembleDebug -Pdm.skipCargo=true` (the library built above; the runner's NDK as `dm.ndkVersion`); the APK kept as the artifact `dm-android-x86_64-apk` |
| `emulator` | after `android`, for API 26 and API 35 | KVM opened to the runner's user; the emulator, platform-tools and Google's x86_64 system image through `sdkmanager`; an AVD at a phone's size (1080 x 2340, 420 dpi), booted without a window, the GPU in software (SwiftShader); `scripts/android-emulator-check.sh`; what it saw kept as `dm-android-emulator-api<N>` |
| `release` | a tag `android-v<version>`, after both above pass | the three Rust targets; the tag checked against dm-android's `Cargo.toml` and Gradle's `versionName`; the library for arm64-v8a, armeabi-v7a and x86_64 (paths remapped); platform and build tools; Gradle; dm's key from the secrets, else a notice; `gradle assembleRelease -Pdm.skipCargo=true`; `apksigner verify --print-certs`; `dm-<version>.apk` (or `dm-<version>-unsigned.apk`) and its `.sha256` attached to a draft release, never marked latest |

No action of anyone else's boots the emulator: the runner's own SDK tools do it from the command
line, and `sdkmanager` checks each package it fetches.

`scripts/android-emulator-check.sh` installs the debug APK with its permissions granted, starts
`com.dm.android/dev.gpui.mobile.GpuiInputActivity` and, 20 seconds later, needs dm's process
(`pidof`), its activity as the resumed one (`dumpsys activity activities`) and dm's window
not blank (`screencap`, the status and navigation bars left out: at least 16 colours, and 2 % of
the pixels unlike the commonest, the background). Then it stops dm, serves 3 MiB of random bytes from the
runner (`python3 -m http.server`, which the emulator reaches as `10.0.2.2`), starts dm with a
share (`am start -a android.intent.action.SEND -t text/plain --es android.intent.extra.TEXT
http://10.0.2.2:8765/dm-ci-probe.bin`), and needs the file whole (its SHA-256) within two
minutes in one of the folders dm downloads to (`Download/dm`, the app's own folder on shared
storage, or `files/` read through `run-as`), nothing of dm's in logcat's crash buffer, and dm
still running. dm is stopped before the share because NativeActivity hands a running dm no new
intent (N5). The artifact holds `screen.png`, `logcat.txt`, `activity.txt`, `gfxinfo.txt`,
`surfaces.txt`, `notifications.txt`, `server.log` and the emulator's log.

## A release

Raise `versionCode` by one, then tag the commit `android-v<version>`, where the version is
dm-android's `Cargo.toml` and Gradle's `versionName` (both 0.10.0 now). Not `v*` (the
extension's releases, `release.yml`) or `desktop-v*` (the desktop's). The release job drafts a
GitHub release holding one APK for the three processors and its SHA-256.

dm's key is four repository secrets:

| Secret | What it is |
|--------|------------|
| `DM_ANDROID_KEYSTORE` | the keystore (PKCS12 or JKS), base64: `base64 -w0 dm-release.keystore` |
| `DM_ANDROID_KEYSTORE_PASSWORD` | the keystore's password |
| `DM_ANDROID_KEY_ALIAS` | the key's alias in it |
| `DM_ANDROID_KEY_PASSWORD` | the key's password |

Making one costs nothing: `keytool -genkeypair -v -keystore dm-release.keystore -storetype
PKCS12 -alias dm -keyalg RSA -keysize 4096 -validity 10000`. Keep the keystore and its
passwords out of git and somewhere safe: Android installs an update only over an APK signed with
the same key, so a lost key means every user uninstalls dm to take the next one. The job writes
the keystore into the runner's temporary folder and deletes it after Gradle; Gradle's `release`
signing config (`app/build.gradle.kts`) reads its path (`DM_ANDROID_KEYSTORE_FILE`) and the
passwords from the environment. Without the secrets the release still goes out, the APK named
`-unsigned` and a notice in the run, as R1's unsigned desktop builds do; Android installs it only
once someone signs it (`apksigner sign --ks …`).

Google Play is BLOCKED: its developer account has a fee, the same rule as R1 and R4, until the
maintainer says.

## F-Droid

F-Droid is free and builds every app itself, from source, on its own servers. Nothing has been
submitted. What it needs, and where dm stands:

- **Free software only.** dm is GPL-3.0-or-later; the Android side adds gpui-android and the two
  Java classes (gpui-mobile's, GPL-3.0-or-later), `android-activity`, `jni` and `wgpu` (MIT or
  Apache-2.0), all free. No Google Play Services, Firebase, analytics or AndroidX; the one
  Gradle plugin is the Android Gradle plugin, which F-Droid builds with. `cargo deny`
  (`desktop/deny.toml`) already holds every crate's licence to the allowed list. Prebuilt
  binaries in the source are not allowed: `jniLibs/` is not in git, and dm carries none.
- **A build recipe** (fdroiddata's `metadata/com.dm.android.yml`, written by whoever submits):
  the repository; the tag `android-v<version>` as the commit; `subdir:
  desktop/crates/dm-android/android`; `gradle: [yes]`; the NDK by version (`ndk:`, the one the
  release run prints); `sudo` installing Rust 1.98.1 with rustup (with the three Android
  targets) and cargo-ndk; and a `prebuild:` running the release job's `cargo ndk … build
  --release --locked` from `desktop/`, with `gradleprops: [dm.skipCargo=true]` so Gradle does
  not build it again. `UpdateCheckMode: Tags ^android-v.*$`, which reads
  `versionName` and `versionCode` from `app/build.gradle.kts` (both literal there for that
  reason).
- **Reproducible builds,** so F-Droid can ship dm's own signed APK: the same Rust (1.98.1,
  `rust-toolchain.toml`), `--locked`, the same NDK, `--remap-path-prefix` for the checkout and
  cargo's folder (the release job does it), and the same Gradle and build tools. Whether two
  builds come out identical has not been tried; until then F-Droid signs with its own key.
- **Anti-features** the listing would likely carry: `NonFreeNet` (dm can fetch from services
  such as YouTube, which are not free).
- **Store text and pictures** in fastlane's layout (`fastlane/metadata/android/en-US/`:
  `title.txt`, `short_description.txt`, `full_description.txt`, `images/phoneScreenshots/`),
  which F-Droid reads from the repository; not written yet, waiting for dm's phone window (N17).

## Still owed

- **dm's window on a phone (N3, N16, N17, N7).** `android_main` runs dm-gpui's window; it has
  been checked for `aarch64-linux-android` (with a stand-in C compiler for ring), not linked
  with the NDK, packed or run. The narrow layout that makes the window a phone's is N7's.
- **A link shared while dm is open (N5).** NativeActivity does not pass `onNewIntent` to native
  code, so such a link reaches dm only when Android starts the activity again; a Java
  `onNewIntent` waits for the maintainer's word, as N6's service does.
- **Downloading in the background (N6).** The wake lock and the ongoing notification are in,
  held while a row runs, a queued one is due or a finished torrent is shared
  (`awake::watch_phone` telling `PhoneServices::busy`); the notification says the count and the
  speed in the status bar's words, changed at most once a second, and a tap brings dm to the
  front. Android may still stop a process left in the background. A foreground service, a Stop All
  button in the notification and a Retry button on a failure each need a Java class. Whether
  to add one is the maintainer's call.
- **First run (N9).** On Android 8 and 9, the first start downloads into the app's own folder
  until storage is granted. On Android 10, the shared Download folder cannot be written by path
  at all.
- **Testing (N13, N14).** None of the Android code has been run yet: the emulator job and the
  release job are written but have had no run. Also owed: dm's release key in the repository's
  secrets (the maintainer's), the phone's contract rows (below, waiting for the maintainer's
  word) and their QA script, an F-Droid submission.

## Proposed contract rows for the phone, waiting for the maintainer's word

A proposal only: none of this is in `docs/behaviour-contract.md`, and none of it binds anyone
until the maintainer approves it in words; then it goes there as a section of its own, with its
QA script beside `human-qa.py`. The rows are in the contract's own form. Reference A is a
Windows program with no phone app of its own, so that column names the desktop row each one
follows. A row that needs what is not built yet says so (N5, N6, N7, N17).

| ID | A person does | What must happen | Reference A |
|---|---|---|---|
| P01 | Shares a link from another app (a browser's Share › dm) while dm is not open | dm opens and adds the download at once; it starts, its card shows its progress, and a notification "dm added a download" names it. The file lands in `Download/dm`, which the Files app shows (on Android 8 and 9 once storage is allowed; else in dm's own folder on the phone's storage). With N7's Add sheet, the sheet opens instead, as Download details does (B02) | no phone app; as B02 and B03 |
| P02 | Shares a link while dm is already open | The same as P01, in the open dm. Not yet: NativeActivity hands a running dm no new intent, so it needs a Java `onNewIntent` (N5), the maintainer's word | no phone app; as P01 |
| P03 | Shares text with no link in it | Nothing is added; a notification "dm added nothing" (or "dm could not add it") says why | no phone app; as B01 |
| P04 | Opens a magnet link (taps it in a browser, picks dm) | dm opens and adds the torrent; its name appears once its metadata arrives, and it downloads as the desktop's torrents do | no phone app; dm's own torrent rows |
| P05 | Opens a `.torrent` (from Files, a browser's downloads or a mail, picks dm) | dm copies it into its own folder under the name the sending app gave and adds it, as P04; a file that is not a torrent is refused in words and nothing is added | no phone app; dm's own torrent rows |
| P06 | Opens dm on a phone (a narrow screen) | A top bar (dm's name, Add, an overflow menu holding the desktop's menus and toolbar), each download a card (name, bar, size, speed, time left), no table of columns and no side panels. Not yet: N7 and N17 | no phone app; the desktop's layout (B25) at a phone's width |
| P07 | Taps the top bar's menu button | A drawer slides in with the categories and the queues, as the desktop's side panel lists them; tapping one shows only its downloads and closes the drawer. Not yet: N7 | no phone app; as the desktop's categories |
| P08 | Taps a download's card | Its details open as a full screen (address, folder, size, connections, Stop / Resume, Delete), with a Back that returns to the list where it was. Not yet: N7 | no phone app; as the desktop's properties |
| P09 | Leaves dm (Home, another app) with downloads running | They go on; an ongoing notification says "dm: N downloads running" with the speed and the count waiting, changed at most once a second, and goes away when nothing runs. A foreground service, so that Android does not stop dm later, is N6's and the maintainer's word | no phone app; downloads run with the window closed (the desktop's tray) |
| P10 | Taps a finished download's notification | The file opens in the app the phone uses for its type; with none, Android says so. A failed download's notification opens dm | no phone app; as the desktop's "Download complete" Open |
| P11 | Taps Stop All in the ongoing notification | Every running download stops, keeps what it fetched and can be resumed (B04, B05); the notification goes. Not yet: a notification's button needs a broadcast receiver, a Java class (N6), the maintainer's word | no phone app; as Stop all on the desktop |
| P12 | Turns the phone sideways, or changes its font size | The same window, laid out again for the new size; no download restarts and dm is not started again | no phone app |
