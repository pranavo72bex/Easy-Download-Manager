// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android.data

import java.util.Locale

/** Sizes, speeds and times as the list and the notification show them. Pure, so unit-tested. */
object Format {
    private val UNITS = listOf("KB", "MB", "GB", "TB", "PB")

    /** `1536` as "1.5 KB": 1024 to a unit, one decimal below 100, none above. */
    fun bytes(n: Long): String {
        if (n < 1024) return "$n B"
        var value = n.toDouble() / 1024
        var unit = 0
        while (value >= 1024 && unit < UNITS.lastIndex) {
            value /= 1024
            unit++
        }
        val number = if (value < 100) String.format(Locale.ROOT, "%.1f", value)
        else String.format(Locale.ROOT, "%.0f", value)
        return "$number ${UNITS[unit]}"
    }

    /** Bytes a second, as "1.5 MB/s". */
    fun speed(bytesPerSecond: Double): String {
        val whole = if (bytesPerSecond.isFinite() && bytesPerSecond > 0) bytesPerSecond.toLong() else 0L
        return "${bytes(whole)}/s"
    }

    /** Seconds left as "45s", "3m 05s", "2h 07m" or "3d 04h". */
    fun eta(seconds: Long): String {
        val s = seconds.coerceAtLeast(0)
        return when {
            s < 60 -> "${s}s"
            s < 3600 -> String.format(Locale.ROOT, "%dm %02ds", s / 60, s % 60)
            s < 86_400 -> String.format(Locale.ROOT, "%dh %02dm", s / 3600, (s % 3600) / 60)
            else -> String.format(Locale.ROOT, "%dd %02dh", s / 86_400, (s % 86_400) / 3600)
        }
    }

    /** "12.0 MB of 40.0 MB", "≈" before an estimated total, the downloaded size alone if none. */
    fun progress(downloaded: Long, total: Long?, estimated: Boolean): String {
        if (total == null || total <= 0) return bytes(downloaded)
        val mark = if (estimated) "≈" else ""
        return "${bytes(downloaded)} of $mark${bytes(total)}"
    }

    /** How far, 0 to 1; null where the total is not known (an indefinite bar). */
    fun fraction(downloaded: Long, total: Long?): Float? {
        if (total == null || total <= 0) return null
        return (downloaded.toDouble() / total).coerceIn(0.0, 1.0).toFloat()
    }

    /**
     * A JSON key or a category as words: `watch_clipboard` as "Watch clipboard",
     * `maxConnections` as "Max connections", `videos` as "Videos".
     */
    fun label(key: String): String {
        val words = key
            .replace(Regex("([a-z0-9])([A-Z])"), "$1 $2")
            .split('_', '-', ' ', '.')
            .filter { it.isNotEmpty() }
            .map { it.lowercase(Locale.ROOT) }
        if (words.isEmpty()) return key
        val sentence = words.joinToString(" ")
        return sentence.replaceFirstChar { it.titlecase(Locale.ROOT) }
    }
}
