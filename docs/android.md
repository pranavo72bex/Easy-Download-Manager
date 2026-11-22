# dm on Android

## Test on the Mac with Android Studio

Open the Gradle project in Android Studio, press Run, and dm comes up on an emulator or a phone.
Gradle builds dm's Rust library itself before it packs the APK, so there is nothing to build by
hand first. First run on a Mac (Apple M1) on 2026-11-20: the APK builds, installs and starts,
and dm picks a renderer; the window is not drawn on the emulator (below, "The emulator on an
Apple-silicon Mac"). On a phone (Samsung SM-M156B, Android 16, Mali-G57 over Vulkan,
2026-11-21) the window draws and the checklist's downloads, notifications, Back and a magnet
work; from a terminal: `gradle installDebug -Pdm.abis=arm64-v8a` with `JAVA_HOME` a JDK 17 and
`ANDROID_HOME` the SDK.

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
   15. **JDK 17.** Gradle 8.14.3 does not run on Java 25, which newer Android Studios carry:
   in Settings › Build › Gradle › Gradle JDK pick a 17 (Download JDK… › 17), or from a terminal
   set `JAVA_HOME` to one.

### Open and run

1. File › Open, and choose `crates/dm-android/android` (this folder, not the
   repository's top). Android Studio writes `local.properties` (where the SDK is) and syncs.
   The sync downloads Gradle 8.14.3 (`gradle/wrapper/gradle-wrapper.properties`, checked
   against its SHA-256) and the Android Gradle plugin; the first one takes a few minutes.
2. Tools › Device Manager › Create Virtual Device: a phone (Pixel 8, say), the system image
   **API 35, arm64-v8a** (on Apple silicon the images are arm64). Make a second one with an
   **API 26** (Android 8.0) arm64-v8a image: the oldest Android dm takes.
3. Choose the device in the toolbar and press Run (the green triangle). Gradle runs
   `cargoNdkDebug` first, which runs, from the repository's root:

   `cargo ndk --platform 26 -t arm64-v8a -o …/app/build/rustJniLibs/debug build -p dm-android --lib`

   The first build compiles all of dm and takes a long time (many minutes); later ones only
   what changed, and Gradle skips the step when no file under `crates`,
   `vendor`, `assets`, `Cargo.toml` or `Cargo.lock` changed. The processor is
   the one of the device chosen (arm64-v8a on an Apple-silicon Mac and on phones); to build
   others, put `dm.abis=arm64-v8a,x86_64` in `~/.gradle/gradle.properties`.
4. A phone works the same: Developer options › USB debugging on, plugged in, chosen in the
   toolbar.

If a tool is missing, the build stops with a message naming it and the command that installs
it (cargo, cargo-ndk, a Rust target, the NDK). Android Studio started from the Dock does not
see the shell's PATH; the build looks for cargo in `~/.cargo/bin` itself. A release build
(Build › Select Build Variant › release) builds the Rust with `--release`.

From a terminal instead, with the wrapper in this folder (it fetches Gradle 8.14.3 and checks
its SHA-256): `./gradlew installDebug` (`gradlew.bat` on Windows).

### The emulator on an Apple-silicon Mac

GPUI's renderer needs storage buffers (OpenGL ES 3.1 or Vulkan). What the emulator offers on an
M1 (2026-11-20, emulator 36, images API 35 and 37), none of which draws dm's window:

- `-gpu host`, API 35: Vulkan through MoltenVK, which fails to compile GPUI's quad shader
  (`prepare_gradient_color`, the emulator's log), and the device is lost.
- `-gpu host`, API 37: that image's Vulkan driver cannot open its render node, so only OpenGL
  ES 3.0 is offered, which has no storage buffers ("scene too large … maximum of 0").
- `-gpu swiftshader_indirect`, API 35: Vulkan in software; making the render pipelines never
  returns (the thread waits in `vkCreateGraphicsPipelines` on the emulator).

dm itself starts in each (`adb logcat | grep gpui`: "Selected GPU adapter"). See the window on a
phone with USB debugging; an x86-64 computer's emulator, with its GPU's own Vulkan, may also do.

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
- [ ] **Windows and Back (N16).** Open Options (⋮ › Tasks › Options): it fills the screen,
      clear of the status bar and the navigation bar only where its view keeps to GPUI's safe
      area (owed otherwise); taps and keys go to it. Back closes it and the list is drawn again,
      as it was. Open a dialog (Add URL, then a download's Download details, or Help › About):
      over the list, full screen; Back closes it as its Cancel does; a question it asks comes
      over it and Back answers it as its Cancel. In the narrow layout with the ⋮ sheet in a
      submenu: Back goes back to the menu, Back again closes the sheet; with the drawer open,
      Back closes it. On the list with nothing open: Back sends dm to the background (Recents
      still shows it, downloads go on, no quit in Logcat); tapping dm brings the list back.
- [ ] **Add URL downloads (N2, N3).** A download added in the window runs, segmented, and
      finishes; the file is in Files › Downloads › dm. Stop it halfway and Resume: it goes on
      from where it was.
- [x] **A shared link (N5).** Shared to dm while dm is closed: dm opens and the link is added
      as a link dropped on the window is. Shared while dm is open: it arrives at once
      (`DmActivity.onNewIntent`; "Checked by hand" below). Text with no link in it: nothing
      added.
- [x] **A copied link (N5).** Options › General, "Offer to download links I copy" on. In
      Chrome, copy a link to a file, then switch to dm: the bar "Link on your clipboard:" with
      Download and Dismiss is above the list within a second (Android 12 and up also say that
      dm read the clipboard). Switch away and back without copying anything: the bar is as it
      was, not offered again; Dismiss, switch away and back: not offered. A link copied before
      dm was started is not offered (as on a desktop). With "Take a copied link at once":
      Download details opens instead.
      Seen 2026-11-22 (Samsung SM-M156B, Android 16): a link copied in Chrome's address bar was
      offered on coming back to dm; away and back kept the bar as it was; Dismiss, away and back
      offered nothing. The bar showed the link as "…" at a phone's width: Download and Dismiss
      now go under the link there (`clipwatch.rs`, a window test at 412 points).
- [ ] **A magnet and a `.torrent` (N5, N8).** Each opens dm and adds the torrent; the magnet's
      name appears once its metadata comes; it downloads. Switch the emulator's network off and
      on (the status bar's Wi-Fi) mid-way: it carries on.
- [ ] **In the background (N6).** With a download running, press Home: an ongoing notification
      "dm is downloading" is up while it runs and goes when nothing runs. Lock the screen for a
      minute: the download went on. Stop All, Retry and the foreground service: "Checked by
      hand" below. On Android 13 and up dm asks to post notifications once.
- [ ] **A finished download's notification (N6).** It appears; tapping it opens the file.
- [ ] **Where files land (N9).** On API 26: before storage is allowed the file goes to dm's own
      folder; once allowed, to `Download/dm`. On API 35: `Download/dm`, seen in Files.
- [ ] **Choosing files and folders (N9).** ⋮ › File › Import: the system's document picker
      opens; a `.dmlist` or a `.torrent` picked from Downloads is imported. Export: no panel, the
      list is written to `Download/dm` (`downloads.dmlist`, then `downloads (1).dmlist`; on API
      29, dm's own folder) and the status line says where. Back up dm and Restore the same way.
      Options › Save to Browse…: a prompt offers dm's own folder (on API 26 with storage
      allowed, `Download/dm` first), saying "On a phone dm downloads only into these folders.";
      Cancel leaves the field as it was.
- [x] **Secrets (N11).** Save a site's login in dm, close dm, open it again: the login is
      still there, and `files/dm/secrets/` holds `.sealed` files, not the login in clear.
      Seen 2026-11-22 (Samsung SM-M156B, Android 16): a login for httpbin.org written in clear
      into `settings.json` was moved at start into `secrets/…httpbin.org%0Admuser.sealed`
      (52 bytes, the password not in it; keystore2 made `dm-secrets` in the TEE) and cleared from
      the file; after a force-stop and a fresh start a share of
      `https://httpbin.org/basic-auth/dmuser/dmpass123` downloaded `"authenticated": true`.
- [ ] **Phone's rules (N10).** Options › Downloads has "Wi-Fi only", "Not while roaming" and
      "Pause when Data Saver is on" (on a phone alone). Wi-Fi only on, the emulator's Wi-Fi off
      (mobile data on): a running download waits, its row saying "the phone is not on Wi-Fi",
      and goes on with Wi-Fi back. "On a metered connection: wait" holds on mobile data, and the
      battery line below its level off the charger (`adb shell dumpsys battery set level 10`,
      `unplug`).
- [ ] **Nothing crashed.** `adb logcat -b crash` holds nothing of `com.dm.android`.

Note what broke, with the Logcat lines around it; each finding is a gaps row's next step.

dm's phone app is Rust (`docs/decisions.md`, 2026-11-20, "dm on Android with GPUI"): the
desktop's own window, dm-gpui's (`dm_gpui::run_app`), which starts the desktop's engine and
`dm-shell`'s `Backend` as on a computer, run from `crates/dm-android`. GPUI has no Android platform of its makers', so dm carries one:
`vendor/gpui-android`, gpui-mobile's Android platform at a pinned commit, moved to dm's
`gpui-pre` 0.3.12 (gaps row N16, `vendor/README.md`). What only a phone has is reached
through JNI. The activity is gpui-mobile's NativeActivity with two small Java classes of
gpui-mobile's (below), packed by a minimal Gradle project; there is no Kotlin.

## What is in the crate

| File | What it does |
|------|--------------|
| `src/android.rs` | `android_main`: dm's home (`files/dm`) and the phone's storage (`android/storage.rs`) first; the phone's services handed to `dm_desktop_base::phone::set_phone`; dm's folders (`DM_HOME` = `files/dm`, `TMPDIR` = `files/dm/tmp`, the Keystore's store for secrets, `dm_core::folders::set_phone_folders` with the download folder: dm's own folder on the shared storage on Android 10 and up, else `Download/dm` in shared storage where dm may write it, else the app's own folder on it); then `dm_gpui::run_app(&[], …)` with GPUI's application on gpui-android's platform, until the activity is destroyed. dm said to run in this process (`receivers::dm_started`) once its folders are set, for a receiver's Retry or Stop All and a new intent |
| `src/phone.rs` | `Phone`, dm's `PhoneServices` over the calls into Android (`Java`): a finished file indexed before its notification (a tap opens it), a failure's with Retry, the wake lock and the ongoing notification together while dm is busy (`busy`, its words at most once a second; the foreground service starts and stops with the notification), the schedule's alarm set when its time changes (`next_start`), open, reveal, the clipboard (readable only while dm is in front, `clipboard_readable`), a finished file published to `Download/dm` (`publish`), the folders dm downloads and saves into (`Places`: Browse…'s choices, Export's folder), the connection and battery for the phone's rules (`network`), the Trash refused. Tested on the host |
| `src/wakeup.rs` | When the schedule's alarm goes off (a minute early, as the desktop's wake timer) and what the phone's start does (tested on the host) |
| `src/shared.rs` | What a shared intent hands over (tested on the host) |
| `src/storage.rs`, `src/android/storage.rs` | The phone's storage, worked out once at start (gaps row N9): the shared Download folder looked up once, the folders dm writes by path, each probed with a file (`phone::Places`: the default download folder, what Browse… offers, where Export writes), where downloads land (dm's own folder on Android 10 and up) and each finished file put in `Download/dm` through the media store, or by path on 8 and 9; the decisions tested on the host |
| `src/android/java.rs` | `Java` over JNI: notifications, the media index, opening a file (by its media index address, else `file://`), the system's Downloads, the clipboard and whether dm is in front (`in_front`, gpui-android's `is_active`), the ongoing notification (the foreground service's), the wake lock, the schedule's alarm, publishing a finished file (`storage`), the connection and battery (`network`) |
| `src/android/network.rs` | The connection and the battery for the phone's rules (gaps row N10): `ConnectivityManager`'s active network (metered, roaming, Wi-Fi or a cable), Data Saver, the battery's sticky broadcast; what they mean decided in `dm_desktop_base::phone::network` |
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
| `dev.gpui.mobile.GpuiPathPicker` | gpui-mobile, unchanged | GPUI's `prompt_for_paths`: the system's document picker, the documents imported into the app's cache as paths; dm's Import, Restore and Choose… ask through it (N9) |
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

### Going on once dm is closed

Options › Downloads › "Keep downloads and torrents going when dm is closed" (off by default;
`downloads.keep_after_close`). Swiping dm away from the recent apps ends `android_main`
(android-activity's `onDestroy` waits for it); the window's quit has by then stopped every
download and put it back in line. With the setting on and a saved row running or waiting
(`dm_core::saved_rows`, `wakeup::goes_on`), `android_main` starts dm again on a thread of its
own, as `--headless` runs on a computer (`src/android/engine.rs`), and returns. The queue
resumes, a finished torrent is shared, and the foreground service with its ongoing notification
holds the process (Stop All reaches the headless dm through the inbox). Once nothing has been
downloading, waiting or sharing for ten seconds the headless loop ends
(`headless::QUIT_WHEN_IDLE`) and the process with it. The window's awake watch is retired as
the window winds down, before its downloads stop (`awake::retire_phone_watch` in `wind_down`):
left to speak, it let go of the foreground service at the stop, and Android's low-memory killer
ended the process in the moment before the headless dm took over (seen on the phone). The
headless dm's own watch then alone holds the service.

Checked on a phone (2026-11-21, Samsung SM-M156B, Android 16) with two public torrents: closed
from the recent apps mid-download, dm went on at 2 to 7 MB/s under its notification while the
low-memory killer reaped other processes; opened again, it came back in a fresh process with the
torrent where it got to and going on; closed again, the torrent finished, went to
`Download/dm/Tears of Steel`, and the process ended about ten seconds later, leaving "Download
finished".

Opened again while the headless dm runs, the new `android_main` stops it (all saved, as Exit
saves it), starts dm's launch activity in a new task and ends the process: GPUI's platform is
made once a process (gpui-android's `init_platform`), so the window cannot be made twice in one,
and Android starts the activity in a fresh process that resumes the queue. The process ends with
`_exit` (`engine::end_process`), never `std::process::exit`, whose C exit handlers free the Java
runtime's statics under its running threads: that left a SIGSEGV in 4 of 10 reopens. Seen on a
phone (Samsung SM-M156B, Android 16, 2026-11-22): dm swiped away while a 1 GB download ran, then
its notification tapped, ten times running; dm opened each time, with no crash in the log, and
the download went on to the end.

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
JDK 17, Gradle 8.14.3 (`./gradlew`, the wrapper made by Gradle 8.14.3 itself, its jar checked
against the SHA-256 Gradle publishes, 2026-11-22), and cargo-ndk.
Gradle builds the Rust itself (`app/build.gradle.kts`): `cargoNdkDebug` before `preDebugBuild`
and `cargoNdkRelease` (with `--release`) before `preReleaseBuild` run `cargo ndk --platform 26
-t <abi> -o app/build/rustJniLibs/<type> build -p dm-android --lib` from the repository's root, into a
folder per build type that the build type's source set packs (so `gradle assemble`, both
types at once, never packs one type's library in the other's APK). Its Gradle properties:

| Property | Default | What it does |
|----------|---------|--------------|
| `dm.abis` | the device Android Studio runs on, else `arm64-v8a` | the processors, comma-separated: `arm64-v8a`, `armeabi-v7a`, `x86_64` |
| `dm.skipCargo` | `false` | `true`: no Rust built; the library is already in `app/src/main/jniLibs` (CI) |
| `dm.ndkVersion` | `27.2.12479018` | the NDK under the SDK's `ndk/` (also the one the Android Gradle plugin strips with); `ANDROID_NDK_HOME` wins for cargo-ndk |

cargo is looked for on PATH, then in `CARGO_HOME/bin` and `~/.cargo/bin`; a missing cargo,
cargo-ndk, Rust target or NDK stops the build with the command that installs it. Gradle skips the
step when nothing under `crates` (this Gradle project aside), `vendor`,
`assets`, `Cargo.toml`, `Cargo.lock`, `.cargo/config.toml` or `rust-toolchain.toml`
changed; cargo decides what to rebuild.

```sh
cd crates/dm-android/android
gradle assembleDebug -Pdm.abis=arm64-v8a,x86_64   # app/build/outputs/apk/debug/app-debug.apk
adb install app/build/outputs/apk/debug/app-debug.apk
```

Or the library by hand, as CI builds it:

```sh
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
| `android` | each push to main, each pull request touching `crates/` | cargo-ndk check for arm64-v8a and x86_64; clippy for arm64-v8a with `-D warnings` (`missing_const_for_thread_local` allowed: on Android it flags every `const { }` thread-local); dm-android's host tests; the x86_64 library with cargo-ndk; Android 35's platform and build tools from the runner's SDK; Gradle 8.14.3, checked against its SHA-256; `gradle assembleDebug -Pdm.skipCargo=true` (the library built above; the runner's NDK as `dm.ndkVersion`); the APK kept as the artifact `dm-android-x86_64-apk` |
| `emulator` | after `android`, for API 26 and API 35 | KVM opened to the runner's user; the emulator, platform-tools and Google's x86_64 system image through `sdkmanager`; an AVD at a phone's size (1080 x 2340, 420 dpi), booted without a window, the GPU in software (SwiftShader); `scripts/android-emulator-check.sh`; what it saw kept as `dm-android-emulator-api<N>` |
| `release` | a tag `android-v<version>`, after both above pass | the three Rust targets; the tag checked against dm-android's `Cargo.toml` and Gradle's `versionName`; the library for arm64-v8a, armeabi-v7a and x86_64 (paths remapped); platform and build tools; Gradle; dm's key from the secrets, else a notice; `gradle assembleRelease -Pdm.skipCargo=true`; `apksigner verify --print-certs`; `dm-<version>.apk` (or `dm-<version>-unsigned.apk`) and its `.sha256` attached to a draft release, never marked latest |

No action of anyone else's boots the emulator: the runner's own SDK tools do it from the command
line, and `sdkmanager` checks each package it fetches.

`scripts/android-emulator-check.sh` installs the debug APK with its permissions granted, starts
`com.dm.android/com.dm.android.DmActivity` and, 20 seconds later, needs dm's process
(`pidof`), its activity as the resumed one (`dumpsys activity activities`) and dm's window
not blank (`screencap`, the status and navigation bars left out: at least 16 colours, and 2 % of
the pixels unlike the commonest, the background). Then it stops dm, serves 3 MiB of random bytes from the
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
  (`deny.toml`) already holds every crate's licence to the allowed list. Prebuilt
  binaries in the source are not allowed: `jniLibs/` is not in git, and dm carries none but
  Gradle's own `gradle-wrapper.jar`, which F-Droid's scanner accepts by its published checksum.
- **A build recipe** (fdroiddata's `metadata/com.dm.android.yml`, written by whoever submits):
  the repository; the tag `android-v<version>` as the commit; `subdir:
  crates/dm-android/android`; `gradle: [yes]`; the NDK by version (`ndk:`, the one the
  release run prints); `sudo` installing Rust 1.98.1 with rustup (with the three Android
  targets) and cargo-ndk; and a `prebuild:` running the release job's `cargo ndk … build
  --release --locked` from the repository's root, with `gradleprops: [dm.skipCargo=true]` so Gradle does
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
  dm-gpui's many windows are a stack on the phone's one surface (gpui-android's
  `dm(android-stack)`): the newest drawn full screen, Back closing it or, on the list, sending
  dm to the background. Coded 2026-11-20 and not compiled (the Mac builds it). Owed: each
  window's view keeping its controls out of the status and navigation bars (`Window::insets`,
  which the platform now fills); dm-gpui's windows do not read it yet.
- **A link shared while dm is open (N5).** `DmActivity.onNewIntent` hands it to Rust; coded,
  not compiled or run (the checklist above).
- **The clipboard offer (N5).** The desktop's own watch (Options › General, "Offer to download
  links I copy", `dm-gpui/src/platform/clipwatch.rs`), looked at only while dm's window is in
  front (`PhoneServices::clipboard_readable`, gpui-android's `is_active`): Android 10 and up let
  only the app in front read the clipboard, so a link copied in another app is offered as dm
  comes to the front. Coded 2026-11-20, not compiled or run (the checklist above).
- **Downloading in the background (N6).** The wake lock and the ongoing notification are in,
  held while a row runs, a queued one is due or a finished torrent is shared
  (`awake::watch_phone` telling `PhoneServices::busy`); the notification says the count and the
  speed in the status bar's words, changed at most once a second, and a tap brings dm to the
  front. The foreground service, Stop All and Retry are coded, not compiled or run.
- **The schedule and the phone's start (N12).** Neither starts dm without its window: a
  notification asks for a tap. Where Android 12 and up allow dm only inexact alarms, dm asks
  once, in a notification saying why, whose tap opens Android's Alarms & reminders page for dm.
  Seen on a phone 2026-11-22: the ask, the alarm exact once allowed and going off on the minute.
  A resume after the phone restarts is still to be seen.
- **Where files land (N9).** On Android 10 and up dm downloads into its own folder on the
  shared storage (`Android/data/com.dm.android/files/Download/dm`) and puts each finished file
  in `Download/dm` through the media store (`src/storage.rs`, `src/android/storage.rs`); on 8
  and 9 it writes `Download/dm` by path once storage is granted (the first start, before it is,
  downloads into dm's own folder, and a file finished there is moved across). A torrent's files
  stay in dm's folder, where its seeding reads them. A folder of the person's own through the
  system's storage picker is owed.
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
