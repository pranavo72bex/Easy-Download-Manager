// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.dm_android.Row
import uniffi.dm_android.RowState

class LinksTest {
    @Test
    fun linksAreFoundInASharedTitle() {
        val text = "Look at this: https://example.com/a.zip. And magnet:?xt=urn:btih:abc\nhttps://example.com/a.zip"
        assertEquals(listOf("https://example.com/a.zip", "magnet:?xt=urn:btih:abc"), Links.found(text))
        assertTrue(Links.found("no link here").isEmpty())
    }

    @Test
    fun settingsAreReadByKey() {
        val json = """{"watch_clipboard":true,"download_dir":"/d","n":3}"""
        assertTrue(Links.switchOn(json, "watch_clipboard"))
        assertFalse(Links.switchOn(json, "n"))
        assertFalse(Links.switchOn("not json", "watch_clipboard"))
        assertEquals("/d", Links.text(json, "download_dir"))
        assertNull(Links.text(json, "n"))
    }

    @Test
    fun theChipsFilterByStateAndCategory() {
        val rows = listOf(
            row(1u, RowState.RUNNING, "videos"),
            row(2u, RowState.COMPLETED, "videos"),
            row(3u, RowState.FAILED, null),
            row(4u, RowState.QUEUED, "music"),
        )
        fun ids(filter: ListFilter) = rows.filter { filter.shows(it) }.map { it.id.toInt() }
        assertEquals(listOf(1, 2, 3, 4), ids(ListFilter()))
        assertEquals(listOf(1, 4), ids(ListFilter(StateFilter.RUNNING)))
        assertEquals(listOf(2), ids(ListFilter(StateFilter.FINISHED)))
        assertEquals(listOf(3), ids(ListFilter(StateFilter.FAILED)))
        assertEquals(listOf(1, 2), ids(ListFilter(category = "videos")))
        assertEquals(listOf("videos", "music"), ListFilter.categories(rows))
    }

    private fun row(id: ULong, state: RowState, category: String?) = Row(
        id = id,
        url = "https://example.com/$id",
        name = "file$id",
        state = state,
        failure = null,
        downloaded = 0uL,
        total = null,
        estimated = false,
        speed = 0.0,
        etaSeconds = null,
        path = "",
        category = category,
        torrent = false,
        swarm = null,
    )
}
