// Copyright (C) 2026 pranavo72bex and the EDM contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (EDM): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.edm.android;

import android.app.Service;
import android.content.Intent;
import android.os.IBinder;

/**
 * EDM's foreground service (gaps row N6, type dataSync): while it runs Android keeps EDM's
 * process, so downloads go on with EDM in the background or the screen off. Rust starts it when
 * EDM becomes busy and stops it when not; each event here is handed to Rust
 * ({@code src/android/service.rs}), which puts up the ongoing notification with
 * {@code startForeground}.
 */
public class EdmService extends Service {
    static {
        // Started without the activity (an alarm): the natives need the library loaded.
        System.loadLibrary("edm_android");
    }

    @Override
    public int onStartCommand(Intent intent, int flags, int startId) {
        started(this);
        return START_NOT_STICKY;
    }

    /** Android 15: a dataSync service's time is up. */
    @Override
    public void onTimeout(int startId, int foregroundServiceType) {
        timedOut(this);
    }

    @Override
    public void onDestroy() {
        stopped();
        super.onDestroy();
    }

    @Override
    public IBinder onBind(Intent intent) {
        return null;
    }

    private static native void started(EdmService service);

    private static native void timedOut(EdmService service);

    private static native void stopped();
}
