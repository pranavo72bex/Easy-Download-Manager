// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android.service

import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import com.dm.android.engine.EngineHolder
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import uniffi.dm_android.Engine
import uniffi.dm_android.RowState

/**
 * Keeps dm downloading with the screen off or the window swiped away (gaps row N6): a
 * foreground service of type `dataSync` holding the app's engine while anything downloads or
 * waits its turn, its notification showing how many run, their speed, and Stop All. It stops
 * itself once nothing has been busy for [IDLE_SECONDS].
 */
class DownloadService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        val type = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC
        } else {
            0
        }
        ServiceCompat.startForeground(this, Notifications.ONGOING_ID, Notifications.ongoing(this, 0, 0.0), type)
        scope.launch { watch() }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_PAUSE_ALL) {
            scope.launch { EngineHolder.get(this@DownloadService)?.pauseAll() }
        }
        return START_STICKY
    }

    /** Android 15 ends a `dataSync` service after six hours a day: it is let go, saved. */
    override fun onTimeout(startId: Int, fgsType: Int) {
        stopSelf()
    }

    override fun onDestroy() {
        scope.cancel()
        // With the window open the engine stays for it; otherwise it saves and goes.
        if (!EngineHolder.windowShown) EngineHolder.shutDown()
        super.onDestroy()
    }

    /** Draws the notification at most once a second, and stops when idle long enough. */
    private suspend fun watch() {
        var idle = 0
        var seen = -1L
        while (scope.isActive) {
            val engine = EngineHolder.get(this) ?: break
            val tick = EngineHolder.ticks.value
            val busy = engine.busy()
            if (tick != seen || busy) {
                seen = tick
                draw(engine)
            }
            idle = if (busy) 0 else idle + 1
            if (idle >= IDLE_SECONDS) break
            delay(1_000)
        }
        stopSelf()
    }

    private fun draw(engine: Engine) {
        val running = engine.list().filter { it.state == RowState.RUNNING }
        val notification = Notifications.ongoing(this, running.size, running.sumOf { it.speed })
        Notifications.post(this, Notifications.ONGOING_ID, notification)
    }

    companion object {
        private const val ACTION_PAUSE_ALL = "com.dm.android.PAUSE_ALL"
        private const val IDLE_SECONDS = 5

        /** Starts the service, or tells the running one nothing; safe to call often. */
        fun start(context: Context) {
            val intent = Intent(context, DownloadService::class.java)
            try {
                ContextCompat.startForegroundService(context, intent)
            } catch (e: IllegalStateException) {
                // Android 12 and up refuse a start from the background; the next start from
                // the window, a notification or N12's alarm brings it.
            }
        }

        fun pauseAllIntent(context: Context): PendingIntent {
            val intent = Intent(context, DownloadService::class.java).setAction(ACTION_PAUSE_ALL)
            return PendingIntent.getService(context, 1, intent, Notifications.FLAGS)
        }
    }
}
