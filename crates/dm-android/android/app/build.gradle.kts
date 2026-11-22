// dm's APK: the activity (dm's DmActivity over gpui-mobile's GpuiInputActivity, a
// NativeActivity), dm's service and receivers, and the Rust library they load,
// libdm_android.so (docs/android.md).
//
// Android Studio's Run builds the library itself: `cargoNdkDebug` (or `cargoNdkRelease`)
// runs before `preDebugBuild` (`preReleaseBuild`):
//
//   cargo ndk --platform 29 -t <abi> -o crates/dm-android/android/app/build/rustJniLibs/debug \
//       build -p dm-android --lib            (--release for a release build)
//
// and the build type's source set packs that folder. Gradle properties (-P on the command
// line, or in gradle.properties):
//
//   dm.abis        the processors, comma-separated: arm64-v8a, armeabi-v7a, x86_64. Unset:
//                  the one of the phone or emulator Android Studio runs on, else arm64-v8a
//   dm.skipCargo   true: build no Rust; the library is already in src/main/jniLibs (CI builds
//                  it itself with cargo-ndk, .github/workflows/android.yml)
//   dm.ndkVersion  the NDK under the SDK's ndk/ folder (default below)
//
// From a terminal: `gradle assembleDebug -Pdm.abis=arm64-v8a,x86_64`.

import java.io.ByteArrayOutputStream
import java.io.File
import java.util.Properties
import javax.inject.Inject
import org.gradle.process.ExecOperations

plugins {
    id("com.android.application")
}

// Android 10 (API 29) and up (the maintainer, 2026-11-22): the APK's minSdk and cargo-ndk's
// --platform.
val dmMinSdk = 29

// The NDK Android Studio's SDK Manager installs (SDK Tools, NDK (Side by side), "Show
// Package Details"); cargo-ndk links the library with it and AGP strips it with it.
val dmNdkVersion: String =
    providers.gradleProperty("dm.ndkVersion").orNull?.trim()?.takeIf { it.isNotEmpty() }
        ?: "27.2.12479018"

// Each Android processor and Rust's target for it.
val dmRustTargets = mapOf(
    "arm64-v8a" to "aarch64-linux-android",
    "armeabi-v7a" to "armv7-linux-androideabi",
    "x86_64" to "x86_64-linux-android",
)

// dm.abis if given; else the first processor of the device Android Studio is running on that
// dm builds for (Studio passes android.injected.build.abi, e.g. "arm64-v8a,armeabi-v7a,armeabi");
// else arm64-v8a, the processor of Apple-silicon emulators and of phones.
val dmAbis: List<String> = run {
    val given = providers.gradleProperty("dm.abis").orNull
        ?.split(',', ' ')?.map { it.trim() }?.filter { it.isNotEmpty() }
    if (!given.isNullOrEmpty()) {
        val unknown = given.filter { it !in dmRustTargets }
        if (unknown.isNotEmpty()) {
            throw GradleException(
                "dm.abis names ${unknown.joinToString()}; dm builds for " +
                    dmRustTargets.keys.joinToString(),
            )
        }
        given.distinct()
    } else {
        val device = providers.gradleProperty("android.injected.build.abi").orNull
            ?.split(',')?.map { it.trim() }?.firstOrNull { it in dmRustTargets }
        listOf(device ?: "arm64-v8a")
    }
}

val dmSkipCargo: Boolean =
    providers.gradleProperty("dm.skipCargo").orNull?.trim()?.toBoolean() ?: false

// The cargo workspace: the repository's root (this project is crates/dm-android/android).
val dmWorkspace: File = rootDir.resolve("../../..").canonicalFile

// The SDK Android Studio writes into local.properties (sdk.dir), else the environment's.
val dmSdkDir: String = run {
    val local = Properties()
    val file = rootDir.resolve("local.properties")
    if (file.isFile) file.inputStream().use { local.load(it) }
    local.getProperty("sdk.dir")?.takeIf { it.isNotBlank() }
        ?: System.getenv("ANDROID_HOME")?.takeIf { it.isNotBlank() }
        ?: System.getenv("ANDROID_SDK_ROOT")?.takeIf { it.isNotBlank() }
        ?: ""
}

// dm's release key (gaps row N14), given by CI's release job from the repository's secrets
// (.github/workflows/android.yml): the keystore's path and its passwords. Without them a
// release build comes out unsigned (app-release-unsigned.apk), as F-Droid wants it to sign
// itself, and a debug build is signed with the debug key Gradle makes, as before.
val releaseKeystore: String? = System.getenv("DM_ANDROID_KEYSTORE_FILE")?.takeIf { it.isNotEmpty() }

android {
    namespace = "com.dm.android"
    // Built against Android 15; Android 10 (API 29) and up.
    compileSdk = 35
    ndkVersion = dmNdkVersion

    defaultConfig {
        applicationId = "com.dm.android"
        minSdk = dmMinSdk
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

    // A release's APKs for GitHub (gaps row N14): one for each processor built, small to
    // download (a phone takes arm64-v8a), and a universal one for any phone. Only for a release
    // build, so Android Studio's Run installs its one debug APK as before.
    splits {
        abi {
            isEnable = gradle.startParameter.taskNames.any { it.contains("Release") }
            reset()
            include(*dmAbis.toTypedArray())
            isUniversalApk = true
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

    // The Java classes live beside the crate (crates/dm-android/java), all of it
    // compiled: gpui-mobile's GpuiInputActivity and GpuiPathPicker (dev/gpui/mobile), which
    // gpui-android calls by name, and dm's own (com/dm/android: DmActivity, DmService,
    // DmActionReceiver, DmBootReceiver), each handing its events to Rust (gaps rows N5, N6, N12).
    // The library: src/main/jniLibs when it is built outside Gradle (CI, dm.skipCargo), and
    // the build type's own folder that cargoNdkDebug / cargoNdkRelease fill, which wins over
    // main. A folder per build type, so a build of both (`gradle assemble`) never packs one
    // type's library in the other's APK.
    sourceSets {
        getByName("main") {
            java.srcDirs("../../java")
            jniLibs.srcDirs("src/main/jniLibs")
        }
        getByName("debug") {
            jniLibs.srcDir(layout.buildDirectory.get().asFile.resolve("rustJniLibs/debug"))
        }
        getByName("release") {
            jniLibs.srcDir(layout.buildDirectory.get().asFile.resolve("rustJniLibs/release"))
        }
    }

    packaging {
        jniLibs {
            // cargo strips a release build itself; a debug build keeps its symbols for logcat.
            keepDebugSymbols += listOf("**/libdm_android.so")
        }
    }
}

// libdm_android.so built by cargo-ndk from the repository's root. What it needs from the computer is looked
// for when it runs, not when the project syncs, so a missing tool fails the build with the
// command that installs it, and the sync still succeeds.
abstract class CargoNdkTask : DefaultTask() {
    @get:Inject
    abstract val exec: ExecOperations

    @get:Input
    abstract val abis: ListProperty<String>

    @get:Input
    abstract val rustTargets: MapProperty<String, String>

    @get:Input
    abstract val release: Property<Boolean>

    @get:Input
    abstract val platform: Property<Int>

    @get:Input
    abstract val ndkVersion: Property<String>

    @get:Internal
    abstract val sdkDir: Property<String>

    @get:Internal
    abstract val workspace: DirectoryProperty

    // What the library is built from. cargo itself decides what to rebuild; this only lets
    // Gradle skip the task when none of it changed.
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val sources: ConfigurableFileCollection

    @get:OutputDirectory
    abstract val jniLibs: DirectoryProperty

    private fun fail(vararg lines: String): Nothing =
        throw GradleException(lines.joinToString("\n"))

    private fun exe(name: String): String =
        if (System.getProperty("os.name").lowercase().contains("windows")) "$name.exe" else name

    // cargo on PATH, else CARGO_HOME/bin, else ~/.cargo/bin: Android Studio started from the
    // Dock (or Finder, or a desktop launcher) does not have the shell's PATH.
    private fun findCargo(): File? {
        val dirs = mutableListOf<File>()
        System.getenv("PATH")?.split(File.pathSeparator)?.filter { it.isNotBlank() }
            ?.forEach { dirs += File(it) }
        System.getenv("CARGO_HOME")?.takeIf { it.isNotBlank() }?.let { dirs += File(it, "bin") }
        dirs += File(System.getProperty("user.home"), ".cargo/bin")
        return dirs.map { File(it, exe("cargo")) }.firstOrNull { it.isFile && it.canExecute() }
    }

    private fun compareVersions(a: String, b: String): Int {
        val x = a.split('.').map { it.toIntOrNull() ?: 0 }
        val y = b.split('.').map { it.toIntOrNull() ?: 0 }
        for (i in 0 until maxOf(x.size, y.size)) {
            val c = x.getOrElse(i) { 0 }.compareTo(y.getOrElse(i) { 0 })
            if (c != 0) return c
        }
        return 0
    }

    // ANDROID_NDK_HOME if set, else the SDK's ndk/<ndkVersion>, else the newest NDK there.
    private fun findNdk(): File {
        System.getenv("ANDROID_NDK_HOME")?.takeIf { it.isNotBlank() }?.let { File(it) }
            ?.takeIf { it.isDirectory }?.let { return it }
        val home = System.getProperty("user.home")
        val sdk = sdkDir.get().takeIf { it.isNotBlank() }?.let { File(it) }
            ?: listOf("Library/Android/sdk", "Android/Sdk", "AppData/Local/Android/Sdk")
                .map { File(home, it) }.firstOrNull { it.isDirectory }
        val wanted = ndkVersion.get()
        if (sdk != null) {
            val pinned = File(sdk, "ndk/$wanted")
            if (pinned.isDirectory) return pinned
            val newest = File(sdk, "ndk").listFiles()
                ?.filter { it.isDirectory && File(it, "source.properties").isFile }
                ?.maxWithOrNull { a, b -> compareVersions(a.name, b.name) }
            if (newest != null) {
                logger.warn(
                    "dm: NDK $wanted is not installed; building the Rust with ${newest.name}. " +
                        "Install $wanted, or pass -Pdm.ndkVersion=${newest.name}.",
                )
                return newest
            }
        }
        fail(
            "dm: the Android NDK was not found (looked at ANDROID_NDK_HOME, then " +
                "${sdk?.path ?: "the Android SDK"}/ndk/$wanted).",
            "Install it in Android Studio: Settings > Languages & Frameworks > Android SDK >",
            "SDK Tools, tick \"Show Package Details\", under NDK (Side by side) tick $wanted",
            "(and CMake), Apply. Or from a terminal:",
            "    sdkmanager --install \"ndk;$wanted\"",
            "or set ANDROID_NDK_HOME to an NDK you have.",
        )
    }

    private fun capture(cmd: List<String>, dir: File, env: Map<String, String>): Pair<Int, String> {
        val out = ByteArrayOutputStream()
        val result = exec.exec {
            commandLine(cmd)
            workingDir = dir
            environment(env)
            standardOutput = out
            errorOutput = out
            isIgnoreExitValue = true
        }
        return result.exitValue to out.toString()
    }

    @TaskAction
    fun build() {
        val desktop = workspace.get().asFile
        val cargo = findCargo() ?: fail(
            "dm: cargo (Rust) was not found on PATH, in CARGO_HOME/bin or in ~/.cargo/bin.",
            "Install Rust with rustup (https://rustup.rs), then build again:",
            "    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh",
        )
        val bin = cargo.parentFile
        val ndk = findNdk()
        val env = mapOf(
            "PATH" to listOf(bin.absolutePath, System.getenv("PATH") ?: "")
                .filter { it.isNotEmpty() }.joinToString(File.pathSeparator),
            "ANDROID_NDK_HOME" to ndk.absolutePath,
        )

        val (ndkExit, ndkOut) = capture(listOf(cargo.absolutePath, "ndk", "--version"), desktop, env)
        if (ndkExit != 0) {
            fail(
                "dm: cargo-ndk is not installed (`cargo ndk --version`: " +
                    "${ndkOut.trim().lines().firstOrNull().orEmpty()}).",
                "Install it, then build again:",
                "    cargo install cargo-ndk",
            )
        }

        // The Rust targets, of the toolchain rust-toolchain.toml names (run from the repository's root).
        val targets = abis.get().map { rustTargets.get().getValue(it) }
        val rustup = File(bin, exe("rustup"))
        if (rustup.isFile) {
            val (code, listed) = capture(
                listOf(rustup.absolutePath, "target", "list", "--installed"),
                desktop,
                env,
            )
            if (code == 0) {
                val installed = listed.lines().map { it.trim() }.toSet()
                val missing = targets.filter { it !in installed }
                if (missing.isNotEmpty()) {
                    fail(
                        "dm: Rust's ${missing.joinToString()} target is not installed.",
                        "Install it from the repository's folder (so it goes to the Rust",
                        "rust-toolchain.toml names), then build again:",
                        "    cd ${desktop.absolutePath} && rustup target add ${missing.joinToString(" ")}",
                    )
                }
            } else {
                logger.warn("dm: `rustup target list --installed` failed; the targets are not checked")
            }
        }

        // A clean folder: no other processor's library left in it.
        val out = jniLibs.get().asFile
        out.deleteRecursively()
        out.mkdirs()
        val cmd = mutableListOf(cargo.absolutePath, "ndk", "--platform", platform.get().toString())
        abis.get().forEach { cmd += listOf("-t", it) }
        cmd += listOf("-o", out.absolutePath, "build", "-p", "dm-android", "--lib")
        if (release.get()) cmd += "--release"
        logger.lifecycle("dm: in ${desktop.absolutePath}: ${cmd.joinToString(" ")}")
        val result = exec.exec {
            commandLine(cmd)
            workingDir = desktop
            environment(env)
            isIgnoreExitValue = true
        }
        if (result.exitValue != 0) {
            fail(
                "dm: cargo ndk failed (exit ${result.exitValue}); its output is above.",
                "The same from a terminal:",
                "    cd ${desktop.absolutePath} && ANDROID_NDK_HOME=${ndk.absolutePath} " +
                    cmd.drop(1).joinToString(" ", prefix = "cargo "),
            )
        }
        for (abi in abis.get()) {
            if (!File(out, "$abi/libdm_android.so").isFile) {
                fail("dm: cargo ndk finished but left no $abi/libdm_android.so in $out")
            }
        }
    }
}

val dmCargoSources = files(
    fileTree(dmWorkspace.resolve("crates")) {
        // This Gradle project: its build/, .gradle/ and jniLibs are outputs, not sources.
        exclude("dm-android/android/**")
    },
    fileTree(dmWorkspace.resolve("vendor")),
    fileTree(dmWorkspace.resolve("assets")),
    dmWorkspace.resolve("Cargo.toml"),
    dmWorkspace.resolve("Cargo.lock"),
    dmWorkspace.resolve(".cargo/config.toml"),
    dmWorkspace.resolve("rust-toolchain.toml"),
)

val dmCargoTasks = listOf("debug", "release").associateWith { type ->
    tasks.register<CargoNdkTask>("cargoNdk" + type.replaceFirstChar { it.uppercase() }) {
        group = "dm"
        description = "Builds libdm_android.so ($type) with cargo-ndk for ${dmAbis.joinToString()}"
        enabled = !dmSkipCargo
        abis.set(dmAbis)
        rustTargets.set(dmRustTargets)
        release.set(type == "release")
        platform.set(dmMinSdk)
        ndkVersion.set(dmNdkVersion)
        sdkDir.set(dmSdkDir)
        workspace.set(dmWorkspace)
        sources.from(dmCargoSources)
        jniLibs.set(layout.buildDirectory.dir("rustJniLibs/$type"))
    }
}

// `gradle cargoNdk`: the debug library alone.
tasks.register("cargoNdk") {
    group = "dm"
    description = "Builds the debug libdm_android.so with cargo-ndk"
    dependsOn(dmCargoTasks.getValue("debug"))
}

// Each build type's preBuild (every build of that type starts from it) waits for its library;
// with -Pdm.skipCargo=true the cargoNdk tasks are skipped.
tasks.configureEach {
    when (name) {
        "preDebugBuild" -> dependsOn(dmCargoTasks.getValue("debug"))
        "preReleaseBuild" -> dependsOn(dmCargoTasks.getValue("release"))
    }
}
