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
| `src/phone.rs` | `Phone`, dm's `PhoneServices` over the calls into Android (`Java`): a finished file indexed before its notification (a tap opens it), a failure's with Retry, the wake lock and the ongoing notification together while dm is busy (`busy`, its words at most once a second; the foreground service starts and stops with the notification), the schedule's alarm set when its time changes (`next_start`), open, reveal, the clipboard, the Trash refused. Tested on the host |
| `src/wakeup.rs` | When the schedule's alarm goes off (a minute early, as the desktop's wake timer) and what the phone's start does (tested on the host) |
| `src/shared.rs` | What a shared intent hands over (tested on the host) |
| `src/android/java.rs` | `Java` over JNI: notifications, the media index, opening a file (by its media index address, else `file://`), the system's Downloads, the clipboard, the ongoing notification, the wake lock |
| `src/android/intent.rs` | The activity's intent, read once dm's window is up (a thread waiting for gpui-android's `is_active` the first time) and for each new one (`DmActivity.onNewIntent`), one read at a time: SEND text and VIEW magnet or web address go to `dm_desktop_base::phone::shared`, a `.torrent` is copied into `files/dm/torrents` and goes to `phone::shared_file`; the window adds both as it adds a dropped link |
| `src/android/notify.rs` | Notification channels and messages, a failure's Retry and the ongoing notification's Stop All (broadcasts to `DmActionReceiver`), the ongoing notification, the partial wake lock, the permissions asked for once the window is up |
| `src/android/service.rs` | The foreground service (`DmService`, dataSync): started when dm becomes busy (the ongoing notification is its `startForeground` notification), stopped when not; Android 15's `onTimeout` stops it cleanly |
| `src/android/receivers.rs` | What `DmActionReceiver` and `DmBootReceiver` hand over: Retry and Stop All left in the running dm's inbox, Open, the schedule's alarm (`AlarmManager`), the phone's start |
| `src/android/natives.rs` | The native methods of dm's Java classes, by their exported `Java_com_dm_android_<Class>_<method>` names |
| `src/android/open.rs`, `jvm.rs` | Opening a file; the Java machine and dm's context (the activity, or the application's where a receiver started the process) |
| `java/` | The Java classes (below) |
| `android/` | The Gradle project that makes the APK: `app/src/main/AndroidManifest.xml` (package `com.dm.android`, Android 8.0 (API 26) and up, built against API 35, the permissions and the intent filters) and `app/build.gradle.kts` |

dm-gpui's start does the rest as on a computer: the engine, the settings, the queue, the window,
and on GPUI's quit (the activity destroyed) the downloads stopped and saved to be resumed.
`android_main` and the JNI modules are built only for Android; on the host the crate's tests
check `phone.rs` and `shared.rs`.

## The Java classes

NativeActivity alone cannot hold the soft keyboard's composition (an IME's `InputConnection`
lives on Android's UI thread), and gpui-android calls these classes by name through JNI, so
they are fixed by its symbols. Java and Gradle as gpui-mobile has them, at the maintainer's word
(2026-11-20, "use gpui-mobile."). dm's own four, one per feature, each small and doing nothing
but hand its event to Rust through a `native` method, at the maintainer's word (2026-11-20,
asked "Allow one small Java file per feature that just hands its call to Rust?": "Yes, all
four"). Gradle compiles all of `java/` (`java.srcDirs("../../java")`).

Their native methods are found by exported name, `Java_com_dm_android_<Class>_<method>`
(`src/android/natives.rs`), as gpui-android's are, not registered from `jvm::init`: Android
starts dm's process for the service or a receiver alone (the phone's start, an alarm), before
or without `android_main`, so each class loads `libdm_android.so` itself (`System.loadLibrary`
in a static block; the activity through gpui-mobile's `onCreate`), and each native method keeps
the Java machine and a context first (`jvm::adopt`).

| Class | From | Why |
|-------|------|-----|
| `dev.gpui.mobile.GpuiInputActivity` | gpui-mobile, unchanged | The activity the manifest names: a NativeActivity with a hidden `EditText` whose `InputConnection` reports composition and commits to `Java_dev_gpui_mobile_GpuiInputActivity_nativeIme` (gpui-android `jni.rs`), and `gpuiShowKeyboard`/`gpuiHideKeyboard`/`gpuiResetComposition`, which gpui-android calls. It also loads `libdm_android.so` with `System.loadLibrary` so the JNI methods resolve |
| `dev.gpui.mobile.GpuiPathPicker` | gpui-mobile, unchanged | GPUI's `prompt_for_paths`: the system's document picker, the documents imported into the app's cache as paths |
| `com.dm.android.DmActivity` | dm (N5) | The activity the manifest names: `GpuiInputActivity` with `onNewIntent` made the activity's intent (`setIntent`) and handed to Rust (`newIntent`), so a link shared while dm is open reaches the window's Add at once |
| `com.dm.android.DmService` | dm (N6) | The foreground service, type dataSync, not exported: `onStartCommand` (`started`), `onTimeout` (Android 15, `timedOut`), `onDestroy` (`stopped`) handed to Rust, which calls `startForeground`, `stopForeground` and `stopSelf` |
| `com.dm.android.DmActionReceiver` | dm (N6, N12) | Not exported: `com.dm.android.RETRY` (the row in `com.dm.android.ROW`), `STOP_ALL`, `OPEN` and `SCHEDULE`, with the notification's number, handed to Rust (`received`) |
| `com.dm.android.DmBootReceiver` | dm (N12) | `BOOT_COMPLETED` (exported, the sender holding `RECEIVE_BOOT_COMPLETED`) handed to Rust (`booted`) |

gpui-mobile's two are taken under GPL-3.0-or-later, as gpui-android is; dm's four carry dm's
licence header.

### How a notification's button reaches dm

A button is a `PendingIntent.getBroadcast` (immutable) to `DmActionReceiver`. Rust leaves the
order in the running dm's inbox (`dm_shell::shell`), the path a second launch and the desktop
notification's Retry already use, which the engine's own watcher takes up whether or not the
window is drawn: Retry is `ask_running_to_retry` (the row tried again, as the window's Retry),
Stop All is `ask_running_to_stop_all` (`stop-all`, which the watcher carries out as
`Command::StopAll` does: `DownloadList::pause_all`). The window follows from the queue's change.
A dm that is not running takes them at its next start.

### The schedule and the phone's start

dm-desktop-base's wake watch hands the schedule's next start to the phone every minute
(`PhoneServices::next_start`; on a phone always, not only where "wake the computer" is ticked);
`Phone` sets an `AlarmManager` alarm for it, a minute early, only when its time changes
(`setExactAndAllowWhileIdle`, or `setAndAllowWhileIdle` where Android 12 and up do not allow
dm exact alarms), and keeps its time in `files/dm/next-start`. The alarm sends `SCHEDULE`:
where dm runs, the foreground service holds it up for three minutes (an exact alarm lets an app
start one from the background) and dm's own schedule starts the queue; where the process was
started for the alarm, a notification says the schedule is due and its tap opens dm. dm's
window needs the activity (GPUI draws in it), so neither the alarm nor the phone's start runs
dm without it. At the phone's start (`BOOT_COMPLETED`) the alarm is set again from
`next-start`, and where the saved list (`dm_core::saved_rows`) has downloads that were running
or waiting, or the schedule's time went by, a notification says "dm has downloads to resume"
(or "dm's schedule is due"); its tap opens dm, whose start goes on with them.

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
the debug key Gradle makes. A release APK is signed with dm's own key when
`DM_ANDROID_KEYSTORE_FILE` and its passwords are in the environment (below); otherwise it comes
out unsigned (`app-release-unsigned.apk`).

## In CI

`.github/workflows/android.yml` has three jobs:

| Job | When | Its steps |
|-----|------|-----------|
| `android` | each push to main, each pull request touching `desktop/` | cargo-ndk check for arm64-v8a and x86_64; dm-android's host tests; the x86_64 library with cargo-ndk; Android 35's platform and build tools from the runner's SDK; Gradle 8.14.3, checked against its SHA-256; `gradle assembleDebug`; the APK kept as the artifact `dm-android-x86_64-apk` |
| `emulator` | after `android`, for API 26 and API 35 | KVM opened to the runner's user; the emulator, platform-tools and Google's x86_64 system image through `sdkmanager`; an AVD at a phone's size (1080 x 2340, 420 dpi), booted without a window, the GPU in software (SwiftShader); `scripts/android-emulator-check.sh`; what it saw kept as `dm-android-emulator-api<N>` |
| `release` | a tag `android-v<version>`, after both above pass | the three Rust targets; the tag checked against dm-android's `Cargo.toml` and Gradle's `versionName`; the library for arm64-v8a, armeabi-v7a and x86_64 (paths remapped); platform and build tools; Gradle; dm's key from the secrets, else a notice; `gradle assembleRelease`; `apksigner verify --print-certs`; `dm-<version>.apk` (or `dm-<version>-unsigned.apk`) and its `.sha256` attached to a draft release, never marked latest |

No action of anyone else's boots the emulator: the runner's own SDK tools do it from the command
line, and `sdkmanager` checks each package it fetches.

`scripts/android-emulator-check.sh` installs the debug APK with its permissions granted, starts
`com.dm.android/com.dm.android.DmActivity` and, 20 seconds later, needs dm's process
(`pidof`), its activity as the resumed one (`dumpsys activity activities`) and dm's blue
(0x1e88e5, the square of `src/app.rs`) on the screen (`screencap`, at least 200 pixels; N17's
window changes what is looked for). Then it stops dm, serves 3 MiB of random bytes from the
runner (`python3 -m http.server`, which the emulator reaches as `10.0.2.2`), starts dm with a
share (`am start -a android.intent.action.SEND -t text/plain --es android.intent.extra.TEXT
http://10.0.2.2:8765/dm-ci-probe.bin`), and needs the file whole (its SHA-256) within two
minutes in one of the folders dm downloads to (`Download/dm`, the app's own folder on shared
storage, or `files/` read through `run-as`), nothing of dm's in logcat's crash buffer, and dm
still running. dm is stopped before the share, so the share starts it (a share to a running dm
is N5's `onNewIntent`, on the checklist below). The artifact holds `screen.png`, `logcat.txt`, `activity.txt`, `gfxinfo.txt`,
`surfaces.txt`, `notifications.txt`, `server.log` and the emulator's log.

### Checked by hand on a phone or the emulator (not yet run)

The script does not reach these; each needs a run with its result written in gaps rows N5, N6
and N12:

- [ ] A link shared while dm is open (`am start -a android.intent.action.SEND -t text/plain
  --es android.intent.extra.TEXT <url>` with dm in front) reaches the window's Add at once,
  once; a `.torrent` the same way.
- [ ] A download started, then Home and the screen off for ten minutes: it goes on (`adb shell
  dumpsys activity services com.dm.android` shows `DmService` foreground, type dataSync); the
  ongoing notification is the service's; when nothing runs the service and the notification go.
- [ ] Stop All in the ongoing notification stops every download, each resumable.
- [ ] A failure's notification shows Retry; pressing it tries the row again and the
  notification goes.
- [ ] Android 15: the service's time running out (six hours of dataSync a day) ends it cleanly
  through `onTimeout`, with no crash; dm downloads on while the process lives.
- [ ] Android 12 and up from the background: a refused `startForegroundService` is logged in
  dm's words and the plain ongoing notification stays.
- [ ] A queue start set in the Scheduler two minutes ahead: `adb shell dumpsys alarm` shows
  dm's alarm (exact where allowed); with dm in the background the queue starts on time.
- [ ] `adb reboot` with a download running: after the start, "dm has downloads to resume";
  its tap opens dm and the download goes on; a schedule set before the reboot is in `dumpsys
  alarm` again.

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
  --release --locked` from `desktop/`. `UpdateCheckMode: Tags ^android-v.*$`, which reads
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
- **A link shared while dm is open (N5).** `DmActivity.onNewIntent` hands it to Rust; coded,
  not compiled or run (the checklist above).
- **Downloading in the background (N6).** The wake lock and the ongoing notification are in,
  held while a row runs, a queued one is due or a finished torrent is shared
  (`awake::watch_phone` telling `PhoneServices::busy`); the notification says the count and the
  speed in the status bar's words, changed at most once a second, and a tap brings dm to the
  front. The foreground service, Stop All and Retry are coded, not compiled or run.
- **The schedule and the phone's start (N12).** Coded, not compiled or run. Neither starts dm
  without its window: a notification asks for a tap. The exact-alarm permission is not asked
  for with its reason yet (without it the alarm is inexact).
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
| P02 | Shares a link while dm is already open | The same as P01, in the open dm (`DmActivity.onNewIntent`, N5; coded, not run) | no phone app; as P01 |
| P03 | Shares text with no link in it | Nothing is added; a notification "dm added nothing" (or "dm could not add it") says why | no phone app; as B01 |
| P04 | Opens a magnet link (taps it in a browser, picks dm) | dm opens and adds the torrent; its name appears once its metadata arrives, and it downloads as the desktop's torrents do | no phone app; dm's own torrent rows |
| P05 | Opens a `.torrent` (from Files, a browser's downloads or a mail, picks dm) | dm copies it into its own folder under the name the sending app gave and adds it, as P04; a file that is not a torrent is refused in words and nothing is added | no phone app; dm's own torrent rows |
| P06 | Opens dm on a phone (a narrow screen) | A top bar (dm's name, Add, an overflow menu holding the desktop's menus and toolbar), each download a card (name, bar, size, speed, time left), no table of columns and no side panels. Not yet: N7 and N17 | no phone app; the desktop's layout (B25) at a phone's width |
| P07 | Taps the top bar's menu button | A drawer slides in with the categories and the queues, as the desktop's side panel lists them; tapping one shows only its downloads and closes the drawer. Not yet: N7 | no phone app; as the desktop's categories |
| P08 | Taps a download's card | Its details open as a full screen (address, folder, size, connections, Stop / Resume, Delete), with a Back that returns to the list where it was. Not yet: N7 | no phone app; as the desktop's properties |
| P09 | Leaves dm (Home, another app) with downloads running | They go on; an ongoing notification says "dm: N downloads running" with the speed and the count waiting, changed at most once a second, and goes away when nothing runs. A foreground service (N6, coded, not run) keeps Android from stopping dm | no phone app; downloads run with the window closed (the desktop's tray) |
| P10 | Taps a finished download's notification | The file opens in the app the phone uses for its type; with none, Android says so. A failed download's notification opens dm | no phone app; as the desktop's "Download complete" Open |
| P11 | Taps Stop All in the ongoing notification | Every running download stops, keeps what it fetched and can be resumed (B04, B05); the notification goes (`DmActionReceiver`, N6; coded, not run) | no phone app; as Stop all on the desktop |
| P12 | Turns the phone sideways, or changes its font size | The same window, laid out again for the new size; no download restarts and dm is not started again | no phone app |
