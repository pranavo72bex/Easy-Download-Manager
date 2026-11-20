// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android.data

import kotlinx.coroutines.flow.StateFlow
import uniffi.dm_android.Row

/** A call the engine answered, or refused with its reason in dm's words. */
sealed interface Outcome<out T> {
    data class Done<T>(val value: T) : Outcome<T>
    data class Refused(val reason: String) : Outcome<Nothing>
}

/** What adding links gave: how many rows were made, and each link left out with why. */
data class AddResult(val added: Int, val skipped: List<String>)

/**
 * What the screens ask of dm. The app's is the engine (`EngineSource`); a test's is a fake,
 * so the screens run without `libdm_android.so`. Every call may block: the view model makes
 * them off the main thread.
 */
interface DownloadsSource {
    /** Goes up each time the list changes (the engine's `changed()`). */
    val ticks: StateFlow<Long>

    /** Every row, in the list's order; empty where dm could not start. */
    fun list(): List<Row>

    /** Why dm could not start, where it could not. */
    fun startFailure(): String?

    /** Whether anything downloads or waits its turn. */
    fun busy(): Boolean

    fun add(text: String): Outcome<AddResult>
    fun pause(id: ULong)
    fun resume(id: ULong)
    fun retry(id: ULong)
    fun cancel(id: ULong)
    fun remove(id: ULong)
    fun pauseAll(): Int
    fun resumeAll(): Int
    fun clearFinished(): Int

    /** Opens a finished file; why not, if it could not. */
    fun open(path: String): String?

    /** Shows where a finished file is; why not, if it could not. */
    fun reveal(path: String): String?

    /** The clipboard's text, where there is any. */
    fun clipboard(): String?

    /** The settings in force, as the JSON dm saves them as. */
    fun settingsJson(): Outcome<String>

    /** Puts `json` in force, whole or not at all; the settings now in force come back. */
    fun saveSettings(json: String): Outcome<String>

    /** What went wrong at start and was put right, said once. */
    fun takeNotices(): List<String>
}
