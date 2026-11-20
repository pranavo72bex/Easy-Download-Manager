// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

// The engine is desktop/crates/dm-android, built by cargo-ndk into jniLibs; its Kotlin bindings
// are generated from the built library into build/generated/uniffi (gaps row N3). Neither is
// checked in. The switches are in gradle.properties.
val cargo: String = providers.gradleProperty("dm.cargo").getOrElse("cargo")
val rustProfile: String = providers.gradleProperty("dm.rustProfile").getOrElse("release")
val abis: List<String> = providers.gradleProperty("dm.abis")
    .getOrElse("arm64-v8a,armeabi-v7a,x86_64")
    .split(",").map { it.trim() }.filter { it.isNotEmpty() }
val skipRust: Boolean = providers.gradleProperty("dm.skipRust").getOrElse("false").toBoolean()
val desktopDir: File = rootDir.resolve("../../desktop").canonicalFile
val jniLibsDir: File = projectDir.resolve("src/main/jniLibs")
val uniffiDir: File = layout.buildDirectory.dir("generated/uniffi").get().asFile

/** cargo's flag for the chosen profile. */
fun profileArgs(): List<String> = when (rustProfile) {
    "release" -> listOf("--release")
    "debug", "dev" -> emptyList()
    else -> listOf("--profile", rustProfile)
}

val cargoNdk = tasks.register<Exec>("cargoNdk") {
    group = "rust"
    description = "Builds libdm_android.so for each ABI with cargo-ndk."
    onlyIf { !skipRust }
    workingDir = desktopDir
    val args = mutableListOf(cargo, "ndk", "--platform", "26")
    abis.forEach { args += listOf("-t", it) }
    args += listOf("-o", jniLibsDir.absolutePath, "build", "-p", "dm-android")
    args += profileArgs()
    commandLine(args)
}

val uniffiBindgen = tasks.register<Exec>("uniffiBindgen") {
    group = "rust"
    description = "Generates the Kotlin bindings (package uniffi.dm_android) from the built library."
    dependsOn(cargoNdk)
    onlyIf { !skipRust }
    workingDir = desktopDir
    val library = jniLibsDir.resolve("${abis.first()}/libdm_android.so")
    commandLine(
        cargo, "run", "-p", "dm-android", "--features", "bindgen", "--bin", "uniffi-bindgen",
        "--", "generate", "--library", library.absolutePath,
        "--language", "kotlin", "--out-dir", uniffiDir.absolutePath,
    )
}

tasks.named("preBuild") { dependsOn(uniffiBindgen) }

android {
    namespace = "com.dm.android"
    compileSdk = 35

    defaultConfig {
        // The desktop's bundle is com.dm.desktop (dm-gpui's packager): the phone's is its sibling.
        applicationId = "com.dm.android"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.10.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        ndk { abiFilters += abis }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    sourceSets {
        getByName("main") { java.srcDir(uniffiDir) }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures { compose = true }

    packaging {
        resources { excludes += "/META-INF/{AL2.0,LGPL2.1}" }
    }
}

kotlin {
    compilerOptions { jvmTarget.set(JvmTarget.JVM_17) }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.ui.tooling.preview)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.material.icons.core)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)
    // UniFFI's Kotlin reaches libdm_android.so through JNA; the AAR carries JNA's own native part.
    implementation(variantOf(libs.jna) { artifactType("aar") })

    testImplementation(libs.junit)

    androidTestImplementation(libs.androidx.test.ext.junit)
    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(platform(libs.androidx.compose.bom))
    androidTestImplementation(libs.androidx.compose.ui.test.junit4)
    debugImplementation(libs.androidx.compose.ui.tooling)
    debugImplementation(libs.androidx.compose.ui.test.manifest)
}
