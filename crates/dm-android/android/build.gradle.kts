// The Android Gradle plugin, at one version for the whole build. Nothing else:
// the Rust is built by cargo-ndk before Gradle runs (docs/android.md).
plugins {
    id("com.android.application") version "8.7.3" apply false
}
