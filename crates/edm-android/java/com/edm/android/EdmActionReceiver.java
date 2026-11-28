// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.edm.android;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;

/**
 * A notification's button or the schedule's alarm (gaps rows N6, N12), not exported: EDM's own
 * PendingIntents alone reach it. The action ({@code com.edm.android.RETRY}, {@code STOP_ALL},
 * {@code OPEN}, {@code SCHEDULE}), the row and the notification are handed to Rust
 * ({@code src/android/receivers.rs}), which runs what the window runs.
 */
public class EdmActionReceiver extends BroadcastReceiver {
    static {
        System.loadLibrary("edm_android");
    }

    @Override
    public void onReceive(Context context, Intent intent) {
        received(context, intent.getAction(),
                intent.getLongExtra("com.edm.android.ROW", -1L),
                intent.getIntExtra("com.edm.android.NOTE", -1));
    }

    private static native void received(Context context, String action, long row, int note);
}
