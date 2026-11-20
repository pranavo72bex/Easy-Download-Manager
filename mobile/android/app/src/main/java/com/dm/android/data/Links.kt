// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android.data

import java.util.Locale
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull

/**
 * The links in a text, for the Add sheet's preview and the clipboard offer only: what is added
 * is what the engine finds itself (`dm_shell::app::links::links_in`), with the same starts.
 */
object Links {
    /** The starts of the addresses dm takes (`dm_shell::app::links::SCHEMES`). */
    private val SCHEMES = listOf(
        "http://", "https://", "ftp://", "magnet:?", "file://", "rtmp://", "rtmps://", "rtmpe://",
        "rtmpt://", "rtsp://", "rtsps://", "mms://", "mmsh://", "mmst://",
    )
    private val SPLIT = Regex("[\\s<>\"']+")
    private val TRAILING = charArrayOf('.', ',', ';', ':', '!', '?', ')', ']')

    fun found(text: String): List<String> = text.split(SPLIT)
        .map { it.trimEnd(*TRAILING) }
        .filter { word -> SCHEMES.any { word.lowercase(Locale.ROOT).startsWith(it) } }
        .distinct()

    /** A setting's switch in the settings' JSON (`watch_clipboard`, say); false if absent. */
    fun switchOn(settingsJson: String, key: String): Boolean = runCatching {
        val root = Json.parseToJsonElement(settingsJson) as? JsonObject
        (root?.get(key) as? JsonPrimitive)?.booleanOrNull == true
    }.getOrDefault(false)

    /** A setting's text in the settings' JSON (`download_dir`, say). */
    fun text(settingsJson: String, key: String): String? = runCatching {
        val root = Json.parseToJsonElement(settingsJson) as? JsonObject
        (root?.get(key) as? JsonPrimitive)?.takeIf { it.isString }?.content
    }.getOrNull()
}
