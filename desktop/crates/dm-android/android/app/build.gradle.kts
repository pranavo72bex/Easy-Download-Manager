// dm's APK: the activity (dm's DmActivity over gpui-mobile's GpuiInputActivity, a
// NativeActivity), dm's service and receivers, and the Rust library they load,
// libdm_android.so, which cargo-ndk puts in src/main/jniLibs/<abi>/ first (docs/android.md):
//
//   cd desktop
//   cargo ndk --platform 26 -t arm64-v8a -o crates/dm-android/android/app/src/main/jniLibs \
//       build -p dm-android --lib
//   cd crates/dm-android/android && gradle assembleDebug

plugins {
    id("com.android.application")
}

// dm's release key (gaps row N14), given by CI's release job from the repository's secrets
// (.github/workflows/android.yml): the keystore's path and its passwords. Without them a
// release build comes out unsigned (app-release-unsigned.apk), as F-Droid wants it to sign
// itself, and a debug build is signed with the debug key Gradle makes, as before.
val releaseKeystore: String? = System.getenv("DM_ANDROID_KEYSTORE_FILE")?.takeIf { it.isNotEmpty() }

android {
    namespace = "com.dm.android"
    // Built against Android 15; Android 8.0 (API 26) and up (gaps row N1).
    compileSdk = 35

    defaultConfig {
        applicationId = "com.dm.android"
        minSdk = 26
        targetSdk = 35
        // Raised by one at each release (a tag android-v<versionName>): Android and F-Droid
        // take an update only with a higher one. A number, not computed, so F-Droid reads it.
        versionCode = 1
        versionName = "0.10.0"
        // The activity's `android.app.lib_name`: libdm_android.so without `lib` and `.so`.
        manifestPlaceholders["nativeLibraryName"] = "dm_android"
    }

    signingConfigs {
        if (releaseKeystore != null) {
            create("dm") {
                storeFile = file(releaseKeystore)
                storePassword = System.getenv("DM_ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("DM_ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("DM_ANDROID_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            if (releaseKeystore != null) {
                signingConfig = signingConfigs.getByName("dm")
            }
        }
        debug {
            isJniDebuggable = true
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    // The Java classes live beside the crate (desktop/crates/dm-android/java), all of it
    // compiled: gpui-mobile's GpuiInputActivity and GpuiPathPicker (dev/gpui/mobile), which
    // gpui-android calls by name, and dm's own (com/dm/android: DmActivity, DmService,
    // DmActionReceiver, DmBootReceiver), each handing its events to Rust (gaps rows N5, N6, N12).
    sourceSets {
        getByName("main") {
            java.srcDirs("../../java")
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }

    packaging {
        jniLibs {
            // cargo strips a release build itself; a debug build keeps its symbols for logcat.
            keepDebugSymbols += listOf("**/libdm_android.so")
        }
    }
}
