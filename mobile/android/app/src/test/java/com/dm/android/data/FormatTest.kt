// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android.data

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class FormatTest {
    @Test
    fun bytesAreGivenInTheirUnit() {
        assertEquals("0 B", Format.bytes(0))
        assertEquals("1023 B", Format.bytes(1023))
        assertEquals("1.0 KB", Format.bytes(1024))
        assertEquals("1.5 MB", Format.bytes(1536L * 1024))
        assertEquals("100 GB", Format.bytes(100L * 1024 * 1024 * 1024))
    }

    @Test
    fun speedIsBytesASecond() {
        assertEquals("1.5 MB/s", Format.speed(1536.0 * 1024))
        assertEquals("0 B/s", Format.speed(Double.NaN))
        assertEquals("0 B/s", Format.speed(-5.0))
    }

    @Test
    fun timeLeftIsInItsTwoLargestUnits() {
        assertEquals("45s", Format.eta(45))
        assertEquals("3m 05s", Format.eta(185))
        assertEquals("2h 07m", Format.eta(2 * 3600 + 7 * 60 + 30))
        assertEquals("3d 04h", Format.eta(3 * 86_400 + 4 * 3600))
        assertEquals("0s", Format.eta(-1))
    }

    @Test
    fun progressMarksAnEstimate() {
        assertEquals("1.0 KB of 2.0 KB", Format.progress(1024, 2048, false))
        assertEquals("1.0 KB of ≈2.0 KB", Format.progress(1024, 2048, true))
        assertEquals("1.0 KB", Format.progress(1024, null, false))
        assertNull(Format.fraction(5, null))
        assertEquals(0.5f, Format.fraction(1, 2)!!, 0.0001f)
        assertEquals(1f, Format.fraction(3, 2)!!, 0.0001f)
    }

    @Test
    fun keysReadAsWords() {
        assertEquals("Watch clipboard", Format.label("watch_clipboard"))
        assertEquals("Max connections", Format.label("maxConnections"))
        assertEquals("Videos", Format.label("videos"))
        assertEquals("Download dir", Format.label("download_dir"))
        assertEquals("", Format.label(""))
    }
}
