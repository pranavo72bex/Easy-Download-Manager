# dm for Android

The phone's window for dm (gaps rows N5 to N7): Jetpack Compose in Kotlin over the
desktop's engine, `desktop/crates/dm-android`, bound through UniFFI
(`docs/decisions.md`, "dm on Android"). Android 8.0 (API 26) and up.

## What you need

- JDK 17.
- The Android SDK with platform 35 and the NDK (Android Studio's SDK Manager, or
  `sdkmanager "platforms;android-35" "ndk;27.2.12479018"`). Point Gradle at it with
  `ANDROID_HOME`, or a `local.properties` holding `sdk.dir=...`; set `ANDROID_NDK_HOME` to
  the NDK for cargo-ndk.
- Rust (the toolchain in `rust-toolchain.toml`) with the phone's targets:

  ```sh
  rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
  cargo install cargo-ndk
  ```

- Gradle 8.11 once, to make the wrapper: the repository holds
  `gradle/wrapper/gradle-wrapper.properties` but not the wrapper's jar or scripts. In this
  folder run `gradle wrapper` once; `./gradlew` works from then on.

## Building

From `mobile/android`:

```sh
./gradlew assembleDebug
```

`preBuild` runs two tasks first (`app/build.gradle.kts`):

1. `cargoNdk`: `cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -o app/src/main/jniLibs
   build -p dm-android --release`, run in `desktop/`.
2. `uniffiBindgen`: `cargo run -p dm-android --features bindgen --bin uniffi-bindgen --
   generate --library <the first ABI's libdm_android.so> --language kotlin --out-dir
   app/build/generated/uniffi`, the Kotlin bindings (package `uniffi.dm_android`), which are
   never checked in.

Switches, in `gradle.properties` or on the command line as `-Pname=value`:

| Property | Default | What it does |
|----------|---------|--------------|
| `dm.cargo` | `cargo` | cargo's path, where Android Studio's PATH lacks `~/.cargo/bin` |
| `dm.rustProfile` | `release` | `debug` for a quicker engine build, or a profile of `desktop/Cargo.toml` |
| `dm.abis` | `arm64-v8a,armeabi-v7a,x86_64` | the ABIs built; the first is read for the bindings |
| `dm.skipRust` | `false` | `true` when the library and bindings are already made |

For the emulator alone: `./gradlew assembleDebug -Pdm.abis=x86_64 -Pdm.rustProfile=debug`
(on an Apple-silicon Mac's arm64 emulator, `-Pdm.abis=arm64-v8a`).

## Running on the emulator

```sh
emulator -avd <name> &            # an API 26 or later image
./gradlew installDebug
adb shell am start -n com.dm.android/.MainActivity
```

Sharing a link to dm: `adb shell am start -a android.intent.action.SEND -t text/plain
--es android.intent.extra.TEXT "https://example.com/file.zip" -n com.dm.android/.MainActivity`.
A magnet: `adb shell am start -a android.intent.action.VIEW -d "magnet:?xt=urn:btih:..."`.

## Tests

- `./gradlew testDebugUnitTest`: the Kotlin helpers on the JVM (sizes, speeds, times, the
  Options form read from and written to the settings JSON, the list's chips).
- `./gradlew connectedDebugAndroidTest`: the list screen on a running emulator, over a fake
  source, so no engine is started.
- The engine's own tests are Rust: `cargo test -p dm-android` in `desktop/`.

## Where things are

| Path | What it is |
|------|------------|
| `app/src/main/java/com/dm/android/MainActivity.kt` | the window; what is shared or opened with dm reaches the Add sheet here |
| `engine/` | the one engine (`EngineHolder`), the phone's side of it (`PhoneSystem`), and the screens' source over it (`EngineSource`) |
| `service/` | the foreground service that keeps downloads going, its notifications, and Retry |
| `share/IncomingLinks.kt` | a share's text, a magnet, a `.torrent` copied into dm's files |
| `data/` | what the screens use that needs no phone: the source interface, formatting, the Options form, the chips |
| `ui/` | the Compose screens: list, details, Add, Options |
