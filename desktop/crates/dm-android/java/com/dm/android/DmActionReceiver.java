// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;

/**
 * A notification's button or the schedule's alarm (gaps rows N6, N12), not exported: dm's own
 * PendingIntents alone reach it. The action ({@code com.dm.android.RETRY}, {@code STOP_ALL},
 * {@code OPEN}, {@code SCHEDULE}), the row and the notification are handed to Rust
 * ({@code src/android/receivers.rs}), which runs what the window runs.
 */
public class DmActionReceiver extends BroadcastReceiver {
    static {
        System.loadLibrary("dm_android");
    }

    @Override
    public void onReceive(Context context, Intent intent) {
        received(context, intent.getAction(),
                intent.getLongExtra("com.dm.android.ROW", -1L),
                intent.getIntExtra("com.dm.android.NOTE", -1));
    }

    private static native void received(Context context, String action, long row, int note);
}
