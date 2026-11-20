// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android.service

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import com.dm.android.engine.EngineHolder

/** A failed download's notification's Retry: tries it again and brings the service back. */
class RetryReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val row = intent.getLongExtra(EXTRA_ROW, -1L)
        if (row < 0) return
        Notifications.cancel(context, intent.getIntExtra(EXTRA_NOTIFICATION, 0))
        val app = context.applicationContext
        val pending = goAsync()
        // The engine's calls block; a receiver's main thread must not.
        Thread {
            try {
                EngineHolder.get(app)?.retry(row.toULong())
                DownloadService.start(app)
            } finally {
                pending.finish()
            }
        }.start()
    }

    companion object {
        private const val EXTRA_ROW = "row"
        private const val EXTRA_NOTIFICATION = "notification"

        fun intent(context: Context, row: ULong, notification: Int): PendingIntent {
            val intent = Intent(context, RetryReceiver::class.java)
                .putExtra(EXTRA_ROW, row.toLong())
                .putExtra(EXTRA_NOTIFICATION, notification)
            return PendingIntent.getBroadcast(context, notification, intent, Notifications.FLAGS)
        }
    }
}
