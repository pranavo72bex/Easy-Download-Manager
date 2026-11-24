# The phone app (Android)

dm on Android is the same Rust and the same GPUI window as the desktop, in the phone's layout
(under 600 points wide), inside a small Java shell. Android 10 (API 29) and up only.

## Where things are

- `crates/dm-android/src/`: the Rust side: `android.rs` (the activity's life and the window),
  `phone.rs` (what is the phone's own: notifications, sharing, storage), `storage.rs`,
  `wakeup.rs`, `sealed.rs`.
- `crates/dm-android/java/` and `crates/dm-android/android/`: the Java shell and the Gradle
  project (`app/build.gradle.kts`).
- The window itself is `crates/dm-gpui` ([desktop.md](desktop.md)): a phone change is usually
  a window change at narrow width, and must not break the desktop's layout.
- `docs/android.md`: how it is built, run and what has been seen on real phones.

## How to test

- `cargo test -p dm-android` runs on the runner (no phone needed).
- The phone's layout: dm-gpui's window tests at phone width (the sweep's narrow widths, and
  the tests named for a phone, e.g. `on_a_phone_…`), with the worst cases in
  `.claude/skills/break/scenarios.md` (the largest text size, a long name, right to left).
- The pull request runs "Android, the engine built" (`android.yml`): `cargo ndk` check and
  clippy for arm64-v8a and x86_64, the tests, and the APK. Code behind
  `#[cfg(target_os = "android")]` is only compiled there: read it carefully, it is not built
  on Linux.

## Traps

- Touch targets are at least 48 dp; nothing needs a hover; Back steps back (a page to its
  list, a dialog closed), never out of the app unexpectedly.
- No new Android permission: a feature that needs one is not one for a bot.
- An emulator has no vibration and the Mac's emulator draws no window: what only a real phone
  can show is not claimed done.

## Lessons

