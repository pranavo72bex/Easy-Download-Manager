// dm's APK: the activity (gpui-mobile's GpuiInputActivity, a NativeActivity) and the
// Rust library it loads, libdm_android.so, which cargo-ndk puts in
// src/main/jniLibs/<abi>/ first (docs/android.md):
//
//   cd desktop
//   cargo ndk --platform 26 -t arm64-v8a -o crates/dm-android/android/app/src/main/jniLibs \
//       build -p dm-android --lib
//   cd crates/dm-android/android && gradle assembleDebug

plugins {
    id("com.android.application")
}

android {
    namespace = "com.dm.android"
    // Built against Android 15; Android 8.0 (API 26) and up (gaps row N1).
    compileSdk = 35

    defaultConfig {
        applicationId = "com.dm.android"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.10.0"
        // The activity's `android.app.lib_name`: libdm_android.so without `lib` and `.so`.
        manifestPlaceholders["nativeLibraryName"] = "dm_android"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
        debug {
            isJniDebuggable = true
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    // The Java classes live beside the crate (desktop/crates/dm-android/java): gpui-mobile's
    // GpuiInputActivity (the activity) and GpuiPathPicker, which gpui-android calls by name.
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
