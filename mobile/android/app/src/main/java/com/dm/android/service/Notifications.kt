// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android.service

import android.Manifest
import android.annotation.SuppressLint
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import com.dm.android.MainActivity
import com.dm.android.R
import com.dm.android.data.Format
import com.dm.android.engine.Files
import java.util.concurrent.atomic.AtomicInteger

/** dm's notifications: the service's ongoing one, and one for each download finished or failed. */
object Notifications {
    const val CHANNEL_PROGRESS = "progress"
    const val CHANNEL_DONE = "done"
    const val ONGOING_ID = 1
    private const val FAILED_BASE = 1_000_000
    private val nextId = AtomicInteger(1_000)

    /** Made once, when the app starts. */
    fun createChannels(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java) ?: return
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_PROGRESS,
                context.getString(R.string.channel_progress),
                NotificationManager.IMPORTANCE_LOW,
            ),
        )
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_DONE,
                context.getString(R.string.channel_done),
                NotificationManager.IMPORTANCE_DEFAULT,
            ),
        )
    }

    /** The service's: how many download, at what speed, and Stop All. */
    fun ongoing(context: Context, running: Int, bytesPerSecond: Double): Notification {
        val body = if (running > 0) {
            context.getString(R.string.ongoing_body, running, Format.speed(bytesPerSecond))
        } else {
            context.getString(R.string.ongoing_waiting)
        }
        return NotificationCompat.Builder(context, CHANNEL_PROGRESS)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(context.getString(R.string.ongoing_title))
            .setContentText(body)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setContentIntent(openApp(context))
            .addAction(0, context.getString(R.string.action_stop_all), DownloadService.pauseAllIntent(context))
            .build()
    }

    /** A download finished (`path`, opened by a tap) or failed (`failed`, with Retry). */
    fun download(context: Context, title: String, body: String, path: String?, failed: ULong?) {
        val id = failed?.let { idForFailure(it) } ?: nextId.incrementAndGet()
        val builder = NotificationCompat.Builder(context, CHANNEL_DONE)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(title)
            .setContentText(body)
            .setStyle(NotificationCompat.BigTextStyle().bigText(body))
            .setAutoCancel(true)
            .setContentIntent(path?.let { openFile(context, it, id) } ?: openApp(context))
        if (failed != null) {
            builder.addAction(0, context.getString(R.string.action_retry), RetryReceiver.intent(context, failed, id))
        }
        post(context, id, builder.build())
    }

    /** Shows `notification` where the person allows dm's notifications. */
    @SuppressLint("MissingPermission") // allowed() is the check.
    fun post(context: Context, id: Int, notification: Notification) {
        if (!allowed(context)) return
        try {
            NotificationManagerCompat.from(context).notify(id, notification)
        } catch (e: SecurityException) {
            // The permission was taken back between the check and the call.
        }
    }

    fun cancel(context: Context, id: Int) {
        NotificationManagerCompat.from(context).cancel(id)
    }

    private fun allowed(context: Context): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) ==
            PackageManager.PERMISSION_GRANTED

    private fun idForFailure(row: ULong): Int = FAILED_BASE + (row % 1_000_000uL).toInt()

    private fun openApp(context: Context): PendingIntent {
        val intent = Intent(context, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
        return PendingIntent.getActivity(context, 0, intent, FLAGS)
    }

    private fun openFile(context: Context, path: String, id: Int): PendingIntent? = try {
        PendingIntent.getActivity(context, id, Files.viewIntent(context, path), FLAGS)
    } catch (e: IllegalArgumentException) {
        null
    }

    const val FLAGS = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
}
