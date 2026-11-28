// EDM's APK (docs/android.md, gaps row N14): one module, `app`, which packs the
// libedm_android.so cargo-ndk builds with the few Java classes the activity needs
// (../java). No Kotlin sources; Kotlin DSL for the build files only.

pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "EDM"
include(":app")
