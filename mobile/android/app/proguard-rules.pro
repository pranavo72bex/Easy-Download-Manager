# Copyright (C) 2026 pranavo72bex and the dm contributors.
# SPDX-License-Identifier: GPL-3.0-or-later
# Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
# with the attribution term in NOTICE. See the LICENSE and NOTICE files.

# Not used while release builds are not minified (app/build.gradle.kts); kept for when they are.
# JNA finds its classes and the bindings' callback interfaces by reflection.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class uniffi.dm_android.** { *; }
-dontwarn java.awt.**
