// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android.share

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import java.io.File
import java.io.IOException

/** What an intent handed dm: the text for the Add sheet, or why it could not be read. */
sealed interface Incoming {
    data class Text(val text: String) : Incoming
    data class Unreadable(val why: String) : Incoming
}

/**
 * The links another app hands dm (gaps row N5): a share's text, a magnet opened, a `.torrent`
 * opened from Files or a browser's downloads. Blocking: a `.torrent` is copied.
 */
object IncomingLinks {
    private const val MAX_TORRENT_BYTES = 32L * 1024 * 1024

    fun from(context: Context, intent: Intent?): Incoming? {
        intent ?: return null
        return when (intent.action) {
            Intent.ACTION_SEND -> shared(intent)
            Intent.ACTION_VIEW -> opened(context, intent.data ?: return null)
            else -> null
        }
    }

    private fun shared(intent: Intent): Incoming? {
        val parts = listOfNotNull(
            intent.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString(),
            intent.getStringExtra(Intent.EXTRA_SUBJECT),
        ).filter { it.isNotBlank() }
        return if (parts.isEmpty()) null else Incoming.Text(parts.joinToString("\n"))
    }

    private fun opened(context: Context, uri: Uri): Incoming? = when (uri.scheme?.lowercase()) {
        "magnet" -> Incoming.Text(uri.toString())
        "content", "file" -> torrent(context, uri)
        else -> null
    }

    /**
     * The `.torrent` copied into dm's own files (not the cache, which the system may empty
     * while the torrent still runs), handed on as its `file://` address.
     */
    private fun torrent(context: Context, uri: Uri): Incoming = try {
        val folder = File(context.filesDir, "torrents").apply { mkdirs() }
        val file = File(folder, "${System.currentTimeMillis()}-${nameOf(context, uri)}")
        copy(context, uri, file)
        Incoming.Text(Uri.fromFile(file).toString())
    } catch (e: IOException) {
        Incoming.Unreadable(e.message ?: e.toString())
    } catch (e: SecurityException) {
        Incoming.Unreadable(e.message ?: e.toString())
    }

    private fun copy(context: Context, uri: Uri, to: File) {
        val input = context.contentResolver.openInputStream(uri) ?: throw IOException(uri.toString())
        input.use { source ->
            to.outputStream().use { sink ->
                val buffer = ByteArray(64 * 1024)
                var total = 0L
                while (true) {
                    val n = source.read(buffer)
                    if (n < 0) break
                    total += n
                    if (total > MAX_TORRENT_BYTES) throw IOException("larger than a .torrent file can be")
                    sink.write(buffer, 0, n)
                }
            }
        }
    }

    /** The file's own name, made safe and ending in `.torrent`. */
    private fun nameOf(context: Context, uri: Uri): String {
        val shown = try {
            context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
                ?.use { if (it.moveToFirst()) it.getString(0) else null }
        } catch (e: RuntimeException) {
            null
        }
        val raw = shown ?: uri.lastPathSegment ?: "download"
        val safe = raw.substringAfterLast('/').replace(Regex("[^A-Za-z0-9._ -]"), "_").take(120)
        val name = safe.ifBlank { "download" }
        return if (name.endsWith(".torrent", ignoreCase = true)) name else "$name.torrent"
    }
}
