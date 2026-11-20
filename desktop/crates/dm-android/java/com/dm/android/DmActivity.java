// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android;

import android.content.Intent;

import dev.gpui.mobile.GpuiInputActivity;

/**
 * dm's activity (gaps row N5): gpui-mobile's, with a new intent handed to Rust. A link shared
 * while dm is open comes to the running activity (singleTask) as {@code onNewIntent}, which
 * NativeActivity keeps from native code; here it becomes the activity's intent and Rust reads
 * it as it reads the first one ({@code src/android/intent.rs}). Nothing else is done here.
 */
public class DmActivity extends GpuiInputActivity {
    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        newIntent();
    }

    /** {@code Java_com_dm_android_DmActivity_newIntent} in {@code src/android/natives.rs}. */
    private static native void newIntent();
}
