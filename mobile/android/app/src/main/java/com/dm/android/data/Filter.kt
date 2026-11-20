// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android.data

import uniffi.dm_android.Row
import uniffi.dm_android.RowState

/** The list's chips by where a download has got to. */
enum class StateFilter { ALL, RUNNING, FINISHED, FAILED }

/** Which rows the list shows: a state chip and, optionally, one category. */
data class ListFilter(val state: StateFilter = StateFilter.ALL, val category: String? = null) {
    fun shows(row: Row): Boolean {
        val stateOk = when (state) {
            StateFilter.ALL -> true
            StateFilter.RUNNING -> row.state == RowState.RUNNING || row.state == RowState.QUEUED
            StateFilter.FINISHED -> row.state == RowState.COMPLETED
            StateFilter.FAILED -> row.state == RowState.FAILED
        }
        return stateOk && (category == null || row.category == category)
    }

    companion object {
        /** The categories the rows have been sorted as, in the order first seen. */
        fun categories(rows: List<Row>): List<String> = rows.mapNotNull { it.category }.distinct()
    }
}

/** Whether a row is on its way: the list polls each second while one is. */
fun Row.active(): Boolean = state == RowState.RUNNING || state == RowState.QUEUED
