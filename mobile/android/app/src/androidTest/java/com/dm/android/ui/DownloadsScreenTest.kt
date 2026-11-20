// Copyright (C) 2026 pranavo72bex and the dm contributors.
// SPDX-License-Identifier: GPL-3.0-or-later
// Part of Easy Download Manager (dm): GNU General Public License v3.0 or later,
// with the attribution term in NOTICE. See the LICENSE and NOTICE files.

package com.dm.android.ui

import androidx.compose.material3.SnackbarHostState
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.ComposeContentTestRule
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.dm.android.data.AddResult
import com.dm.android.data.DownloadsSource
import com.dm.android.data.Outcome
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.dm_android.Row
import uniffi.dm_android.RowState

/** The list over a fake source: no `libdm_android.so` is loaded (gaps row N13). */
@RunWith(AndroidJUnit4::class)
class DownloadsScreenTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun theListShowsEachRowAndTheChipsFilterIt() {
        val source = FakeSource(
            listOf(
                row(1u, "film.mkv", RowState.RUNNING),
                row(2u, "song.mp3", RowState.COMPLETED),
                row(3u, "broken.zip", RowState.FAILED),
            ),
        )
        compose.setContent { DmTheme { DownloadsScreen(DownloadsViewModel(source), SnackbarHostState()) } }
        compose.waitUntil(5_000) { compose.count("film.mkv") > 0 }
        compose.onNodeWithText("song.mp3").assertIsDisplayed()
        compose.onNodeWithTag("chip-FAILED").performClick()
        compose.waitUntil(5_000) { compose.count("film.mkv") == 0 }
        compose.onNodeWithText("broken.zip").assertIsDisplayed()
    }

    @Test
    fun stopAllAsksTheSource() {
        val source = FakeSource(listOf(row(1u, "film.mkv", RowState.RUNNING)))
        compose.setContent { DmTheme { DownloadsScreen(DownloadsViewModel(source), SnackbarHostState()) } }
        compose.waitUntil(5_000) { compose.count("film.mkv") > 0 }
        compose.onNode(hasContentDescription("More")).performClick()
        compose.onNodeWithText("Stop All").performClick()
        compose.waitUntil(5_000) { source.pausedAll == 1 }
        assertEquals(1, source.pausedAll)
    }

    private fun ComposeContentTestRule.count(text: String) =
        onAllNodes(hasText(text)).fetchSemanticsNodes().size

    private fun row(id: ULong, name: String, state: RowState) = Row(
        id = id,
        url = "https://example.com/$name",
        name = name,
        state = state,
        failure = if (state == RowState.FAILED) "the server said 404" else null,
        downloaded = 512uL,
        total = 1024uL,
        estimated = false,
        speed = if (state == RowState.RUNNING) 2048.0 else 0.0,
        etaSeconds = if (state == RowState.RUNNING) 30uL else null,
        path = "/sdcard/Download/dm/$name",
        category = null,
        torrent = false,
        swarm = null,
    )
}

/** A source with fixed rows that counts what it is asked. */
private class FakeSource(private val rows: List<Row>) : DownloadsSource {
    override val ticks: StateFlow<Long> = MutableStateFlow(0L)
    @Volatile
    var pausedAll = 0

    override fun list(): List<Row> = rows
    override fun startFailure(): String? = null
    override fun busy(): Boolean = rows.any { it.state == RowState.RUNNING }
    override fun add(text: String): Outcome<AddResult> = Outcome.Done(AddResult(1, emptyList()))
    override fun pause(id: ULong) = Unit
    override fun resume(id: ULong) = Unit
    override fun retry(id: ULong) = Unit
    override fun cancel(id: ULong) = Unit
    override fun remove(id: ULong) = Unit
    override fun pauseAll(): Int {
        pausedAll++
        return 1
    }
    override fun resumeAll(): Int = 0
    override fun clearFinished(): Int = 0
    override fun open(path: String): String? = null
    override fun reveal(path: String): String? = null
    override fun clipboard(): String? = null
    override fun settingsJson(): Outcome<String> = Outcome.Done("""{"watch_clipboard":false}""")
    override fun saveSettings(json: String): Outcome<String> = Outcome.Done(json)
    override fun takeNotices(): List<String> = emptyList()
}
