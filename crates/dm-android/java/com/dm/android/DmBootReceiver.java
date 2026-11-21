// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;

/**
 * The phone has started (gaps row N12): handed to Rust ({@code src/android/receivers.rs}),
 * which sets the schedule's alarm again and says when dm has downloads to go on with.
 */
public class DmBootReceiver extends BroadcastReceiver {
    static {
        System.loadLibrary("dm_android");
    }

    @Override
    public void onReceive(Context context, Intent intent) {
        if (Intent.ACTION_BOOT_COMPLETED.equals(intent.getAction())) {
            booted(context);
        }
    }

    private static native void booted(Context context);
}
